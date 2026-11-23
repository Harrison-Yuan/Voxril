//! 直播引擎 — 音频流构建 + 线程编排
//!
//! 职责：
//! - 创建 CPAL 输入/输出/监听流
//! - 启动修音中继线程（F0 检测 + Phase Vocoder）
//! - 启动伴奏/翻唱播放线程
//! - 启动歌词推送线程
//!
//! # 排错指南
//! - 找不到设备 → 检查 `config.input_device` / `config.output_device` 名称
//! - 启动超时 → 检查 `stop_rx` 是否在 120 秒内未收到 ready 事件
//! - 不出声 → 先看日志[引擎]，再看[mixer.rs]的混音逻辑

use crate::audio_engine::auto_tune;
use crate::audio_engine::config;
use crate::audio_engine::output_stage::{OutputStage, MAX_STAGE_BLOCK};
use crate::audio_engine::pitch_correction::{PitchCorrector, PV_FFT_SIZE};
use crate::audio_engine::resampler::StreamResampler;
use crate::audio_engine::rvc::{build_session, fcpe_f0, rmpe_f0};
use crate::audio_engine::smoother::StreamSmoother;
use crate::audio_engine::thread::boost_audio_thread_priority;
use crate::audio_engine::voice_state::{VoiceState, VoiceStateDetector};
use crate::audio_engine::{
    blackhole, common_decode_cover, emit_log, emit_progress, push_ringbuf,
    push_ringbuf_with_latency_cap, LiveSingingConfig, F0_UP_KEY,
};
use crate::audio_engine::{accompaniment, mixer};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{HeapConsumer, HeapProducer, HeapRb};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;
use tokio::sync::oneshot;

// ── 权限请求（仅在 macOS 需要） ──
#[cfg(target_os = "macos")]
fn request_microphone_permission() -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn request_microphone_permission() -> Result<(), String> {
    Ok(())
}

// ══════════════════════════════════════════════════════════════════
//  修音中继线程
// ══════════════════════════════════════════════════════════════════

fn run_correction_relay(
    vocal_prod: &mut HeapProducer<f32>,
    mut bh_vocal_prod: Option<&mut HeapProducer<f32>>,
    mic_cons: &mut HeapConsumer<f32>,
    config: LiveSingingConfig,
    running: Arc<AtomicBool>,
    app_handle: tauri::AppHandle,
) {
    boost_audio_thread_priority();

    let mut fcpe_sess = if !config.fcpe_path.is_empty() && Path::new(&config.fcpe_path).exists() {
        match build_session(&config.fcpe_path) {
            Ok(s) => { eprintln!("[修音] FCPE 加载成功"); Some(s) }
            Err(e) => { eprintln!("[修音] FCPE 失败: {}", e); None }
        }
    } else {
        None
    };

    let mut rmvpe_sess = if fcpe_sess.is_none()
        && !config.rmvpe_path.is_empty()
        && Path::new(&config.rmvpe_path).exists()
    {
        match build_session(&config.rmvpe_path) {
            Ok(s) => { eprintln!("[修音] RMVPE 兜底加载成功"); Some(s) }
            Err(e) => { eprintln!("[修音] RMVPE 失败: {}", e); None }
        }
    } else {
        None
    };

    let mut corrector = PitchCorrector::new();
    let mut auto_tune_engine = auto_tune::AutoTune::new();
    let mut mic_scratch = vec![0.0f32; 4096];
    let mut mic_buf: VecDeque<f32> = VecDeque::with_capacity(8192);
    let mut vad = VoiceStateDetector::new();
    let mut smoothed_pitch_ratio: f32 = 1.0;
    let mut processed: usize = 0;
    let mut next_metric_at: usize = 0;
    let mut last_metric_rms: f32 = 0.0;
    let mut last_metric_f0: f32 = 0.0;
    let mut last_metric_state: &str = "mute";

    eprintln!(
        "[修音] 就绪, FCPE={}, RMVPE={}",
        fcpe_sess.is_some(),
        rmvpe_sess.is_some()
    );
    const CHUNK_960: usize = 960;
    // 麦克风静音时复用同一段静音缓冲（长度恒等于 CHUNK_960，与修音输出契约一致）
    let silent_chunk = [0.0f32; CHUNK_960];

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let mut collected = 0usize;
        loop {
            let n = mic_cons.pop_slice(&mut mic_scratch);
            if n == 0 {
                break;
            }
            mic_buf.extend(mic_scratch[..n].iter().copied());
            collected += n;
        }
        if collected == 0 {
            // 麦克风回调只负责把数据推入环形缓冲，此处以 1ms 轮询等待，
            // 避免在实时回调与修音线程之间共享锁（锁竞争会导致爆音）。
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }

        const MAX_IN_WATERMARK: usize = 960 * 3;
        if mic_buf.len() > MAX_IN_WATERMARK {
            let excess = mic_buf.len() - MAX_IN_WATERMARK;
            let drop_chunks = (excess / CHUNK_960) + 1;
            for _ in 0..(drop_chunks * CHUNK_960) {
                mic_buf.pop_front();
            }
        }

        const MAX_OUT_WATERMARK: usize = 3840;

        while mic_buf.len() >= CHUNK_960 {
            // ⚠️ 检查伴奏调性全局原子，自动设置 AutoTune（翻唱模式也生效）
            let acc_root = crate::audio_engine::accompaniment::ACCOMPANIMENT_KEY_ROOT.load(Ordering::Relaxed);
            if acc_root >= 0 {
                let acc_major = crate::audio_engine::accompaniment::ACCOMPANIMENT_KEY_MAJOR.load(Ordering::Relaxed);
                auto_tune_engine.set_key(acc_root as u8, acc_major);
                // 消费掉，避免每帧都重复设置
                crate::audio_engine::accompaniment::ACCOMPANIMENT_KEY_ROOT.store(-1, Ordering::Relaxed);
            }

            let mut chunk = [0.0f32; CHUNK_960];
            for s in chunk.iter_mut() {
                *s = mic_buf.pop_front().unwrap_or(0.0);
            }

            // F0 检测与谱子解耦：只要 FCPE/RMVPE 会话存在就实时推理，
            // 不存在时 F0 为 0（VAD 会判定为静音/说话，修音链路直通而非崩溃）
            let mic_f0 = if let Some(ref mut fcpe) = fcpe_sess {
                fcpe_f0(&chunk, fcpe).first().copied().unwrap_or(0.0)
            } else if let Some(ref mut rmvpe) = rmvpe_sess {
                rmpe_f0(&chunk, 1, rmvpe).first().copied().unwrap_or(0.0)
            } else {
                0.0
            };

            let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
            let rms = (sum_sq / 960.0).sqrt();

            let state = vad.update(mic_f0, rms);
            let is_singing = state == VoiceState::Singing;

            let mut target_pitch_ratio = 1.0f32;

            if is_singing && mic_f0 > 50.0 {
                let target_f0 = auto_tune_engine.process(mic_f0, true);
                if target_f0 > 50.0 {
                    let raw_ratio = target_f0 / mic_f0;
                    let max_shift = 2.0f32.powf(4.0 / 12.0);
                    target_pitch_ratio = raw_ratio.clamp(1.0 / max_shift, max_shift);
                }
            }

            let retune_speed = 0.15;
            smoothed_pitch_ratio = smoothed_pitch_ratio * (1.0 - retune_speed)
                + target_pitch_ratio * retune_speed;

            // 定长契约：corrected.len() 恒等于 CHUNK_960
            let corrected = corrector.process(&chunk, smoothed_pitch_ratio);

            let output: &[f32] = if crate::audio_engine::MIC_MUTED.load(Ordering::Relaxed) {
                &silent_chunk
            } else {
                &corrected
            };

            // 水位保护仅作异常兜底：定长流水线下游正常消费时不应触发
            let prod_ok = vocal_prod.len() < MAX_OUT_WATERMARK
                && bh_vocal_prod
                    .as_ref()
                    .map(|p| p.len() < MAX_OUT_WATERMARK)
                    .unwrap_or(true);
            if prod_ok {
                let _ = push_ringbuf(vocal_prod, output);
                if let Some(ref mut p) = bh_vocal_prod {
                    let _ = push_ringbuf(p, output);
                }
            }

            last_metric_rms = rms;
            last_metric_f0 = mic_f0;
            last_metric_state = match state {
                VoiceState::Singing => "singing",
                VoiceState::Speaking => "speaking",
                VoiceState::Silent => "mute",
            };
            processed += CHUNK_960;
            if processed >= next_metric_at {
                next_metric_at = processed + 24000;
                let output_backlog_ms = vocal_prod.len() as f64 / 48.0;
                let input_backlog_ms = mic_buf.len() as f64 / 48.0;
                let pv_ms = PV_FFT_SIZE as f64 / 48.0;
                let total_ms = (output_backlog_ms + input_backlog_ms + pv_ms) as f32;
                let _ = app_handle.emit(
                    "live-metrics",
                    serde_json::json!({
                        "state": last_metric_state,
                        "rms": last_metric_rms,
                        "f0": last_metric_f0,
                        "f0_continuity": if is_singing {
                            1.0
                        } else if state == VoiceState::Speaking {
                            0.5
                        } else {
                            0.0
                        },
                        "hnr": 0,
                        "engine_load_ms": total_ms,
                        "detected_key": auto_tune_engine.detected_key().label(),
                    }),
                );
            }
        }
    }

    let tail = corrector.flush();
    if !tail.is_empty() {
        push_ringbuf(vocal_prod, &tail);
        if let Some(ref mut p) = bh_vocal_prod {
            push_ringbuf(p, &tail);
        }
    }
}

// ══════════════════════════════════════════════════════════════════
//  引擎入口
// ══════════════════════════════════════════════════════════════════

pub fn start_live_engine(
    config: LiveSingingConfig,
    app_handle: tauri::AppHandle,
    phase: Arc<RwLock<crate::state::LivePhase>>,
    stop_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    eprintln!("[VocalBoost] 入口");
    emit_log(&app_handle, "[引擎] 启动 VocalBoost 引擎...");
    emit_progress(&app_handle, 5, "正在查找音频设备...");

    let host = cpal::default_host();
    let route = blackhole::detect_route(&host);

    let (input_device_name, output_device_name) = if route.is_blackhole() {
        let bh_name = route.device_name.as_deref().unwrap_or("BlackHole 2ch");
        emit_log(
            &app_handle,
            &format!(
                "[路由] ✅ BlackHole 输出桥接：输入「{}」→ Voxril → 输出「{}」",
                config.input_device, bh_name
            ),
        );
        emit_log(&app_handle,
            "[路由] 耳机从物理设备监听混音（人声+伴奏），需关闭声卡 Loopback 防反馈",
        );
        (config.input_device.clone(), bh_name.to_string())
    } else {
        emit_log(
            &app_handle,
            &format!(
                "[路由] 直通模式：输入「{}」→ 输出「{}」",
                config.input_device, config.output_device
            ),
        );
        (config.input_device.clone(), config.output_device.clone())
    };

    let user_output_name = config.output_device.clone();

    let input_device = host
        .input_devices()
        .map_err(|e| e.to_string())?
        .find(|d| d.name().ok() == Some(input_device_name.clone()))
        .ok_or_else(|| format!("找不到输入设备: {}", input_device_name))?;
    let output_device = host
        .output_devices()
        .map_err(|e| e.to_string())?
        .find(|d| d.name().ok() == Some(output_device_name.clone()))
        .ok_or_else(|| format!("找不到输出设备: {}", output_device_name))?;

    // 列举可用输入设备
    {
        let all_inputs: Vec<String> = host
            .input_devices()
            .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();
        emit_log(
            &app_handle,
            &format!("[音频设备] 可用输入设备列表 ({} 个):", all_inputs.len()),
        );
        for (i, name) in all_inputs.iter().enumerate() {
            emit_log(&app_handle, &format!("[音频设备]   [{}] {}", i, name));
        }
    }
    {
        let in_name = input_device.name().unwrap_or_default();
        let loopback_keywords = ["blackhole", "loopback", "virtual", "bus", "总线", "null"];
        let is_virtual = loopback_keywords.iter().any(|k| in_name.to_lowercase().contains(k));
        if is_virtual && !route.is_blackhole() {
            emit_log(
                &app_handle,
                &format!("[音频设备] ⚠️ 检测到虚拟回路设备「{}」——如果不出声请更换物理设备", in_name),
            );
        }
    }

    emit_log(&app_handle, "[权限] 检查麦克风授权状态...");
    request_microphone_permission().map_err(|e| {
        let _ = app_handle.emit("live-singing-error", &e);
        e
    })?;
    emit_log(&app_handle, "[权限] ✅ 麦克风已授权，继续建流");

    let native_in = input_device
        .default_input_config()
        .map_err(|e| e.to_string())?;
    let native_out = output_device
        .default_output_config()
        .map_err(|e| e.to_string())?;

    let engine_sr = 48000u32;
    let in_sr = native_in.sample_rate().0;
    let out_sr = native_out.sample_rate().0;
    let in_channels = native_in.channels() as usize;
    let out_channels = native_out.channels() as usize;

    let input_config = cpal::StreamConfig {
        channels: cpal::ChannelCount::from(in_channels as u16),
        sample_rate: cpal::SampleRate(in_sr),
        buffer_size: cpal::BufferSize::Fixed(1024),
    };
    let output_config = cpal::StreamConfig {
        channels: cpal::ChannelCount::from(out_channels as u16),
        sample_rate: cpal::SampleRate(out_sr),
        buffer_size: cpal::BufferSize::Fixed(1024),
    };
    emit_log(
        &app_handle,
        &format!(
            "[音频] 输入: {}Hz {}ch | 输出: {}Hz {}ch | 引擎内部: {}Hz",
            in_sr, in_channels, out_sr, out_channels, engine_sr
        ),
    );

    // ── 缓冲区 ──
    let (mut mic_prod, mut mic_cons) =
        HeapRb::<f32>::new(config::INPUT_RING_CAPACITY).split();
    let (mut acc_prod, acc_cons) =
        HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
    let (mut cover_prod, cover_cons) =
        HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
    let (mut vocal_prod, vocal_cons) =
        HeapRb::<f32>::new(config::OUTPUT_RING_CAPACITY).split();

    let mut has_bh = false;
    let (mut bh_vocal_prod, bh_vocal_cons) = if route.is_blackhole() {
        has_bh = true;
        let (p, c) = HeapRb::<f32>::new(config::OUTPUT_RING_CAPACITY).split();
        (p, c)
    } else {
        let (p, c) = HeapRb::<f32>::new(1).split();
        (p, c)
    };
    let (bh_acc_prod, bh_acc_cons) = if has_bh {
        let (p, c) = HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
        (p, c)
    } else {
        let (p, c) = HeapRb::<f32>::new(1).split();
        (p, c)
    };
    let (bh_cover_prod, bh_cover_cons) = if has_bh {
        let (p, c) = HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
        (p, c)
    } else {
        let (p, c) = HeapRb::<f32>::new(1).split();
        (p, c)
    };

    let acc_prefill = vec![0.0f32; config::ACC_PREFILL_SAMPLES];
    let _ = push_ringbuf(&mut acc_prod, &acc_prefill);
    let _ = push_ringbuf(&mut cover_prod, &acc_prefill);

    let running = Arc::new(AtomicBool::new(true));

    // ── LRC 歌词：由前端在启动时显式传入，写到全局原子供歌词线程加载 ──
    if !config.lrc_path.is_empty() && Path::new(&config.lrc_path).exists() {
        emit_log(&app_handle, &format!("[歌词] 初始路径: {}", config.lrc_path));
        crate::audio_engine::accompaniment::set_lrc_path(config.lrc_path.clone());
    } else {
        emit_log(&app_handle, "[歌词] 本曲无歌词");
    }

    emit_progress(&app_handle, 15, "Phase 3 推理引擎");
    emit_log(&app_handle, "[Phase3] 启动: 修音引擎");

    let running_relay = running.clone();
    let app_relay = app_handle.clone();
    F0_UP_KEY.store(config.f0_up_key, Ordering::Relaxed);
    let config_cover = config.cover_path.clone();
    let phase_post = phase.clone();

    std::thread::spawn(move || {
        let bh_ref = if has_bh {
            Some(&mut bh_vocal_prod)
        } else {
            None
        };
        run_correction_relay(
            &mut vocal_prod,
            bh_ref,
            &mut mic_cons,
            config.clone(),
            running_relay.clone(),
            app_relay.clone(),
        );
    });

    // ── 伴奏播放线程 ──
    {
        let running_clone = running.clone();
        let app_acc = app_handle.clone();
        let mut bh_acc = if has_bh {
            Some(bh_acc_prod)
        } else {
            None
        };
        std::thread::spawn(move || {
            let mut current_audio: Vec<f32> = Vec::new();
            let mut pos: usize = 0;
            let mut has_loaded_once = false;
            let mut last_seek_ver: usize = 0;
            let mut last_song_ver: usize = accompaniment::SONG_VERSION.load(Ordering::SeqCst);
            emit_log(&app_acc, "[伴奏] 线程就绪，等待歌曲加载...");

            while running_clone.load(Ordering::SeqCst) {
                // ── 检测换歌/停止 (SONG_VERSION) ──
                let cur_song_ver = accompaniment::SONG_VERSION.load(Ordering::SeqCst);
                if cur_song_ver != last_song_ver {
                    last_song_ver = cur_song_ver;
                    // ⚠️ 拉取最新数据（停止时为空 Vec）
                    // 输出回调检测到 SONG_VERSION 变化后已 drain 所有 RingBuffer，
                    // producer 无需额外操作
                    if accompaniment::ACC_NEED_RELOAD.swap(false, Ordering::Relaxed) {
                        current_audio = accompaniment::get_audio();
                    }
                    pos = 0;
                    has_loaded_once = !current_audio.is_empty();
                    accompaniment::ACC_ENDED.store(false, Ordering::Relaxed);
                    emit_log(&app_acc, &format!("[伴奏] 版本变更：重置 pos=0, 时长={:.1}s", current_audio.len() as f64 / 48000.0));
                    continue;
                }

                // ── 检测 Seek ──
                let cur_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
                if cur_seek_ver != last_seek_ver {
                    last_seek_ver = cur_seek_ver;
                    let target_pos = accompaniment::ACC_SEEK_POSITION.load(Ordering::Relaxed);
                    if target_pos >= 0 {
                        pos = target_pos as usize;
                        accompaniment::ACC_PUSH_POS.store(pos, Ordering::Relaxed);
                        accompaniment::ACC_ENDED.store(false, Ordering::Relaxed);
                        emit_log(&app_acc, &format!("[伴奏] seek 到位置 {:.1}s", pos as f64 / 48000.0));
                    }
                }

                // ── 已有 ACC_NEED_RELOAD（SONG_VERSION 处理后未消耗的情况）──
                if accompaniment::ACC_NEED_RELOAD.swap(false, Ordering::Relaxed) {
                    let new_audio = accompaniment::get_audio();
                    if !new_audio.is_empty() || has_loaded_once {
                        emit_log(&app_acc, &format!("[伴奏] 切换: {:.1}s", new_audio.len() as f64 / 48000.0));
                    }
                    current_audio = new_audio;
                    pos = 0;
                    crate::audio_engine::accompaniment::ACC_PUSH_POS.store(0, Ordering::Relaxed);
                    accompaniment::ACC_ENDED.store(false, Ordering::Relaxed);
                    has_loaded_once = !current_audio.is_empty();
                }

                if !has_loaded_once {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }

                if pos < current_audio.len() {
                    let paused = accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
                    if paused {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    let end = (pos + 4800).min(current_audio.len());
                    let chunk = &current_audio[pos..end];
                    let written = push_ringbuf_with_latency_cap(&mut acc_prod, chunk, 48000);
                    if let Some(ref mut bhp) = bh_acc {
                        let _ = push_ringbuf_with_latency_cap(bhp, chunk, 48000);
                    }
                    pos += written;
                    // ⚠️ 更新推流位置（cover 线程 seek 时用到）
                    crate::audio_engine::accompaniment::ACC_PUSH_POS.store(pos, Ordering::Relaxed);
                    loop {
                        let level = acc_prod.len();
                        if level < 24000 || !running_clone.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                } else {
                    // 全部采样已推入环形缓冲：通知输出级在缓冲排空后判定播放结束
                    accompaniment::ACC_ENDED.store(true, Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
    }

    // ── 翻唱人声播放线程 ──
    let running_cov = running.clone();
    let app_cov = app_handle.clone();
    let mut bh_cov = if has_bh {
        Some(bh_cover_prod)
    } else {
        None
    };
    // ⚠️ 翻唱人声播放线程 — 始终和伴奏同步推流（由混音器门控 audible）
    std::thread::spawn(move || {
        let mut current_audio: Vec<f32> = Vec::new();
        let mut pos: usize = 0;
        let mut has_loaded_once = false;
        let mut last_seek_ver: usize = 0;

        if !config_cover.is_empty() && Path::new(&config_cover).exists() {
            match common_decode_cover(&config_cover, &app_cov) {
                Ok(audio) => {
                    accompaniment::load_cover(audio);
                    has_loaded_once = true;
                }
                Err(e) => emit_log(&app_cov, &format!("[翻唱] 预加载失败: {}", e)),
            }
        }

        if !has_loaded_once {
            emit_log(&app_cov, "[翻唱] 线程就绪，等待翻唱人声加载...");
        }

        while running_cov.load(Ordering::SeqCst) {
            // 检测 Seek
            let cur_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
            if cur_seek_ver != last_seek_ver {
                last_seek_ver = cur_seek_ver;
                let target_pos = accompaniment::ACC_SEEK_POSITION.load(Ordering::Relaxed);
                if target_pos >= 0 {
                    pos = target_pos as usize;
                    emit_log(&app_cov, &format!("[翻唱] seek 到位置 {:.1}s", pos as f64 / 48000.0));
                }
            }

            if accompaniment::COVER_NEED_RELOAD.swap(false, Ordering::Relaxed) {
                let new_audio = accompaniment::get_cover_audio();
                if !new_audio.is_empty() || has_loaded_once {
                    emit_log(
                        &app_cov,
                        &format!("[翻唱] 切换: {:.1}s", new_audio.len() as f64 / 48000.0),
                    );
                }
                let hush = vec![0.0f32; 240];
                let _ = push_ringbuf_with_latency_cap(&mut cover_prod, &hush, 48000);
                if let Some(ref mut bhp) = bh_cov {
                    let _ = push_ringbuf_with_latency_cap(bhp, &hush, 48000);
                }
                current_audio = new_audio;
                // ⚠️ seek 到伴奏线程的实时推流位置（而非 output callback 的消耗位置），
                // ACC_PUSH_POS 直接来自伴奏线程，与伴奏实际播放位置对齐
                let acc_pos = crate::audio_engine::accompaniment::ACC_PUSH_POS.load(Ordering::Relaxed);
                if acc_pos > 0 && !current_audio.is_empty() {
                    pos = acc_pos.min(current_audio.len() - 1);
                    emit_log(&app_cov, &format!("[翻唱] seek 到伴奏位置 {:.1}s",
                        pos as f64 / 48000.0));
                } else {
                    pos = 0;
                }
                has_loaded_once = true;
            }

            if !has_loaded_once {
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }

            // ⚠️ 始终推流（与伴奏同步），但暂停时停止推流
            // 混音器（mixer.rs）根据 COVER_PLAYING 决定是否 audible
            if pos < current_audio.len() {
                let acc_paused = crate::audio_engine::accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
                if acc_paused {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
                let end = (pos + 4800).min(current_audio.len());
                let chunk = &current_audio[pos..end];
                let written = push_ringbuf_with_latency_cap(&mut cover_prod, chunk, 48000);
                if let Some(ref mut bhp) = bh_cov {
                    let _ = push_ringbuf_with_latency_cap(bhp, chunk, 48000);
                }
                pos += written;
                // ⚠️ 水位等待策略（与伴奏一致）
                loop {
                    let level = cover_prod.len();
                    if level < 24000 || !running_cov.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            } else {
                if has_loaded_once && !current_audio.is_empty() {
                    emit_log(
                        &app_cov,
                        &format!("[翻唱] 播放完毕 ({:.1}s)", current_audio.len() as f64 / 48000.0),
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    });

    // ── 歌词推送线程（独立生命周期，不受引擎启停影响） ──
    spawn_lyrics_thread(&app_handle);

    // ── 输入流 ──
    let in_resampler = if in_sr != engine_sr {
        Some(StreamResampler::new(in_sr, engine_sr, 1024))
    } else {
        None
    };
    let input_stream = input_device
        .build_input_stream(
            &input_config,
            {
                // 预分配全部实时缓冲：回调内零分配
                let mut mono_scratch = vec![0.0f32; 4096];
                let mut rs_scratch = vec![
                    0.0f32;
                    in_resampler
                        .as_ref()
                        .map(|r| r.output_frames_max())
                        .unwrap_or(0)
                ];
                let mut in_resampler = in_resampler;
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let channels = in_channels;
                    let frames = data.len() / channels;
                    if frames == 0 {
                        return;
                    }

                    // 快速路径：1ch 且无需重采样
                    if channels == 1 && in_resampler.is_none() {
                        let _ = push_ringbuf(&mut mic_prod, data);
                        return;
                    }

                    // 多声道混单 / 采样率转换
                    if mono_scratch.len() < frames {
                        mono_scratch.resize(frames, 0.0);
                    }
                    for f in 0..frames {
                        let mut sum = 0.0f32;
                        for c in 0..channels {
                            sum += data[f * channels + c];
                        }
                        mono_scratch[f] = sum / channels as f32;
                    }

                    if let Some(ref mut r) = in_resampler {
                        r.feed(&mono_scratch[..frames]);
                        let n = r.drain_into(&mut rs_scratch);
                        let _ = push_ringbuf(&mut mic_prod, &rs_scratch[..n]);
                    } else {
                        let _ = push_ringbuf(&mut mic_prod, &mono_scratch[..frames]);
                    }
                }
            },
            |err| eprintln!("[输入流] {}", err),
            None,
        )
        .map_err(|e| e.to_string())?;

    // ── 输出流 ──
    // 输出级集中处理换歌/暂停同步、混音、重采样与唯一播放时钟，
    // 音频回调本身只保留一次 `fill`，便于审计实时安全性。
    let mut output_stage = OutputStage::new(
        acc_cons,
        vocal_cons,
        cover_cons,
        engine_sr,
        out_sr,
        out_channels,
    );
    let output_stream = output_device
        .build_output_stream(
            &output_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                output_stage.fill(data);
            },
            |err| eprintln!("[输出流] {}", err),
            None,
        )
        .map_err(|e| e.to_string())?;

    // ── CoreAudio 预加热 ──
    if route.is_blackhole() {
        emit_log(
            &app_handle,
            "[音频] 预加热 CoreAudio HAL（消除杂音冲击）...",
        );
        let warmup_cfg = cpal::StreamConfig {
            channels: in_channels as u16,
            sample_rate: cpal::SampleRate(in_sr),
            buffer_size: cpal::BufferSize::Fixed(1024),
        };
        if let Ok(warmup_stream) = input_device.build_input_stream(
            &warmup_cfg,
            |_: &[f32], _: &cpal::InputCallbackInfo| {},
            |_| {},
            None,
        ) {
            let _ = warmup_stream.play();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        emit_log(&app_handle, "[音频] 预加热完成，创建真实音频流...");
    }

    input_stream.play().map_err(|e| e.to_string())?;
    output_stream.play().map_err(|e| e.to_string())?;

    emit_log(&app_handle, "[音频] 流已就绪");
    emit_progress(&app_handle, 90, "音频流已就绪");

    // ── 物理设备监听镜像流 ──
    let monitor_stream = if route.is_blackhole() {
        let dev = host
            .output_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.name().ok() == Some(user_output_name.clone()))
            .ok_or_else(|| format!("找不到监听设备: {}", user_output_name))?;
        let mon_cfg = cpal::StreamConfig {
            channels: native_out.channels() as u16,
            sample_rate: cpal::SampleRate(out_sr),
            buffer_size: cpal::BufferSize::Fixed(1024),
        };
        emit_log(
            &app_handle,
            &format!(
                "[监听] 创建物理设备镜像流「{}」→ 耳机（确保关闭 Loopback 防反馈）",
                user_output_name
            ),
        );
        match dev.build_output_stream(
            &mon_cfg,
            {
                let mut bh_cons = bh_vocal_cons;
                let mut bh_acc_c = bh_acc_cons;
                let mut bh_cov_c = bh_cover_cons;
                let mut bh_vocal_smoother = StreamSmoother::new();
                let mut bh_cover_smoother = StreamSmoother::new();
                let ch = out_channels;
                let sr_cb = out_sr;
                // 预分配实时缓冲：回调内零分配（仅在设备给出超常规帧数时扩容）
                let mut bh_acc_buf = vec![0.0f32; MAX_STAGE_BLOCK];
                let mut bh_vocal_buf = vec![0.0f32; MAX_STAGE_BLOCK];
                let mut bh_cover_buf = vec![0.0f32; MAX_STAGE_BLOCK];
                let mut bh_discard = [0.0f32; 1024];
                let mut last_seek_ver: usize = usize::MAX;
                let mut last_song_ver: usize = usize::MAX;
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / ch.max(1);
                    let mono_frames = if sr_cb == 48000 {
                        frames
                    } else {
                        (frames as f64 * 48000.0 / sr_cb as f64).ceil() as usize
                    };
                    if bh_acc_buf.len() < mono_frames {
                        bh_acc_buf.resize(mono_frames, 0.0);
                        bh_vocal_buf.resize(mono_frames, 0.0);
                        bh_cover_buf.resize(mono_frames, 0.0);
                    }

                    // ⚠️ SONG_VERSION 检测（换歌/停止时 drain）
                    let cur_song_ver = accompaniment::SONG_VERSION.load(Ordering::Relaxed);
                    if cur_song_ver != last_song_ver {
                        last_song_ver = cur_song_ver;
                        while bh_acc_c.pop_slice(&mut bh_discard) > 0 {}
                        while bh_cons.pop_slice(&mut bh_discard) > 0 {}
                        while bh_cov_c.pop_slice(&mut bh_discard) > 0 {}
                        last_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
                    }

                    // 检测 Seek，清空 RingBuffer，消除延迟
                    let cur_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
                    if cur_seek_ver != last_seek_ver {
                        last_seek_ver = cur_seek_ver;
                        while bh_acc_c.pop_slice(&mut bh_discard) > 0 {}
                        while bh_cons.pop_slice(&mut bh_discard) > 0 {}
                        while bh_cov_c.pop_slice(&mut bh_discard) > 0 {}
                    }
                    let acc_n = bh_acc_c.pop_slice(&mut bh_acc_buf[..mono_frames]);
                    let vocal_n = bh_cons.pop_slice(&mut bh_vocal_buf[..mono_frames]);
                    let cover_n = bh_cov_c.pop_slice(&mut bh_cover_buf[..mono_frames]);
                    let acc_paused = accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
                    // 音源模式在回调入口读取一次，避免逐采样读原子
                    let cover_enabled =
                        crate::audio_engine::AUDIO_SOURCE.load(Ordering::Relaxed) == 1;

                    mixer::mix_bh_monitor(
                        frames,
                        sr_cb,
                        &bh_acc_buf,
                        acc_n,
                        acc_paused,
                        &bh_vocal_buf,
                        vocal_n,
                        &bh_cover_buf,
                        cover_n,
                        cover_enabled,
                        &mut bh_vocal_smoother,
                        &mut bh_cover_smoother,
                        data,
                        ch,
                    );
                }
            },
            |err| eprintln!("[监听流] {}", err),
            None,
        ) {
            Ok(s) => {
                let _ = s.play();
                Some(s)
            }
            Err(e) => {
                emit_log(&app_handle, &format!("[监听] ⚠️ 创建失败: {}", e));
                None
            }
        }
    } else {
        None
    };

    let _keep = (input_stream, output_stream, monitor_stream);

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        *phase_post.write().await = crate::state::LivePhase::Running;
    });
    emit_progress(&app_handle, 100, "引擎已就绪");
    let _ = app_handle.emit("live-singing-ready", ());

    let _ = rt.block_on(async { stop_rx.await.ok() });

    running.store(false, Ordering::SeqCst);

    rt.block_on(async {
        *phase_post.write().await = crate::state::LivePhase::Idle;
    });
    emit_log(&app_handle, "[引擎] 已停止");
    Ok(())
}

/// 启动歌词推送线程（独立生命周期，不受引擎启停影响）
/// 只会启动一次（由 LYRICS_THREAD_SPAWNED 保证）
pub fn spawn_lyrics_thread(app_handle: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    use crate::audio_engine::accompaniment;

    if accompaniment::LYRICS_THREAD_SPAWNED.swap(true, Ordering::SeqCst) {
        return; // 已启动，不重复创建
    }

    let app = app_handle.clone();
    std::thread::spawn(move || {
        use crate::audio_engine::lrc::LrcData;
        let mut current_lrc: Option<LrcData> = None;
        let mut last_lrc_version: usize = 0;
        let mut prev_index: isize = -3;
        let mut tick: u32 = 0;

        let mut last_status = "stopped";
        let mut last_source = 99;
        let mut last_loaded = false;
        let mut last_song_ver: usize = 0;

        // 启动时从 LRC_PATH 加载已有歌词
        let init_path = accompaniment::get_lrc_path();
        if !init_path.is_empty() {
            match std::fs::read_to_string(&init_path) {
                Ok(content) => {
                    let parsed = LrcData::parse(&content);
                    if !parsed.is_empty() {
                        let n = parsed.len();
                        current_lrc = Some(parsed);
                        emit_log(&app, &format!("[歌词] 加载: {} 行", n));
                        // 通知前端预览
                        let _ = app.emit("lyrics-preview", serde_json::json!({
                            "lines": current_lrc.as_ref().map(|l| {
                                l.lines.iter().map(|ln| serde_json::json!({
                                    "time_secs": ln.time_secs,
                                    "text": ln.text,
                                })).collect::<Vec<_>>()
                            }).unwrap_or_default(),
                        }));
                    }
                }
                Err(e) => emit_log(&app, &format!("[歌词] 初始加载失败: {}", e)),
            }
            last_lrc_version = accompaniment::LRC_VERSION.load(Ordering::Relaxed);
        }

        // ⚠️ 使用 LYRICS_RUNNING 控制生命周期，而非引擎的 running 标志
        while accompaniment::LYRICS_RUNNING.load(Ordering::SeqCst) {
            tick += 1;

            // ⚠️ SONG_VERSION 检测（换歌/停止），强制下一帧发送完整状态
            let cur_song_ver = accompaniment::SONG_VERSION.load(Ordering::Relaxed);
            if cur_song_ver != last_song_ver {
                last_song_ver = cur_song_ver;
                prev_index = -3;
                last_status = "force-send";
            }

            let cur_version = accompaniment::LRC_VERSION.load(Ordering::Relaxed);
            if cur_version != last_lrc_version {
                last_lrc_version = cur_version;
                let cur_path = accompaniment::get_lrc_path();
                current_lrc = None;
                prev_index = -3;

                if !cur_path.is_empty() {
                    match std::fs::read_to_string(&cur_path) {
                        Ok(content) => {
                            let parsed = LrcData::parse(&content);
                            if !parsed.is_empty() {
                                let n = parsed.len();
                                current_lrc = Some(parsed);
                                emit_log(&app, &format!("[歌词] 加载: {} 行", n));
                            }
                        }
                        Err(e) => emit_log(&app, &format!("[歌词] 读取失败: {}", e)),
                    }
                } else {
                    emit_log(&app, "[歌词] 已清除");
                }
            }

            // 从引擎全局读取当前状态
            let pb = accompaniment::PLAYBACK_COUNTER.load(Ordering::Relaxed);
            let acc_paused = accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
            let source = crate::audio_engine::AUDIO_SOURCE.load(Ordering::Relaxed);
            let has_audio = accompaniment::has_audio();

            let status = if !has_audio {
                "stopped"
            } else if acc_paused {
                "paused"
            } else {
                "playing"
            };
            let loaded = has_audio;

            let (current_text, total_secs, current_idx) = if let Some(ref data) = current_lrc {
                let pos_secs = pb as f64 / 48000.0;
                let (ci, _ni) = data.find_line_at(pos_secs);
                let text = if ci >= 0 { data.lines.get(ci as usize).map(|l| l.text.clone()).unwrap_or_default() } else { String::new() };
                let dur = data.total_duration_secs();
                (text, dur, ci)
            } else {
                (String::new(), 0.0, -1)
            };

            let progress_secs = pb as f64 / 48000.0;

            let status_changed = status != last_status;
            let source_changed = source != last_source;
            let loaded_changed = loaded != last_loaded;
            let lrc_line_changed = current_idx != prev_index;
            let play_tick = status == "playing" && tick % 2 == 0;

            let should_emit = status_changed || source_changed || loaded_changed || lrc_line_changed || play_tick;

            if should_emit {
                prev_index = current_idx;
                last_status = status;
                last_source = source;
                last_loaded = loaded;

                let _ = app.emit("player-state", serde_json::json!({
                    "status": status,
                    "loaded": loaded,
                    "audio_source": source,
                    "progress_secs": progress_secs,
                    "current_line": current_text,
                    "current_line_index": current_idx,
                    "total_lines": current_lrc.as_ref().map_or(0, |d| d.len()) as u32,
                    "total_duration_secs": total_secs,
                    "preview_lines": current_lrc.as_ref().map(|l| {
                        l.lines.iter().map(|ln| serde_json::json!({
                            "time_secs": ln.time_secs,
                            "text": ln.text,
                        })).collect::<Vec<_>>()
                    }).unwrap_or_default(),
                    "instrumental_path": accompaniment::get_current_instrumental_path(),
                    "lrc_path": accompaniment::get_current_lrc_path(),
                    "cover_path": accompaniment::get_current_cover_path(),
                }));
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}
