use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceState {
    MUTE,
    SPEAKING,
    SINGING,
}

impl VoiceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            VoiceState::MUTE => "mute",
            VoiceState::SPEAKING => "speaking",
            VoiceState::SINGING => "singing",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub sample_rate: u32,
    pub block_size: usize,
    pub f0_up_key: i32,
    pub accompaniment_path: Option<PathBuf>,
    pub duck_gain: f32,
}

impl EngineConfig {
    pub fn frame_duration_ms(&self) -> f32 {
        self.block_size as f32 / self.sample_rate as f32 * 1000.0
    }
}

#[derive(Debug, Clone)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub timestamp: std::time::Instant,
}

impl AudioChunk {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
            timestamp: std::time::Instant::now(),
        }
    }

    pub fn duration_ms(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32 * 1000.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FrameFeatures {
    pub rms: f32,
    pub f0: f32,
    pub f0_continuity: f32,
    pub hnr: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineMetrics {
    pub state: VoiceState,
    pub rms: f32,
    pub f0_continuity: f32,
    pub hnr: f32,
    pub f0: f32,
}

/// 预提取的原唱 F0 音高曲线。
///
/// `f0_curve[i]` 对应时间点 `i * hop_length / sample_rate` 秒处的基频（Hz），
/// 0.0 表示无声音/无声段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct F0Profile {
    pub f0_curve: Vec<f32>,
    pub sample_rate: u32,
    pub hop_length: usize,
}

impl F0Profile {
    pub fn is_empty(&self) -> bool {
        self.f0_curve.is_empty()
    }

    pub fn duration_secs(&self) -> f32 {
        self.f0_curve.len() as f32 * self.hop_length as f32 / self.sample_rate as f32
    }

    /// 根据音频采样位置查找 F0 帧索引。
    /// `position_samples`: 伴奏当前播放到第几个采样点（引擎采样率）。
    /// 注意：如果 profile 采样率与调用端不同，需调用方自行转换。
    pub fn frame_at(&self, position_samples: usize) -> usize {
        (position_samples / self.hop_length).min(self.f0_curve.len().saturating_sub(1))
    }

    /// 查找从 `position_samples` 开始的 `count` 帧 F0 值。
    /// 越界部分用 0.0 填充（表示无声段）。
    pub fn f0_slice(&self, position_samples: usize, count: usize) -> Vec<f32> {
        let start = self.frame_at(position_samples);
        let end = (start + count).min(self.f0_curve.len());
        let mut out = Vec::with_capacity(count);
        out.extend_from_slice(&self.f0_curve[start..end]);
        out.resize(count, 0.0);
        out
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            block_size: 1920,
            f0_up_key: 0,
            accompaniment_path: None,
            duck_gain: 0.3,
        }
    }
}
