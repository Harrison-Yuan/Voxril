//! Sinc 插值重采样器（流式封装）
//!
//! 基于 rubato 库的 `SincFixedIn` 引擎，Blackman2 窗 + Quadratic 插值，
//! 替换手写线性插值，消除混叠失真（雪花音）。
//!
//! # 实时约束
//! `feed` / `drain_into` 全程复用构造期预分配的缓冲，不分配内存、不加锁，
//! 可直接在音频回调中调用。输入输出长度不必相等：
//! - `feed()` 喂入任意长度输入，内部按固定 chunk 处理，产出追加到 `pending`；
//! - `drain_into()` 取走 `pending` 中最多 `out.len()` 帧。
//!
//! 这种「先攒后取」的设计让调用方可以按设备回调的真实帧数取数，
//! 避免旧实现中「每次多算一点然后丢弃余量」造成的时钟漂移。

use rubato::{
    calculate_cutoff, Resampler, SincFixedIn, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;

/// 流式 Sinc 插值重采样器
pub struct StreamResampler {
    inner: SincFixedIn<f32>,
    /// 待处理的输入帧（长度始终 < 一个 chunk，处理完即 drain）
    in_buf: Vec<f32>,
    /// rubato 的输出暂存（单声道）
    out_buf: Vec<Vec<f32>>,
    /// 已重采样、但调用方尚未取走的输出帧
    pending: VecDeque<f32>,
}

impl StreamResampler {
    /// 创建重采样器。
    ///
    /// - `from_rate`: 输入采样率 (Hz)
    /// - `to_rate`:   输出采样率 (Hz)
    /// - `chunk_size`: 单次内部处理的输入帧数
    pub fn new(from_rate: u32, to_rate: u32, chunk_size: usize) -> Self {
        let ratio = to_rate as f64 / from_rate as f64;
        let sinc_len = 128;
        let oversampling_factor = 256;
        let interpolation = SincInterpolationType::Quadratic;
        let window = WindowFunction::Blackman2;
        let f_cutoff = calculate_cutoff(sinc_len, window);
        let params = SincInterpolationParameters {
            sinc_len,
            f_cutoff,
            interpolation,
            oversampling_factor,
            window,
        };
        let inner = SincFixedIn::<f32>::new(ratio, 1.01, params, chunk_size, 1)
            .expect("StreamResampler::new 失败");
        let out_frames_max = inner.output_frames_max();
        Self {
            inner,
            // 需容纳「一个完整 chunk + 单次 feed 的最大输入」，预留 8 倍 chunk
            in_buf: Vec::with_capacity(chunk_size * 8),
            out_buf: vec![vec![0.0f32; out_frames_max]; 1],
            // pending 常态只积压约 1 个 chunk，预留 4 倍防止抖动时扩容
            pending: VecDeque::with_capacity(out_frames_max * 4),
        }
    }

    /// 下次 `feed` 需要的输入帧数（不足一个 chunk 时不会产生输出）
    pub fn input_frames_next(&self) -> usize {
        self.inner.input_frames_next()
    }

    /// 单次内部处理最多产出的帧数
    pub fn output_frames_max(&self) -> usize {
        self.inner.output_frames_max()
    }

    /// 已缓冲、尚未被取走的输出帧数
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 清空内部状态（换歌时调用，丢弃残留相位与缓冲）
    pub fn reset(&mut self) {
        self.in_buf.clear();
        self.pending.clear();
        self.inner.reset();
    }

    /// 喂入任意长度输入，产出追加到内部 `pending`
    pub fn feed(&mut self, input: &[f32]) {
        self.in_buf.extend_from_slice(input);
        while self.in_buf.len() >= self.inner.input_frames_next() {
            let need = self.inner.input_frames_next();
            let input_slices = [&self.in_buf[..need]];
            let (consumed, produced) = self
                .inner
                .process_into_buffer(&input_slices, &mut self.out_buf, None)
                .expect("StreamResampler::feed 处理失败");
            self.pending
                .extend(self.out_buf[0][..produced].iter().copied());
            self.in_buf.drain(..consumed);
        }
    }

    /// 取走最多 `out.len()` 个输出帧，返回实际写入数
    pub fn drain_into(&mut self, out: &mut [f32]) -> usize {
        let n = out.len().min(self.pending.len());
        for (dst, src) in out.iter_mut().zip(self.pending.drain(..n)) {
            *dst = src;
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 长时间运行后输出总量应等于输入总量 × 采样率比（无系统性漂移）
    #[test]
    fn feed_drain_has_no_long_term_drift() {
        let mut r = StreamResampler::new(48000, 44100, 512);
        let input = vec![0.1f32; 48000 * 10];
        let mut out = vec![0.0f32; r.output_frames_max()];
        let mut produced = 0usize;
        let mut offset = 0usize;
        while offset < input.len() {
            let n = (offset + 512).min(input.len()) - offset;
            r.feed(&input[offset..offset + n]);
            offset += n;
            loop {
                let w = r.drain_into(&mut out);
                if w == 0 {
                    break;
                }
                produced += w;
            }
        }
        let expected = (input.len() as f64 * 44100.0 / 48000.0) as usize;
        let diff = (produced as i64 - expected as i64).abs();
        // 仅允许 sinc 滤波器启动延迟级别的误差，不允许随运行时长累积
        assert!(diff < 512, "produced={} expected={}", produced, expected);
        // pending 不应无界增长
        assert!(r.pending_len() < r.output_frames_max() * 2);
    }
}
