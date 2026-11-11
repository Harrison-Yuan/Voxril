//! Sinc 插值重采样器（流式封装）
//!
//! 基于 rubato 库的 `SincFixedIn` 引擎，Blackman2 窗 + Quadratic 插值，
//! 替换手写线性插值，消除混叠失真（雪花音）。
//!
//! 用法：
//! ```ignore
//! let mut r = StreamResampler::new(44100, 48000, 512);
//! let out = r.push(&input);  // 推送输入，获取可输出的帧
//! ```

use rubato::{
    calculate_cutoff, Resampler, SincFixedIn, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

/// 流式 Sinc 插值重采样器
pub struct StreamResampler {
    inner: SincFixedIn<f32>,
    in_buf: Vec<f32>,
    out_buf: Vec<Vec<f32>>,
}

impl StreamResampler {
    /// 创建重采样器。
    ///
    /// - `from_rate`: 输入采样率 (Hz)
    /// - `to_rate`:   输出采样率 (Hz)
    /// - `chunk_size`: 推荐匹配音频回调块大小 (如 512)
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
            in_buf: Vec::with_capacity(chunk_size * 2),
            out_buf: vec![vec![0.0f32; out_frames_max]; 1],
        }
    }

    /// 推送新输入数据，返回本次可输出的所有重采样帧。
    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        self.in_buf.extend_from_slice(input);
        let mut result = Vec::new();
        while self.in_buf.len() >= self.inner.input_frames_next() {
            let need = self.inner.input_frames_next();
            let input_slices = [&self.in_buf[..need]];
            let (consumed, produced) = self
                .inner
                .process_into_buffer(&input_slices, &mut self.out_buf, None)
                .unwrap();
            result.extend_from_slice(&self.out_buf[0][..produced]);
            self.in_buf.drain(..consumed);
        }
        result
    }

    /// 刷新剩余数据（引擎停止时调用），返回尾部重采样帧。
    pub fn flush(&mut self) -> Vec<f32> {
        if self.in_buf.is_empty() {
            return vec![];
        }
        let input_slices = [&self.in_buf[..]];
        let (consumed, produced) = self
            .inner
            .process_partial_into_buffer(Some(&input_slices), &mut self.out_buf, None)
            .unwrap();
        self.in_buf.drain(..consumed);
        self.out_buf[0][..produced].to_vec()
    }

    /// 返回下次 push 需要的输入帧数（用于对齐帧率）。
    pub fn input_frames_next(&self) -> usize {
        self.inner.input_frames_next()
    }
}
