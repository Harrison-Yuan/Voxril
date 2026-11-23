//! 全局伴奏播放控制
//!
//! 直播引擎运行时，伴奏由一个独立的线程从 ACC_AUDIO_GLOBAL 读取并推送到 ring buffer。
//! 此模块提供动态加载/切换伴奏音频的能力。
//! 同时管理全局 LRC 歌词数据，使引擎能在运行时动态加载歌词。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 歌曲版本号 — 每次换歌或停止时递增
/// 所有引擎线程（伴奏、翻唱、歌词、输出回调）监听此值，
/// 检测到变化立即执行 reset：pos=0、drain RingBuffer、清空歌词计数
pub static SONG_VERSION: AtomicUsize = AtomicUsize::new(0);

/// 歌词线程独立生命周期标志 — true 时线程持续运行，不受引擎启停影响
pub static LYRICS_RUNNING: AtomicBool = AtomicBool::new(true);
/// 歌词线程是否已创建（只创建一次，避免重复 spawn）
pub static LYRICS_THREAD_SPAWNED: AtomicBool = AtomicBool::new(false);

/// 全局歌曲播放时钟（48kHz 采样数），由引擎输出回调累加，换歌时重置
pub static PLAYBACK_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 伴奏线程当前推流位置（48kHz 采样数），由伴奏线程写入，cover 线程 seek 时读取
/// PLAYBACK_COUNTER 是输出回调消耗后的位置（滞后 ~100ms），
/// ACC_PUSH_POS 是伴奏线程实时推流位置，cover seek 到此处才能和伴奏同步
pub static ACC_PUSH_POS: AtomicUsize = AtomicUsize::new(0);

// ── 伴奏 ──

/// 当前伴奏音频数据 (48kHz 单声道 f32)
static ACC_AUDIO_GLOBAL: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// 是否需要重新加载伴奏（引擎线程检查此标志）
pub static ACC_NEED_RELOAD: AtomicBool = AtomicBool::new(false);

/// 伴奏播放暂停标志（引擎线程检查此标志）
pub static ACC_PAUSED: AtomicBool = AtomicBool::new(false);

/// 伴奏是否已全部推入环形缓冲（由伴奏线程置位，输出级在缓冲排空后判定播放结束）
pub static ACC_ENDED: AtomicBool = AtomicBool::new(false);

/// 加载新伴奏音频到全局缓冲区（自动播放）
pub fn load_audio(samples: Vec<f32>) {
    if let Ok(mut guard) = ACC_AUDIO_GLOBAL.lock() {
        *guard = samples;
    }
    PLAYBACK_COUNTER.store(0, Ordering::SeqCst);
    ACC_PUSH_POS.store(0, Ordering::SeqCst);
    ACC_PAUSED.store(false, Ordering::Relaxed);
    ACC_ENDED.store(false, Ordering::Relaxed);
    SONG_VERSION.fetch_add(1, Ordering::SeqCst);
    ACC_NEED_RELOAD.store(true, Ordering::Relaxed);
}

/// 开始播放当前已加载的伴奏
pub fn play() {
    ACC_PAUSED.store(false, Ordering::Relaxed);
    ACC_ENDED.store(false, Ordering::Relaxed);
}

/// 引擎线程读取当前伴奏数据（返回克隆，避免锁争用）
pub fn get_audio() -> Vec<f32> {
    if let Ok(guard) = ACC_AUDIO_GLOBAL.lock() {
        guard.clone()
    } else {
        Vec::new()
    }
}

/// 检查当前是否有伴奏数据载入
pub fn has_audio() -> bool {
    if let Ok(guard) = ACC_AUDIO_GLOBAL.lock() {
        !guard.is_empty()
    } else {
        false
    }
}

/// 清除伴奏（停止播放）— 同时递增 SONG_VERSION 触发所有线程重置
pub fn clear() {
    if let Ok(mut guard) = ACC_AUDIO_GLOBAL.lock() {
        guard.clear();
    }
    // 重置播放时钟
    PLAYBACK_COUNTER.store(0, Ordering::SeqCst);
    ACC_PUSH_POS.store(0, Ordering::SeqCst);
    ACC_PAUSED.store(false, Ordering::Relaxed);
    ACC_ENDED.store(false, Ordering::Relaxed);
    // ⚠️ 递增 SONG_VERSION：引擎线程检测到后 drain buffer + 发送 stopped 事件
    SONG_VERSION.fetch_add(1, Ordering::SeqCst);
    ACC_NEED_RELOAD.store(true, Ordering::Relaxed);
}

// ── 当前歌曲信息（用于同步到桌面歌词窗口等子窗口） ──

static CURRENT_INSTRUMENTAL_PATH: Mutex<String> = Mutex::new(String::new());
static CURRENT_LRC_PATH: Mutex<String> = Mutex::new(String::new());
static CURRENT_COVER_PATH: Mutex<String> = Mutex::new(String::new());

/// 设置当前加载的歌曲路径信息
pub fn set_current_song_paths(instrumental_path: &str, lrc_path: &str, cover_path: &str) {
    if let Ok(mut guard) = CURRENT_INSTRUMENTAL_PATH.lock() {
        *guard = instrumental_path.to_string();
    }
    if let Ok(mut guard) = CURRENT_LRC_PATH.lock() {
        *guard = lrc_path.to_string();
    }
    if let Ok(mut guard) = CURRENT_COVER_PATH.lock() {
        *guard = cover_path.to_string();
    }
}

/// 获取当前伴奏路径
pub fn get_current_instrumental_path() -> String {
    if let Ok(guard) = CURRENT_INSTRUMENTAL_PATH.lock() {
        guard.clone()
    } else {
        String::new()
    }
}

/// 获取当前歌词路径
pub fn get_current_lrc_path() -> String {
    if let Ok(guard) = CURRENT_LRC_PATH.lock() {
        guard.clone()
    } else {
        String::new()
    }
}

/// 获取当前翻唱路径
pub fn get_current_cover_path() -> String {
    if let Ok(guard) = CURRENT_COVER_PATH.lock() {
        guard.clone()
    } else {
        String::new()
    }
}

/// 清除当前歌曲路径信息
pub fn clear_current_song_paths() {
    if let Ok(mut guard) = CURRENT_INSTRUMENTAL_PATH.lock() {
        guard.clear();
    }
    if let Ok(mut guard) = CURRENT_LRC_PATH.lock() {
        guard.clear();
    }
    if let Ok(mut guard) = CURRENT_COVER_PATH.lock() {
        guard.clear();
    }
}

// ── 歌词 ──

/// LRC 歌词文件路径（歌词线程从 LRC_PATH 读取文件）
/// LRC_VERSION 每次路径变化时递增，歌词线程通过版本号检测变化
/// 避免路径"先清空再设置"发生在同一 tick 内无法被检测的问题
static LRC_PATH: Mutex<String> = Mutex::new(String::new());
pub static LRC_VERSION: AtomicUsize = AtomicUsize::new(0);

/// 设置歌词文件路径（歌词线程自动加载）
pub fn set_lrc_path(path: String) {
    if let Ok(mut guard) = LRC_PATH.lock() {
        *guard = path;
    }
    LRC_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// 获取当前歌词文件路径
pub fn get_lrc_path() -> String {
    if let Ok(guard) = LRC_PATH.lock() {
        guard.clone()
    } else {
        String::new()
    }
}

/// 清除歌词
pub fn clear_lrc() {
    if let Ok(mut guard) = LRC_PATH.lock() {
        guard.clear();
    }
    LRC_VERSION.fetch_add(1, Ordering::Relaxed);
}

// ── 翻唱人声 ──

/// 当前翻唱人声音频数据 (48kHz 单声道 f32)
static COVER_AUDIO_GLOBAL: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// 是否需要重新加载翻唱人声（引擎线程检查此标志）
pub static COVER_NEED_RELOAD: AtomicBool = AtomicBool::new(false);

/// 翻唱人声播放标志
pub static COVER_PLAYING: AtomicBool = AtomicBool::new(false);

/// 加载翻唱人声音频到全局缓冲区
pub fn load_cover(samples: Vec<f32>) {
    if let Ok(mut guard) = COVER_AUDIO_GLOBAL.lock() {
        *guard = samples;
    }
    COVER_NEED_RELOAD.store(true, Ordering::Relaxed);
}

/// 开始/停止播放翻唱人声
pub fn set_cover_playing(playing: bool) {
    COVER_PLAYING.store(playing, Ordering::Relaxed);
}

/// 引擎线程读取当前翻唱人声数据
pub fn get_cover_audio() -> Vec<f32> {
    if let Ok(guard) = COVER_AUDIO_GLOBAL.lock() {
        guard.clone()
    } else {
        Vec::new()
    }
}

/// 清除翻唱人声
pub fn clear_cover() {
    if let Ok(mut guard) = COVER_AUDIO_GLOBAL.lock() {
        guard.clear();
    }
    COVER_NEED_RELOAD.store(true, Ordering::Relaxed);
    COVER_PLAYING.store(false, Ordering::Relaxed);
}

// ── 伴奏调性（由 play_accompaniment 写入，修音线程读取） ──

/// 伴奏检测到的调性根音 0=C..11=B，-1 表示未检测
pub static ACCOMPANIMENT_KEY_ROOT: AtomicI32 = AtomicI32::new(-1);
/// 伴奏检测到的调性大小调
pub static ACCOMPANIMENT_KEY_MAJOR: AtomicBool = AtomicBool::new(true);

/// 写入伴奏调性检测结果
pub fn set_accompaniment_key(root: u8, is_major: bool) {
    ACCOMPANIMENT_KEY_ROOT.store(root as i32, Ordering::Relaxed);
    ACCOMPANIMENT_KEY_MAJOR.store(is_major, Ordering::Relaxed);
}

// ── 播放进度 Seek ──
pub static ACC_SEEK_POSITION: AtomicI32 = AtomicI32::new(-1);
pub static ACC_SEEK_VERSION: AtomicUsize = AtomicUsize::new(0);

// ── 独立伴奏输出流（引擎不运行时用 CPAL 直接播放） ──

/// 启动独立伴奏输出流（含翻唱人声混音）
/// 当直播引擎未运行时由 play_accompaniment 调用，直接从 ACC_AUDIO_GLOBAL / COVER_AUDIO_GLOBAL 读取
/// 通过 CPAL 输出到默认音频设备，无需引擎输出回调
pub fn start_standalone_output() {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::atomic::Ordering;

    // 串行标志禁止重复启动
    if crate::audio_engine::STANDALONE_ACC_OUTPUT.swap(true, Ordering::SeqCst) {
        return;
    }

    let audio = get_audio();
    if audio.is_empty() {
        crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
        return;
    }

    // 读取翻唱人声数据（可能为空）
    let cover_audio = get_cover_audio();

    std::thread::spawn(move || {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                eprintln!("[独立输出] 无默认输出设备");
                crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
                return;
            }
        };

        // 使用设备默认输出格式，避免硬编码 48kHz/单声道在部分设备上建流失败
        let supported = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[独立输出] 读取默认格式失败: {}", e);
                crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
                return;
            }
        };
        let config: cpal::StreamConfig = supported.config();
        let out_sr = config.sample_rate.0;
        let out_ch = config.channels as usize;
        // 48kHz 源 → 设备采样率的步进（f64 累加，长期无漂移）
        let step = 48_000.0f64 / out_sr as f64;

        let audio_arc = Arc::new(audio);
        let cover_arc = Arc::new(Mutex::new(cover_audio));
        // 播放位置（48kHz 采样数，f64 位模式存储）
        let pos_atom = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let local_seek_ver = Arc::new(std::sync::atomic::AtomicUsize::new(
            ACC_SEEK_VERSION.load(Ordering::Relaxed),
        ));

        let cb_pos = pos_atom.clone();
        let cb_audio = audio_arc.clone();
        let cb_cover = cover_arc.clone();
        let cb_seek_ver = local_seek_ver.clone();

        let stream = match device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let frames = data.len() / out_ch.max(1);
                if frames == 0 {
                    return;
                }

                // 检测 seek（伴奏与翻唱共享同一位置）
                let cur_seek = ACC_SEEK_VERSION.load(Ordering::Relaxed);
                if cur_seek != cb_seek_ver.load(Ordering::Relaxed) {
                    cb_seek_ver.store(cur_seek, Ordering::Relaxed);
                    let target = ACC_SEEK_POSITION.load(Ordering::Relaxed);
                    if target >= 0 {
                        cb_pos.store((target as f64).to_bits(), Ordering::Relaxed);
                    }
                }

                // 暂停 → 静音（不推进位置，恢复后从原处继续）
                if ACC_PAUSED.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    return;
                }

                // 音源模式与翻唱开关在回调入口读取一次
                let cover_enabled = crate::audio_engine::AUDIO_SOURCE.load(Ordering::Relaxed) == 1;
                let cover_playing = COVER_PLAYING.load(Ordering::Relaxed);
                let cover_guard = if cover_enabled && cover_playing {
                    cb_cover.lock().ok()
                } else {
                    None
                };
                let cover: Option<&[f32]> = cover_guard.as_ref().map(|g| g.as_slice());
                let acc: &[f32] = cb_audio.as_slice();

                let mut pos = f64::from_bits(cb_pos.load(Ordering::Relaxed));
                for f in 0..frames {
                    let src = pos + f as f64 * step;
                    let si = src as usize;
                    let fr = (src - si as f64) as f32;

                    // 伴奏（线性插值）
                    let a = if si + 1 < acc.len() {
                        acc[si] * (1.0 - fr) + acc[si + 1] * fr
                    } else if si < acc.len() {
                        acc[si]
                    } else {
                        0.0
                    };

                    // 翻唱（仅在有效区间内混入，避免人声比伴奏短时整体音量骤降）
                    let mut s = a;
                    if let Some(cv) = cover {
                        if si < cv.len() {
                            let c = if si + 1 < cv.len() {
                                cv[si] * (1.0 - fr) + cv[si + 1] * fr
                            } else {
                                cv[si]
                            };
                            s = a * 0.5 + c * 0.5;
                        }
                    }

                    let base = f * out_ch;
                    for ch in 0..out_ch {
                        data[base + ch] = s;
                    }
                }

                pos += frames as f64 * step;
                cb_pos.store(pos.to_bits(), Ordering::Relaxed);
            },
            |err| eprintln!("[独立输出] 错误: {}", err),
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[独立输出] 创建输出流失败: {}", e);
                crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
                return;
            }
        };

        if let Err(e) = stream.play() {
            eprintln!("[独立输出] 播放失败: {}", e);
            crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
            return;
        }

        // 主循环：定期同步进度到 PLAYBACK_COUNTER + 刷新 cover 数据，等退出信号或播放完毕
        let total_len = audio_arc.len();
        let mut last_cover_playing = false;
        while crate::audio_engine::STANDALONE_ACC_OUTPUT.load(Ordering::SeqCst) {
            let pos = f64::from_bits(pos_atom.load(Ordering::Relaxed));
            PLAYBACK_COUNTER.store(pos.min(total_len as f64) as usize, Ordering::SeqCst);

            let cover_playing = COVER_PLAYING.load(Ordering::Relaxed);

            // ⚠️ cover 从 false→true：重新加载 cover 数据（伴奏与翻唱共享位置，天然对齐）
            if cover_playing && !last_cover_playing {
                let fresh = get_cover_audio();
                if let Ok(mut guard) = cover_arc.lock() {
                    if !fresh.is_empty() {
                        *guard = fresh;
                    }
                }
            }
            last_cover_playing = cover_playing;

            if pos >= total_len as f64 {
                // 播放完毕：进度定格在末尾并冻结，等待下一次 play() 复位
                PLAYBACK_COUNTER.store(total_len, Ordering::SeqCst);
                ACC_PAUSED.store(true, Ordering::Relaxed);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        drop(stream);
        crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
    });
}

/// 停止独立伴奏输出流
pub fn stop_standalone_output() {
    crate::audio_engine::STANDALONE_ACC_OUTPUT.store(false, Ordering::SeqCst);
}
