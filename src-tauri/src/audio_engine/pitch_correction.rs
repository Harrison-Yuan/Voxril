//! 实时修音引擎 — Phase Vocoder 音高移动
//!
//! 参数: FFT=2048, hop=512, sr=48000, bin_width=23.4Hz
//!
//! # 契约（重要）
//! [`PitchCorrector::process`] **对外输出长度恒等于输入长度**。
//! 内部通过「预滚零 + 待输出队列」吸收 Phase Vocoder 的固有延迟，
//! 因此调用方可以按块直接写入环形缓冲区，无需关心块长抖动。
//!
//! 固有算法延迟 = `PV_FFT_SIZE` 采样（约 42.7ms @48kHz），
//! 由构造时预填的静音历史抵消，属于相位声码器的物理下限。
//!
//! 信号链:
//!   麦克风块 → 累积 → 加窗 FFT2048 → 频域移调 → 共轭对称修补
//!   → IFFT → WOLA(2/3) → 定长输出

use rustfft::{FftPlanner, num_complex::Complex};
use std::collections::VecDeque;
use std::sync::Arc;

// ── STFT 参数 ──
// 2048 点 FFT = 23.4Hz/bin @48kHz，足以精准分离人声基频和各次谐波
pub const PV_FFT_SIZE: usize = 2048;
pub const PV_HOP: usize = 512;

/// Hann 窗 + 75% 重叠时的 WOLA 归一化系数（sum(w^2) = 1.5）
const WOLA_SCALE: f32 = 2.0 / 3.0;

/// 判定为直通的音高比阈值（±0.02% ≈ 0.35 音分，低于人耳分辨极限）
const PASSTHROUGH_EPS: f32 = 0.0002;

/// 实时相位声码器
///
/// 每个处理帧：
///   1. FFT 分析 → 瞬时幅度 + 绝对瞬时频率
///   2. 按 ratio 同步插值映射到新频域
///   3. 瞬时频率相位传播（保证时域连续）
///   4. 共轭对称修补 → IFFT → WOLA → 定长输出
pub struct PitchCorrector {
    /// 待处理输入（构造时预填 `PV_FFT_SIZE` 个零作为静音历史）
    input_buf: VecDeque<f32>,
    /// 已合成但尚未交付给调用方的音频
    pending: VecDeque<f32>,
    /// OLA 输出的重叠尾部（长度 `PV_FFT_SIZE`，与 mixed 原地轮转）
    output_tail: Vec<f32>,
    /// 分析相位 [0..n_bins]
    ana_phase: Vec<f64>,
    /// 合成相位 [0..n_bins]
    syn_phase: Vec<f64>,
    /// Hann 分析窗
    window: Vec<f32>,
    /// 正/逆 FFT 计划（构造时缓存，避免每帧查表）
    fft_fwd: Arc<dyn rustfft::Fft<f32>>,
    fft_inv: Arc<dyn rustfft::Fft<f32>>,
    /// 已处理的帧数（用于首帧相位初始化）
    frame_count: usize,
    // ── 复用缓冲，避免实时路径每帧分配 ──
    fft_in: Vec<Complex<f32>>,
    fft_out: Vec<Complex<f32>>,
    ana_mag: Vec<f32>,
    ana_freq: Vec<f64>,
}

impl PitchCorrector {
    pub fn new() -> Self {
        let n_bins = PV_FFT_SIZE / 2 + 1;
        let n = PV_FFT_SIZE as f32;
        // 周期 Hann 窗（首尾为 0），保证 WOLA 完美重建
        let window: Vec<f32> = (0..PV_FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n).cos()))
            .collect();

        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(PV_FFT_SIZE);
        let fft_inv = planner.plan_fft_inverse(PV_FFT_SIZE);

        // 预滚：把 PV 的固有延迟显式化为「静音历史」，使首块即可定长输出
        let mut input_buf = VecDeque::with_capacity(PV_FFT_SIZE * 2);
        input_buf.extend(std::iter::repeat(0.0f32).take(PV_FFT_SIZE));

        Self {
            input_buf,
            pending: VecDeque::with_capacity(PV_FFT_SIZE * 2),
            output_tail: vec![0.0; PV_FFT_SIZE],
            ana_phase: vec![0.0; n_bins],
            syn_phase: vec![0.0; n_bins],
            window,
            fft_fwd,
            fft_inv,
            frame_count: 0,
            fft_in: vec![Complex::new(0.0, 0.0); PV_FFT_SIZE],
            fft_out: vec![Complex::new(0.0, 0.0); PV_FFT_SIZE],
            ana_mag: vec![0.0; n_bins],
            ana_freq: vec![0.0; n_bins],
        }
    }

    /// 处理音频并移动音高。
    ///
    /// - `audio`: 输入音频 (48kHz, 任意长度)
    /// - `pitch_ratio`: 目标/原始音高比 (>1=升调, <1=降调, 1=不变)
    ///
    /// **返回值长度恒等于 `audio.len()`**，内部吸收相位声码器的固有延迟。
    pub fn process(&mut self, audio: &[f32], pitch_ratio: f32) -> Vec<f32> {
        let want = audio.len();
        if want == 0 {
            return Vec::new();
        }
        self.input_buf.extend(audio.iter().copied());

        // 处理帧直到待输出库存足够交付
        while self.pending.len() < want {
            if self.input_buf.len() < PV_FFT_SIZE {
                // 稳态下不会发生（长期输入/输出 1:1）；防御性退出
                break;
            }
            self.process_one_frame(pitch_ratio);
        }

        let mut out = Vec::with_capacity(want);
        let avail = self.pending.len().min(want);
        out.extend(self.pending.drain(..avail));
        // 欠载补零：保持时间轴同步（不可丢样本，否则整条链路会撕裂）
        out.resize(want, 0.0);
        out
    }

    /// 处理单个 STFT 帧：消耗 `PV_HOP` 个输入，产出 `PV_HOP` 个输出进 `pending`
    fn process_one_frame(&mut self, pitch_ratio: f32) {
        let n_bins = PV_FFT_SIZE / 2 + 1;
        let ratio = pitch_ratio.clamp(0.5, 2.0);
        let is_passthrough = (ratio - 1.0).abs() < PASSTHROUGH_EPS;

        // 1. 加窗 + FFT 分析
        for i in 0..PV_FFT_SIZE {
            self.fft_in[i] = Complex::new(self.input_buf[i] * self.window[i], 0.0);
        }
        self.fft_fwd.process(&mut self.fft_in);

        // 2. 瞬时幅度 / 瞬时频率
        let first_frame = self.frame_count == 0;
        for bin in 0..n_bins {
            let phase = self.fft_in[bin].arg() as f64;
            if first_frame {
                self.ana_phase[bin] = phase;
                self.syn_phase[bin] = phase;
            }
            let base_advance =
                2.0 * std::f64::consts::PI * bin as f64 * PV_HOP as f64 / PV_FFT_SIZE as f64;
            let mut phase_diff = phase - self.ana_phase[bin] - base_advance;
            while phase_diff > std::f64::consts::PI {
                phase_diff -= 2.0 * std::f64::consts::PI;
            }
            while phase_diff < -std::f64::consts::PI {
                phase_diff += 2.0 * std::f64::consts::PI;
            }
            self.ana_phase[bin] = phase;
            self.ana_mag[bin] = self.fft_in[bin].norm();
            self.ana_freq[bin] = base_advance + phase_diff;
        }

        // 3. 频域映射与相位传播
        if is_passthrough {
            // 不需要变调：完整复制频谱，保留真实相位与瞬态
            self.fft_out.copy_from_slice(&self.fft_in);
            for bin in 0..n_bins {
                self.syn_phase[bin] = self.fft_in[bin].arg() as f64;
            }
        } else {
            for bin in 0..n_bins {
                let src_bin = bin as f32 / ratio;
                let (mag, freq) = if src_bin < (n_bins - 1) as f32 {
                    let lo = src_bin.floor().max(0.0) as usize;
                    let hi = lo + 1;
                    let frac = src_bin - lo as f32;
                    (
                        self.ana_mag[lo] * (1.0 - frac) + self.ana_mag[hi] * frac,
                        self.ana_freq[lo] * (1.0 - frac as f64)
                            + self.ana_freq[hi] * (frac as f64),
                    )
                } else if (src_bin as usize) < n_bins {
                    (self.ana_mag[src_bin as usize], self.ana_freq[src_bin as usize])
                } else {
                    (0.0, 0.0)
                };
                self.syn_phase[bin] += freq * ratio as f64;
                self.syn_phase[bin] %= 2.0 * std::f64::consts::PI;
                self.fft_out[bin] = Complex::from_polar(mag, self.syn_phase[bin] as f32);
            }
        }

        // 4. 严格共轭对称（实信号 IFFT 的物理前提，缺失会导致 −6dB 且残留虚部）
        self.enforce_hermitian();

        // 5. IFFT → WOLA
        self.fft_inv.process(&mut self.fft_out);
        let scale = WOLA_SCALE / PV_FFT_SIZE as f32;
        // 原地轮转：out[i] = tail[i] + frame[i] * scale
        for i in 0..PV_FFT_SIZE {
            let v = self.output_tail[i] + self.fft_out[i].re * scale * self.window[i];
            if i < PV_HOP {
                self.pending.push_back(v);
            } else {
                // 写入的位置 i - PV_HOP 已在上方被读取且不再使用
                self.output_tail[i - PV_HOP] = v;
            }
        }

        self.input_buf.drain(..PV_HOP);
        self.frame_count += 1;
    }

    /// 修补 FFT 输出的共轭对称性，并清理直流/奈奎斯特的虚部
    fn enforce_hermitian(&mut self) {
        let n_bins = PV_FFT_SIZE / 2 + 1;
        self.fft_out[0].im = 0.0;
        self.fft_out[n_bins - 1].im = 0.0;
        for bin in 1..(PV_FFT_SIZE / 2) {
            self.fft_out[PV_FFT_SIZE - bin] = self.fft_out[bin].conj();
        }
    }

    /// 冲刷内部缓冲：输出待交付音频与未处理输入，并复位状态。
    pub fn flush(&mut self) -> Vec<f32> {
        let mut out: Vec<f32> = self.pending.drain(..).collect();
        out.extend(self.input_buf.drain(..));
        self.output_tail.fill(0.0);
        self.ana_phase.fill(0.0);
        self.syn_phase.fill(0.0);
        self.frame_count = 0;
        // 重新预滚，便于复用实例
        self.input_buf
            .extend(std::iter::repeat(0.0f32).take(PV_FFT_SIZE));
        out
    }
}

impl Default for PitchCorrector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: usize = 960;

    fn sine_chunk(freq: f32, amp: f32, start_sample: usize, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let t = (start_sample + i) as f32 / 48000.0;
                (2.0 * std::f32::consts::PI * freq * t).sin() * amp
            })
            .collect()
    }

    fn rms(buf: &[f32]) -> f32 {
        if buf.is_empty() {
            return 0.0;
        }
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    /// 契约 1：输出长度恒等于输入长度（含首块）
    #[test]
    fn output_len_equals_input_len() {
        let mut c = PitchCorrector::new();
        for block in 0..40 {
            let chunk = sine_chunk(220.0, 0.5, block * CHUNK, CHUNK);
            let out = c.process(&chunk, 1.0);
            assert_eq!(out.len(), CHUNK, "第 {} 块长度不匹配", block);
            assert!(out.iter().all(|s| s.is_finite()), "输出含非有限值");
        }
    }

    /// 契约 2：直通（ratio=1）稳态增益 ≈ 1，不得出现 −6dB 抽吸
    #[test]
    fn passthrough_unity_gain() {
        let mut c = PitchCorrector::new();
        let amp = 0.5;
        let mut in_tail: Vec<f32> = Vec::new();
        let mut out_tail: Vec<f32> = Vec::new();
        let total_blocks = 24;
        for block in 0..total_blocks {
            let chunk = sine_chunk(440.0, amp, block * CHUNK, CHUNK);
            let out = c.process(&chunk, 1.0);
            // 跳过前 4 块（预滚 2048 + WOLA 收敛）
            if block >= total_blocks - 4 {
                in_tail.extend_from_slice(&chunk);
                out_tail.extend_from_slice(&out);
            }
        }
        let gain = rms(&out_tail) / rms(&in_tail);
        assert!(
            (gain - 1.0).abs() < 0.15,
            "直通增益应约为 1，实测 {:.4}",
            gain
        );
    }

    /// 契约 3：变调后长度不变，且输出有限
    #[test]
    fn pitch_shift_keeps_len() {
        let mut c = PitchCorrector::new();
        let ratio = 2.0f32.powf(4.0 / 12.0); // 升 4 个半音
        let mut all = Vec::new();
        for block in 0..16 {
            let chunk = sine_chunk(220.0, 0.5, block * CHUNK, CHUNK);
            let out = c.process(&chunk, ratio);
            assert_eq!(out.len(), CHUNK, "变调块长度不匹配: {}", block);
            all.extend(out);
        }
        assert_eq!(all.len(), 16 * CHUNK);
        assert!(all.iter().all(|s| s.is_finite()));
        // 变调后应仍有能量（不是全零）
        assert!(rms(&all[CHUNK * 8..]) > 0.05, "变调后能量异常");
    }

    /// 契约 4：连续直通 → 变调再切回直通，长度恒定且不 panic
    #[test]
    fn switching_modes_keeps_len() {
        let mut c = PitchCorrector::new();
        let ratios = [1.0f32, 1.0, 2.0f32.powf(2.0 / 12.0), 1.0, 2.0f32.powf(-3.0 / 12.0), 1.0];
        for (block, &r) in ratios.iter().cycle().take(30).enumerate() {
            let chunk = sine_chunk(330.0, 0.4, block * CHUNK, CHUNK);
            let out = c.process(&chunk, r);
            assert_eq!(out.len(), CHUNK);
            assert!(out.iter().all(|s| s.is_finite()));
        }
    }

    /// 契约 5：flush 吐出余量后状态可复用
    #[test]
    fn flush_then_reuse() {
        let mut c = PitchCorrector::new();
        for block in 0..8 {
            let chunk = sine_chunk(220.0, 0.5, block * CHUNK, CHUNK);
            let _ = c.process(&chunk, 1.0);
        }
        let tail = c.flush();
        assert!(!tail.is_empty());
        // 复用：重新处理仍满足定长契约
        for block in 0..8 {
            let chunk = sine_chunk(220.0, 0.5, block * CHUNK, CHUNK);
            let out = c.process(&chunk, 1.0);
            assert_eq!(out.len(), CHUNK);
        }
    }
}
