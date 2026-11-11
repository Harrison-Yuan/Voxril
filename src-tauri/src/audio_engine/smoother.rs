//! 音频防爆音平滑器 — 淡出/淡入状态机
//!
//! `StreamSmoother` 用于实时音频回调中平滑处理 underrun：
//!   - 数据中断时 5ms 淡出 (None)
//!   - 数据恢复时 5ms 淡入 (Some)
//!   - 避免波形突兀切换导致的"噼啪"爆音

/// 淡出/淡入时长 (样本数 @48kHz = 5ms)
const OUTPUT_FADE_SAMPLES: usize = 240;

/// 每个声道独立的状态平滑器
pub struct StreamSmoother {
    last_output: f32,
    fade_out_anchor: f32,
    fade_out_pos: usize,
    fade_in_pos: usize,
    recovering: bool,
}

impl StreamSmoother {
    pub fn new() -> Self {
        Self {
            last_output: 0.0,
            fade_out_anchor: 0.0,
            fade_out_pos: 0,
            fade_in_pos: 0,
            recovering: false,
        }
    }

    /// 处理一个采样帧。
    ///
    /// - `Some(sample)`: 正常数据，若前帧是 underrun 则自动淡入恢复
    /// - `None`:        underrun，从当前音量开始 5ms 淡出到 0
    pub fn next(&mut self, input: Option<f32>) -> f32 {
        match input {
            Some(sample) => {
                self.fade_out_pos = 0;
                let mut out = sample;
                if self.recovering {
                    let t = (self.fade_in_pos as f32 / OUTPUT_FADE_SAMPLES as f32).clamp(0.0, 1.0);
                    out *= t;
                    self.fade_in_pos = (self.fade_in_pos + 1).min(OUTPUT_FADE_SAMPLES);
                    if self.fade_in_pos >= OUTPUT_FADE_SAMPLES {
                        self.recovering = false;
                    }
                }
                self.last_output = out;
                out
            }
            None => {
                if self.fade_out_pos == 0 {
                    self.fade_out_anchor = self.last_output;
                    self.recovering = true;
                    self.fade_in_pos = 0;
                }
                let t = 1.0 - (self.fade_out_pos as f32 / OUTPUT_FADE_SAMPLES as f32).clamp(0.0, 1.0);
                let out = self.fade_out_anchor * t;
                self.fade_out_pos = (self.fade_out_pos + 1).min(OUTPUT_FADE_SAMPLES);
                self.last_output = out;
                out
            }
        }
    }
}
