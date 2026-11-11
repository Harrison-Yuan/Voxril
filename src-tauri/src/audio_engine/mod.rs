//! 直播音频引擎 — 模块入口
//!
//! 这里只保留：
//! - 模块声明
//! - 共享配置类型和全局标志
//! - 通用工具函数
//!
//! 引擎逻辑拆分到了 `engine.rs` 和 `mixer.rs`：
//! - engine.rs: 流构建 + 线程编排
//! - mixer.rs: 多路音频混音

pub mod types;
pub mod rvc;
pub mod config;
pub mod accompaniment;

pub mod pitch_correction;
pub mod auto_tune;
pub mod voice_state;
pub mod lrc;
pub mod daw;
pub mod blackhole;

pub mod smoother;
pub mod resampler;
pub mod thread;

pub mod mixer;
pub mod engine;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32};
use tauri::Emitter;

/// 静音填充 buffer（输出端帧率对齐用）
pub(crate) static SILENT_CHUNK: [f32; 960] = [0.0; 960];

// 静态音高偏移量（半音数），由前端 set_live_f0_up_key 命令实时写入
pub static F0_UP_KEY: AtomicI32 = AtomicI32::new(0);

// 特征检索融合度，由前端 set_live_index_rate 实时写入
pub static INDEX_RATE: AtomicU32 = AtomicU32::new(0);

// 麦克风静音标志 true=静音，前端通过 set_mic_mute 命令写入
pub static MIC_MUTED: AtomicBool = AtomicBool::new(false);

// 音源模式：0=真人演唱，1=AI 翻唱
// 真人: MIC_MUTED=false, cover 不混入
// 翻唱: MIC_MUTED=true,  cover 混入
pub static AUDIO_SOURCE: AtomicU32 = AtomicU32::new(0);

// ── 直播唱歌配置（从 Tauri 命令透传） ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveSingingConfig {
    pub input_device: String,
    pub output_device: String,
    pub mode: String,
    pub scale_root: i32,
    pub scale_name: String,
    pub model_path: String,
    pub vec_path: String,
    pub index_path: String,
    pub rmvpe_path: String,
    pub fcpe_path: String,
    pub f0_score_path: String,
    pub f0_up_key: i32,
    pub accompaniment_path: String,
    pub cover_path: String,
    pub duck_gain: f32,
    pub index_rate: f64,
}

// ── Ring Buffer 工具 ──

pub(crate) fn push_ringbuf(prod: &mut ringbuf::HeapProducer<f32>, data: &[f32]) -> usize {
    let mut written = 0usize;
    while written < data.len() {
        let n = prod.push_slice(&data[written..]);
        if n == 0 {
            break;
        }
        written += n;
    }
    written
}

pub(crate) fn push_ringbuf_with_latency_cap(
    prod: &mut ringbuf::HeapProducer<f32>,
    data: &[f32],
    max_buffered: usize,
) -> usize {
    if prod.len() >= max_buffered {
        return 0;
    }
    let allow = (max_buffered - prod.len()).min(data.len());
    push_ringbuf(prod, &data[..allow])
}

pub(crate) fn keep_recent_samples(buf: &mut Vec<f32>, max_len: usize) {
    if buf.len() > max_len {
        let drop_len = buf.len() - max_len;
        buf.drain(0..drop_len);
    }
}

// ── 音频解码 ──

/// 通用音频解码辅助：加载 WAV/FLAC，混音到 48kHz 单声道 f32
pub(crate) fn common_decode_cover(
    path: &str,
    app: &tauri::AppHandle,
) -> Result<Vec<f32>, String> {
    use std::io::BufReader;
    let file = std::fs::File::open(path).map_err(|e| format!("打开失败: {}", e))?;
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext != "wav" && ext != "flac" {
        return Err(format!("不支持的格式: {}", ext));
    }
    let mut reader = hound::WavReader::new(BufReader::new(file))
        .map_err(|e| format!("解码失败: {}", e))?;
    let sr = reader.spec().sample_rate;
    let channels = reader.spec().channels as usize;
    emit_log(
        app,
        &format!(
            "[解码] 读取 {:.1}s WAV/FLAC ({}Hz, {}ch)",
            reader.duration() as f64 / sr as f64,
            sr,
            channels
        ),
    );
    let mut buf: Vec<f32> = Vec::new();
    while let Some(Ok(s)) = reader.samples::<i16>().next() {
        buf.push(s as f32 / 32768.0);
    }
    let mono: Vec<f32> = if channels > 1 {
        buf.chunks(channels)
            .map(|ch| {
                let sum: f32 = ch.iter().sum();
                let avg = sum / channels as f32;
                let gain = avg * 1.5;
                gain / (1.0 + gain * gain).sqrt()
            })
            .collect()
    } else {
        buf
    };
    if sr != 48000 {
        let ratio = 48000.0 / sr as f64;
        let olen = (mono.len() as f64 * ratio) as usize;
        Ok((0..olen)
            .map(|i| {
                let sp = i as f64 / ratio;
                let si = sp as usize;
                let fr = (sp - si as f64) as f32;
                if si + 1 < mono.len() {
                    mono[si] * (1.0 - fr) + mono[si + 1] * fr
                } else if si < mono.len() {
                    mono[si]
                } else {
                    *mono.last().unwrap_or(&0.0)
                }
            })
            .collect())
    } else {
        Ok(mono)
    }
}

// ── 日志/进度 ──

pub(crate) fn emit_log(app_handle: &tauri::AppHandle, msg: &str) {
    eprintln!("{}", msg);
    let _ = app_handle.emit("live-singing-log", msg);
}

pub(crate) fn emit_progress(app_handle: &tauri::AppHandle, percent: u32, message: &str) {
    let _ = app_handle.emit(
        "live-singing-progress",
        serde_json::json!({
            "percent": percent,
            "message": message,
        }),
    );
}
