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
use crate::audio_engine::pitch_correction::{PitchCorrector, PV_FFT_SIZE};
use crate::audio_engine::resampler::StreamResampler;
use crate::audio_engine::rvc::{build_session, fcpe_f0, rmpe_f0};
use crate::audio_engine::smoother::StreamSmoother;
use crate::audio_engine::thread::boost_audio_thread_priority;
use crate::audio_engine::voice_state::{VoiceState, VoiceStateDetector};
use crate::audio_engine::{
    blackhole, common_decode_cover, emit_log, emit_progress, push_ringbuf,
    push_ringbuf_with_latency_cap, LiveSingingConfig, F0_UP_KEY, SILENT_CHUNK,
};
use crate::audio_engine::{accompaniment, mixer};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{HeapConsumer, HeapProducer, HeapRb};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

fn fallback_f0_score() -> Vec<f32> {
    Vec::new()
}

// ══════════════════════════════════════════════════════════════════
//  修音中继线程
// ══════════════════════════════════════════════════════════════════

fn run_correction_relay(
    vocal_prod: &mut HeapProducer<f32>,
    mut bh_vocal_prod: Option<&mut HeapProducer<f32>>,
    mic_cons: &mut HeapConsumer<f32>,
    config: LiveSingingConfig,
    f0_score: Vec<f32>,
    running: Arc<AtomicBool>,
    app_handle: tauri::AppHandle,
    active_flag: Arc<RwLock<bool>>,
    playback_counter: Arc<AtomicUsize>,
    data_ready: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
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

    let has_score = !f0_score.is_empty();
    let score_frames = f0_score.len();
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
        "[修音] 就绪 {} 谱子帧, FCPE={}, RMVPE={}",
        score_frames,
        fcpe_sess.is_some(),
        rmvpe_sess.is_some()
    );
    const CHUNK_960: usize = 960;

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
            let (lock, cvar) = &*data_ready;
            let mut ready = lock.lock().unwrap();
            while !*ready {
                let (g, _) = cvar
                    .wait_timeout(ready, std::time::Duration::from_millis(5))
                    .unwrap();
                ready = g;
                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }
            *ready = false;
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

            let mic_f0 = if has_score {
                if let Some(ref mut fcpe) = fcpe_sess {
                    let f0_vec = fcpe_f0(&chunk, fcpe);
                    f0_vec.first().copied().unwrap_or(0.0)
                } else if let Some(ref mut rmvpe) = rmvpe_sess {
                    let f0_vec = rmpe_f0(&chunk, 1, rmvpe);
                    f0_vec.first().copied().unwrap_or(0.0)
                } else {
                    0.0
                }
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

            let corrected = corrector.process(&chunk, smoothed_pitch_ratio);

            let output = if crate::audio_engine::MIC_MUTED.load(Ordering::Relaxed) {
                &SILENT_CHUNK[..corrected.len()]
            } else {
                &corrected
            };

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
    active_flag: Arc<RwLock<bool>>,
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
                "[路由] ✅ BlackHole 输出桥接：输入「{}」→ VoiceCraft → 输出「{}」",
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
    let (mut acc_prod, mut acc_cons) =
        HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
    let (mut cover_prod, mut cover_cons) =
        HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
    let (mut vocal_prod, mut vocal_cons) =
        HeapRb::<f32>::new(config::OUTPUT_RING_CAPACITY).split();

    let mut has_bh = false;
    let (mut bh_vocal_prod, mut bh_vocal_cons) = if route.is_blackhole() {
        has_bh = true;
        let (p, c) = HeapRb::<f32>::new(config::OUTPUT_RING_CAPACITY).split();
        (p, c)
    } else {
        let (p, c) = HeapRb::<f32>::new(1).split();
        (p, c)
    };
    let (mut bh_acc_prod, mut bh_acc_cons) = if has_bh {
        let (p, c) = HeapRb::<f32>::new(config::ACC_RING_CAPACITY).split();
        (p, c)
    } else {
        let (p, c) = HeapRb::<f32>::new(1).split();
        (p, c)
    };
    let (mut bh_cover_prod, mut bh_cover_cons) = if has_bh {
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
    let data_ready: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)> =
        Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let playback_counter = Arc::new(AtomicUsize::new(0));
    let acc_drop_samples = Arc::new(AtomicUsize::new(0));
    let cover_drop_samples = Arc::new(AtomicUsize::new(0));
    let mic_drop_samples = Arc::new(AtomicUsize::new(0));
    let vocal_drop_samples = Arc::new(AtomicUsize::new(0));
    let vocal_drop_chunks = Arc::new(AtomicUsize::new(0));
    let vocal_underrun_samples = Arc::new(AtomicUsize::new(0));
    let vocal_underrun_callbacks = Arc::new(AtomicUsize::new(0));
    let vocal_buf_level = Arc::new(AtomicUsize::new(0));

    // ── F0 谱子 ──
    let f0_score_hz: Vec<f32> = {
        let f0_json_path = if !config.f0_score_path.is_empty() {
            std::path::PathBuf::from(&config.f0_score_path)
        } else if !config.accompaniment_path.is_empty() {
            std::path::PathBuf::from(&config.accompaniment_path)
                .with_extension("f0.json")
        } else {
            std::path::PathBuf::new()
        };
        if f0_json_path.exists() {
            match std::fs::read_to_string(&f0_json_path) {
                Ok(content) => {
                    match serde_json::from_str::<crate::audio_engine::types::F0Profile>(
                        &content,
                    ) {
                        Ok(profile) => {
                            emit_log(
                                &app_handle,
                                &format!(
                                    "[F0] ✅ 加载真实谱子: {} 帧, {:.1}s",
                                    profile.f0_curve.len(),
                                    profile.duration_secs()
                                ),
                            );
                            if profile.hop_length == 480 {
                                profile.f0_curve
                            } else {
                                let ratio =
                                    profile.sample_rate as f64 / profile.hop_length as f64;
                                let target_frames =
                                    (profile.f0_curve.len() as f64 * 100.0 / ratio) as usize;
                                let mut resampled = Vec::with_capacity(target_frames);
                                for i in 0..target_frames {
                                    let idx = (i as f64 / target_frames as f64
                                        * profile.f0_curve.len() as f64)
                                        as usize;
                                    resampled.push(profile.f0_curve.get(idx).copied().unwrap_or(0.0));
                                }
                                resampled
                            }
                        }
                        Err(e) => {
                            emit_log(&app_handle, &format!("[F0] ⚠️ 解析失败: {}", e));
                            fallback_f0_score()
                        }
                    }
                }
                Err(e) => {
                    emit_log(&app_handle, &format!("[F0] ⚠️ 读取失败: {}", e));
                    fallback_f0_score()
                }
            }
        } else if !config.accompaniment_path.is_empty() {
            emit_log(&app_handle, "[F0] 未找到 .f0.json，使用实时 YIN");
            fallback_f0_score()
        } else {
            fallback_f0_score()
        }
    };

    // ── LRC 歌词：检测初始歌词路径，写到全局原子供歌词线程加载 ──
    {
        emit_log(
            &app_handle,
            &format!(
                "[歌词] 初始检测: f0_score_path='{}', acc_path='{}'",
                config.f0_score_path, config.accompaniment_path
            ),
        );

        let lrc_path = if !config.f0_score_path.is_empty() {
            let p = std::path::Path::new(&config.f0_score_path);
            let fname = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let base = fname.strip_suffix(".f0").unwrap_or(fname);
            let dir = p.parent().unwrap_or(std::path::Path::new(""));
            let lrc_candidate = dir.join(format!("{}.lrc", base));
            if lrc_candidate.exists() { Some(lrc_candidate.to_string_lossy().to_string()) } else { None }
        } else if !config.accompaniment_path.is_empty() {
            let fname = std::path::Path::new(&config.accompaniment_path)
                .file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let vocal_name = fname
                .replacen("instrumental_", "vocal_", 1)
                .replacen("instrument_", "vocal_", 1)
                .replacen("instrument", "vocal", 1);
            let parent = std::path::Path::new(&config.accompaniment_path)
                .parent().unwrap_or(std::path::Path::new(""));
            let parent_str = parent.to_string_lossy().to_string();
            let vocal_dir = parent_str
                .replacen("instrumental", "vocals", 1)
                .replacen("instrument", "vocals", 1);
            let p = std::path::Path::new(&vocal_dir).join(format!("{}.lrc", vocal_name));
            if p.exists() { Some(p.to_string_lossy().to_string()) } else { None }
        } else { None };

        if let Some(ref path) = lrc_path {
            emit_log(&app_handle, &format!("[歌词] 初始路径: {}", path));
            crate::audio_engine::accompaniment::set_lrc_path(path.clone());
        } else {
            emit_log(&app_handle, "[歌词] 未找到 LRC 文件");
        }
    }

    emit_progress(&app_handle, 15, "Phase 3 推理引擎");
    emit_log(&app_handle, "[Phase3] 启动: 修音引擎");

    let running_relay = running.clone();
    let app_relay = app_handle.clone();
    let playback_counter_relay = playback_counter.clone();
    F0_UP_KEY.store(config.f0_up_key, Ordering::Relaxed);
    let config_cover = config.cover_path.clone();
    let active_flag_post = active_flag.clone();
    let f0_score_for_correction = f0_score_hz.clone();
    let playback_counter_for_correction = playback_counter.clone();
    let data_ready_relay = data_ready.clone();

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
            f0_score_for_correction,
            running_relay.clone(),
            app_relay.clone(),
            active_flag.clone(),
            playback_counter_for_correction,
            data_ready,
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
                    if written < chunk.len() {
                        acc_drop_samples.fetch_add(chunk.len() - written, Ordering::Relaxed);
                    }
                    pos += written;
                    // ⚠️ 更新推流位置（cover 线程 seek 时用到）
                    crate::audio_engine::accompaniment::ACC_PUSH_POS.store(pos, Ordering::Relaxed);
                    loop {
                        let level = acc_prod.len();
                        if level < 24000 { break; }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                } else {
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
                if written < chunk.len() {
                    cover_drop_samples.fetch_add(chunk.len() - written, Ordering::Relaxed);
                }
                pos += written;
                // ⚠️ 水位等待策略（与伴奏一致）
                loop {
                    let level = cover_prod.len();
                    if level < 24000 {
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

    // ── 歌词推送线程 ──
    {
        let running_lrc = running.clone();
        let app_lrc = app_handle.clone();
        let mut prev_index: isize = -3;
        std::thread::spawn(move || {
            use crate::audio_engine::lrc::LrcData;
            let mut current_lrc: Option<LrcData> = None;
            let mut last_lrc_path: String = String::new();
            let mut last_lrc_version: usize = 0;
            let mut tick: u32 = 0;

            let mut last_status = "stopped";
            let mut last_source = 99;
            let mut last_loaded = false;
            let mut last_song_ver: usize = 0;

            // 启动时从 LRC_PATH 加载已有歌词
            let init_path = crate::audio_engine::accompaniment::get_lrc_path();
            if !init_path.is_empty() {
                match std::fs::read_to_string(&init_path) {
                    Ok(content) => {
                        let parsed = LrcData::parse(&content);
                        if !parsed.is_empty() {
                            let n = parsed.len();
                            current_lrc = Some(parsed);
                            emit_log(&app_lrc, &format!("[歌词] 加载: {} 行", n));
                            // 通知前端预览
                            let _ = app_lrc.emit("lyrics-preview", serde_json::json!({
                                "lines": current_lrc.as_ref().map(|l| {
                                    l.lines.iter().map(|ln| serde_json::json!({
                                        "time_secs": ln.time_secs,
                                        "text": ln.text,
                                    })).collect::<Vec<_>>()
                                }).unwrap_or_default(),
                            }));
                        }
                    }
                    Err(e) => emit_log(&app_lrc, &format!("[歌词] 初始加载失败: {}", e)),
                }
                last_lrc_path = init_path;
                last_lrc_version = crate::audio_engine::accompaniment::LRC_VERSION.load(Ordering::Relaxed);
            }

            while running_lrc.load(Ordering::SeqCst) {
                tick += 1;

                // ⚠️ SONG_VERSION 检测（换歌/停止），强制下一帧发送完整状态
                let cur_song_ver = crate::audio_engine::accompaniment::SONG_VERSION.load(Ordering::Relaxed);
                if cur_song_ver != last_song_ver {
                    last_song_ver = cur_song_ver;
                    prev_index = -3;
                    last_status = "force-send"; // 确保 should_emit=true
                }

                let cur_version = crate::audio_engine::accompaniment::LRC_VERSION.load(Ordering::Relaxed);
                if cur_version != last_lrc_version {
                    last_lrc_version = cur_version;
                    let cur_path = crate::audio_engine::accompaniment::get_lrc_path();
                    last_lrc_path = cur_path.clone();
                    current_lrc = None;
                    prev_index = -3; // ⚠️ 强制下一 tick 发送 player-state

                    if !cur_path.is_empty() {
                        match std::fs::read_to_string(&cur_path) {
                            Ok(content) => {
                                let parsed = LrcData::parse(&content);
                                if !parsed.is_empty() {
                                    let n = parsed.len();
                                    current_lrc = Some(parsed);
                                    emit_log(&app_lrc, &format!("[歌词] 加载: {} 行", n));
                                }
                            }
                            Err(e) => emit_log(&app_lrc, &format!("[歌词] 读取失败: {}", e)),
                        }
                    } else {
                        emit_log(&app_lrc, "[歌词] 已清除");
                    }
                }

                // ⚠️ 从引擎全局读取当前状态，组成 player-state 事件
                let pb = crate::audio_engine::accompaniment::PLAYBACK_COUNTER.load(Ordering::Relaxed);
                let acc_paused = crate::audio_engine::accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
                let source = crate::audio_engine::AUDIO_SOURCE.load(Ordering::Relaxed);
                let has_audio = crate::audio_engine::accompaniment::has_audio();

                let status = if !has_audio {
                    "stopped"
                } else if acc_paused {
                    "paused"
                } else {
                    "playing"
                };
                let loaded = has_audio;

                // 计算歌词位置
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

                // 是否应当发送事件：状态、音源、或加载情况改变，或者歌词行数改变，或者播放中每200ms自动刷新进度
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

                    let _ = app_lrc.emit("player-state", serde_json::json!({
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
                    }));
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }

    // ── 输入流 ──
    let mut in_resampler = if in_sr != engine_sr {
        let chunk_size = 1024;
        Some(StreamResampler::new(in_sr, engine_sr, chunk_size))
    } else {
        None
    };
    let input_stream = input_device
        .build_input_stream(
            &input_config,
            {
                let data_ready_input = data_ready_relay;
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let channels = in_channels;
                    let frames = data.len() / channels;

                    // 快速路径：1ch 48kHz 无重采样
                    if channels == 1 && in_resampler.is_none() {
                        let _ = push_ringbuf(&mut mic_prod, data);
                    } else {
                        // 混音到 mono
                        let mut mono = Vec::with_capacity(frames);
                        for f in 0..frames {
                            let sum: f32 = data[f * channels..(f + 1) * channels]
                                .iter()
                                .sum();
                            mono.push(sum / channels as f32);
                        }
                        if let Some(ref mut r) = in_resampler {
                            let resampled = r.push(&mono);
                            let _ = push_ringbuf(&mut mic_prod, &resampled);
                        } else {
                            let _ = push_ringbuf(&mut mic_prod, &mono);
                        }
                    }

                    // Condvar 唤醒修音线程
                    let (lock, cvar) = &*data_ready_input;
                    let mut ready = lock.lock().unwrap();
                    *ready = true;
                    cvar.notify_one();
                }
            },
            |err| eprintln!("[输入流] {}", err),
            None,
        )
        .map_err(|e| e.to_string())?;

    // ── 输出流 ──
    let playback_counter_out = playback_counter.clone();
    let vocal_underrun_samples_out = vocal_underrun_samples.clone();
    let vocal_underrun_callbacks_out = vocal_underrun_callbacks.clone();
    let vocal_buf_level_out = vocal_buf_level.clone();
    let mut output_resampler = if out_sr != engine_sr {
        let chunk_size = 512;
        Some(StreamResampler::new(engine_sr, out_sr, chunk_size))
    } else {
        None
    };
    let output_stream = output_device
        .build_output_stream(
            &output_config,
            {
                let mut vocal_smoother = StreamSmoother::new();
                let mut cover_smoother = StreamSmoother::new();
                let mut acc_smoother = StreamSmoother::new();
                let out_ch_cb = out_channels;
                let mut mono_buf: Vec<f32> = Vec::new();
                let mut out_resampler = output_resampler;
                let mut vocal_buf_local: Vec<f32> = Vec::new();
                let mut cover_buf_local: Vec<f32> = Vec::new();
                let mut mixed: Vec<f32> = Vec::new();
                let mut last_seek_ver: usize = 0;
                let mut last_song_ver: usize = 0;
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / out_ch_cb.max(1);
                    let mono_frames = if engine_sr == out_sr {
                        frames
                    } else {
                        (frames as f64 * engine_sr as f64 / out_sr as f64).ceil() as usize
                    };

                    if mono_buf.len() < mono_frames {
                        mono_buf.resize(mono_frames, 0.0);
                    }

                    // ⚠️ 检测 SONG_VERSION（换歌/停止），清空所有 RingBuffer + 重置计数器
                    let cur_song_ver = crate::audio_engine::accompaniment::SONG_VERSION.load(Ordering::Relaxed);
                    if cur_song_ver != last_song_ver {
                        last_song_ver = cur_song_ver;
                        playback_counter_out.store(0, Ordering::SeqCst);
                        let mut discard = [0.0f32; 1024];
                        while acc_cons.pop_slice(&mut discard) > 0 {}
                        while vocal_cons.pop_slice(&mut discard) > 0 {}
                        while cover_cons.pop_slice(&mut discard) > 0 {}
                    }

                    // 检测 Seek，清空 RingBuffer，消除延迟
                    let cur_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
                    if cur_seek_ver != last_seek_ver {
                        last_seek_ver = cur_seek_ver;
                        let target_pos = accompaniment::ACC_SEEK_POSITION.load(Ordering::Relaxed);
                        if target_pos >= 0 {
                            // 重置本地 Arc 计数器，确保和全局同步
                            playback_counter_out.store(target_pos as usize, Ordering::SeqCst);
                        }
                        let mut discard = [0.0f32; 1024];
                        while acc_cons.pop_slice(&mut discard) > 0 {}
                        while cover_cons.pop_slice(&mut discard) > 0 {}
                    }
                    if vocal_buf_local.len() < mono_frames {
                        vocal_buf_local.resize(mono_frames, 0.0);
                    }
                    if cover_buf_local.len() < mono_frames {
                        cover_buf_local.resize(mono_frames, 0.0);
                    }

                    let acc_read = acc_cons.pop_slice(&mut mono_buf[..mono_frames]);
                    let vocal_read = vocal_cons.pop_slice(&mut vocal_buf_local[..mono_frames]);
                    let cover_read = cover_cons.pop_slice(&mut cover_buf_local[..mono_frames]);
                    vocal_buf_level_out.store(vocal_cons.len(), Ordering::Relaxed);

                    let is_underrun = vocal_read < mono_frames;
                    if is_underrun {
                        vocal_underrun_callbacks_out.fetch_add(1, Ordering::Relaxed);
                        vocal_underrun_samples_out
                            .fetch_add(mono_frames - vocal_read, Ordering::Relaxed);
                    }

                    let acc_paused = accompaniment::ACC_PAUSED.load(Ordering::Relaxed);

                    // 混音
                    let _ = mixer::mix_audio_tracks(
                        mono_frames,
                        &mono_buf,
                        acc_read,
                        acc_paused,
                        &vocal_buf_local,
                        vocal_read,
                        &cover_buf_local,
                        cover_read,
                        &mut acc_smoother,
                        &mut vocal_smoother,
                        &mut cover_smoother,
                        &mut mixed,
                    );

                    // ⚠️ 播放时钟：只在实际播放时前进，暂停时冻结
                    // 用 acc_paused 门控而非 acc_read（acc_read 在暂停时可能为 0
                    // 但输出回调仍然会触发，防止歌词与音频不同步）
                    if !acc_paused {
                        let new_pos = playback_counter_out.fetch_add(mono_frames, Ordering::SeqCst) + mono_frames;
                        // 同步写入全局，供歌词线程和 seek 命令读取
                        crate::audio_engine::accompaniment::PLAYBACK_COUNTER.store(new_pos, Ordering::SeqCst);
                    }

                    // 重采样 → 输出
                    if let Some(ref mut r) = out_resampler {
                        let resampled = r.push(&mixed);
                        for f in 0..frames {
                            let sample = *resampled.get(f).unwrap_or(&0.0);
                            for c in 0..out_ch_cb {
                                data[f * out_ch_cb + c] = sample;
                            }
                        }
                    } else {
                        for f in 0..frames {
                            let sample = if f < mixed.len() { mixed[f] } else { 0.0 };
                            for c in 0..out_ch_cb {
                                data[f * out_ch_cb + c] = sample;
                            }
                        }
                    }
                }
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
                let mut bh_acc_buf: Vec<f32> = Vec::new();
                let mut bh_vocal_buf: Vec<f32> = Vec::new();
                let mut bh_cover_buf: Vec<f32> = Vec::new();
                let mut last_seek_ver: usize = 0;
                let mut last_song_ver: usize = 0;
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / ch.max(1);
                    let mono_frames = if sr_cb == 48000 {
                        frames
                    } else {
                        (frames as f64 * 48000.0 / sr_cb as f64).ceil() as usize
                    };
                    if bh_acc_buf.len() < mono_frames {
                        bh_acc_buf.resize(mono_frames, 0.0);
                    }

                    // ⚠️ SONG_VERSION 检测（换歌/停止时 drain）
                    let cur_song_ver = crate::audio_engine::accompaniment::SONG_VERSION.load(Ordering::Relaxed);
                    if cur_song_ver != last_song_ver {
                        last_song_ver = cur_song_ver;
                        let mut discard = [0.0f32; 1024];
                        while bh_acc_c.pop_slice(&mut discard) > 0 {}
                        while bh_cons.pop_slice(&mut discard) > 0 {}
                        while bh_cov_c.pop_slice(&mut discard) > 0 {}
                    }

                    // 检测 Seek，清空 RingBuffer，消除延迟
                    let cur_seek_ver = accompaniment::ACC_SEEK_VERSION.load(Ordering::Relaxed);
                    if cur_seek_ver != last_seek_ver {
                        last_seek_ver = cur_seek_ver;
                        let mut discard = [0.0f32; 1024];
                        while bh_acc_c.pop_slice(&mut discard) > 0 {}
                        while bh_cons.pop_slice(&mut discard) > 0 {}
                        while bh_cov_c.pop_slice(&mut discard) > 0 {}
                    }
                    if bh_vocal_buf.len() < mono_frames {
                        bh_vocal_buf.resize(mono_frames, 0.0);
                    }
                    if bh_cover_buf.len() < mono_frames {
                        bh_cover_buf.resize(mono_frames, 0.0);
                    }
                    let acc_n = bh_acc_c.pop_slice(&mut bh_acc_buf[..mono_frames]);
                    let vocal_n = bh_cons.pop_slice(&mut bh_vocal_buf[..mono_frames]);
                    let cover_n = bh_cov_c.pop_slice(&mut bh_cover_buf[..mono_frames]);
                    let acc_paused = accompaniment::ACC_PAUSED.load(Ordering::Relaxed);

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
        *active_flag_post.write().await = true;
    });
    emit_progress(&app_handle, 100, "引擎已就绪");
    let _ = app_handle.emit("live-singing-ready", ());

    let _ = rt.block_on(async { stop_rx.await.ok() });

    running.store(false, Ordering::SeqCst);

    rt.block_on(async {
        *active_flag_post.write().await = false;
    });
    emit_log(&app_handle, "[引擎] 已停止");
    Ok(())
}
