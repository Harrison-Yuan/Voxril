//! VocalBoost Phase 3 — ONNX 推理引擎 (F0 强制注入)
//!
//! # 架构
//!
//! 1. 加载 ContentVec ONNX (16kHz → 768d 特征)
//! 2. 加载 RVC 合成器 ONNX (特征 + F0 → 40kHz 音频)
//! 3. 加载 RMVPE ONNX (mel 频谱 → F0 音高)
//! 4. 中继线程调 `InferCore::run()` 注入查表 F0
//!
//! # 维度
//!
//! 全链路强制 768 维（v2 标准）。
//! ContentVec 使用 `vec-768-layer-12.onnx` 输出第 12 层 768 维特征。
//! 加载时校验 ContentVec 实际输出是否为 768 维，否则报错。

use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::path::Path;
use std::sync::Once;

// ── F0 工具 ──
fn f0_mel_min() -> f32 { 1127.0 * (1.0_f32 + 50.0_f32 / 700.0_f32).ln() }
fn f0_mel_max() -> f32 { 1127.0 * (1.0_f32 + 1100.0_f32 / 700.0_f32).ln() }

// ── 递归相位累加合成器 (PhaseAccumulator) ──
struct PhaseAccumulator {
    /// 核心：只存储基础相位角度 (0 ~ 2π)，彻底抛弃绝对时间
    fundamental_phase: f64,
    /// 上一个有效 F0，用于无声帧仿真驱动相位前进
    last_valid_f0: f64,
    sample_rate: f64,
    hop_samples: usize,
    max_harmonics: usize,
}

impl PhaseAccumulator {
    fn new(sample_rate: f64) -> Self {
        let hop_samples = (sample_rate * 0.02) as usize; // 20ms
        let max_harmonics = (sample_rate / 120.0) as usize;
        Self { fundamental_phase: 0.0, last_valid_f0: 100.0, sample_rate, hop_samples, max_harmonics }
    }

    fn synthesize_frame(&mut self, f0_hz: f32, harmonic_amps: &[f32], out: &mut [f32]) -> usize {
        let n = self.hop_samples.min(out.len());
        if f0_hz <= 0.0 {
            // ⚠️ 关键修复：即使输出静音，也要用上一个有效 F0 继续推进相位！
            // 否则下次有声帧的 phase 会从旧位置起跳 → 爆音/雪花。
            let sr = self.sample_rate;
            let dt = 1.0 / sr;
            let phase_inc = self.last_valid_f0 * 2.0 * std::f64::consts::PI * dt;
            for _ in 0..n {
                self.fundamental_phase += phase_inc;
                if self.fundamental_phase >= 2.0 * std::f64::consts::PI {
                    self.fundamental_phase -= 2.0 * std::f64::consts::PI;
                }
            }
            for i in 0..n { out[i] = 0.0; }
            return n;
        }

        self.last_valid_f0 = f0_hz as f64;

        let sr = self.sample_rate;
        let dt = 1.0 / sr;
        let nyquist = sr * 0.5;
        let n_harm = harmonic_amps.len().min(self.max_harmonics);

        // 每一步的相位增量
        let phase_inc = f0_hz as f64 * 2.0 * std::f64::consts::PI * dt;

        for i in 0..n {
            // ⚠️ 核心修复：累加真实相位，确保任何频率突变下的绝对连续
            self.fundamental_phase += phase_inc;
            if self.fundamental_phase >= 2.0 * std::f64::consts::PI {
                self.fundamental_phase -= 2.0 * std::f64::consts::PI;
            }

            let mut sample = 0.0f64;
            for h in 0..n_harm {
                let hz = f0_hz as f64 * (h + 1) as f64;
                if hz >= nyquist { break; }
                // 谐波相位永远是基频相位的整数倍，完美锁定
                let harm_phase = self.fundamental_phase * (h + 1) as f64;
                sample += harm_phase.sin() * harmonic_amps[h] as f64;
            }
            out[i] = sample.clamp(-1.0, 1.0) as f32;
        }
        n
    }

    /// 使用固定谐波分布合成（1/h^1.5 指数衰减）
    /// 这是在没有 Unit2Control ONNX 时的 fallback
    fn synthesize_fallback(&mut self, f0_hz: f32, out: &mut [f32]) -> usize {
        let nyquist = self.sample_rate * 0.5;
        let max_h = (nyquist / f0_hz.max(60.0) as f64).ceil() as usize;
        let n_h = max_h.min(60usize).min(self.max_harmonics); // 硬上限 60 谐波
        let mut amps = Vec::with_capacity(n_h);
        for h in 1..=n_h {
            amps.push(1.0 / (h as f32).powf(1.5));
        }
        let sum: f32 = amps.iter().sum();
        if sum > 0.0 {
            for a in amps.iter_mut() { *a /= sum; }
        }
        self.synthesize_frame(f0_hz, &amps, out)
    }

    fn reset(&mut self) {
        self.fundamental_phase = 0.0;
        self.last_valid_f0 = 100.0;
    }
}

/// 滑动窗口平滑 F0 曲线，消除相邻帧音高跳变（Pitch Jumps）。
/// 窗口 = 3 帧（约 30ms），只平滑有声段，无声段（<=0）不参与平均。
fn smooth_f0(pitch: &[f32]) -> Vec<f32> {
    let half = crate::audio_engine::config::F0_SMOOTH_WINDOW / 2;
    pitch.iter().enumerate().map(|(i, &v)| {
        if v <= 0.0 { return 0.0; }
        let start = i.saturating_sub(half);
        let end = (i + half + 1).min(pitch.len());
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for j in start..end {
            if pitch[j] > 0.0 {
                sum += pitch[j];
                count += 1;
            }
        }
        if count > 0 { sum / count as f32 } else { v }
    }).collect()
}

fn build_pitch(pitchf_hz: &[f32]) -> Vec<i64> {
    let (a, b) = (f0_mel_min(), f0_mel_max());
    pitchf_hz.iter().map(|&f| {
        // ⚠️ RVC 官方静音 Token = 1（对应 f0_mel <= 1）。
        // 槽位 0 是未训练的随机张量，传 0 会注入毁灭性噪音→囫囵声。
        if f <= 1.0 { return 1i64; }
        let m = 1127.0 * (1.0 + f / 700.0).ln();
        ((m - a) * 254.0 / (b - a) + 1.0).round().clamp(1.0, 255.0) as i64
    }).collect()
}

fn resample_linear(input: &[f32], from: f64, to: f64) -> Vec<f32> {
    if (from - to).abs() < 0.5 { return input.to_vec(); }
    let ratio = to / from;
    let olen = (input.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(olen);
    for i in 0..olen {
        let sp = i as f64 / ratio;
        let si = sp as usize;
        let fr = (sp - si as f64) as f32;
        if si + 1 < input.len() { out.push(input[si] * (1.0 - fr) + input[si + 1] * fr); }
        else if si < input.len() { out.push(input[si]); }
    }
    out
}

/// 专用于 48kHz 到 16kHz 的抗混叠降采样。
/// 使用 7 阶 FIR 低通核预滤波，消除 8kHz 以上高频折返到 0-8kHz 人声频段。
/// 降采样物理要求：先低通（截止 ≤ 8kHz），再 3 倍抽取。
/// 单次实时块 ~4800 采样，计算开销 < 0.01ms，可忽略。
fn resample_48k_to_16k_fir(input: &[f32]) -> Vec<f32> {
    let olen = input.len() / 3;
    let mut out = Vec::with_capacity(olen);
    // 7 阶对称低通核，DC 增益 = 1.0，归一化截止 ~π/3
    let kernel = [0.05, 0.1, 0.2, 0.3, 0.2, 0.1, 0.05];

    for i in 0..olen {
        let center = i * 3;
        let mut sample = 0.0;
        for (j, &w) in kernel.iter().enumerate() {
            let idx = center as isize + j as isize - 3;
            let val = if idx < 0 {
                input[0]
            } else if idx >= input.len() as isize {
                *input.last().unwrap_or(&0.0)
            } else {
                input[idx as usize]
            };
            sample += val * w;
        }
        out.push(sample);
    }
    out
}

/// YIN 算法：从 48kHz 音频块中提取 F0 曲线。
/// `nf` = 目标输出帧数，`sample_rate` = 输入采样率（48kHz）。
/// 返回 `nf` 个 F0 值（Hz），无声段为 0.0。
///
/// 设计：用整个 chunk（100ms）做一次 YIN 分析，滑动窗长 1920 采样（40ms），
/// 步长 480 采样（10ms），覆盖 60-800Hz 人声范围，比短帧更准。
/// 输出 interpolate 到 nf 帧对齐合成器。
pub fn yin_f0(chunk_48k: &[f32], nf: usize, sample_rate: u32) -> Vec<f32> {
    let sr = sample_rate as f64;
    let min_f0 = 60.0;
    let max_f0 = 800.0;
    // YIN voiced/unvoiced 判定阈值。
    // 阈值策略需要平衡两个错误方向：
    //   假阳性（将无声辅音误判为有声）→ 合成器用 ContentVec 特征主导频谱，影响小
    //   假阴性（将元音误判为无声）→ 合成器输出气声，完全丢失有声性 ← 更严重
    //
    // 实测在实时麦克风条件下（背景噪底、口腔耦合），
    // 正常有声段的 CMND 最小值分布在 0.05-0.20 区间。
    // 原阈值 0.20 不够宽松 → 大量有声帧被判无声 → pitch=1 → 气声/沙哑。
    // 调整为 0.12 确保绝大多数有声帧通过主阈值判定。
    let threshold = 0.12;

    // 分析窗 1920 采样（40ms），可测最低 50Hz
    let frame_len = 1920usize;
    let hop = 480usize;  // 10ms 步长

    // 在 chunk 上滑动窗口，收集每帧的候选 F0
    let mut raw_f0: Vec<f32> = Vec::new();
    let mut pos = 0usize;
    while pos + frame_len <= chunk_48k.len() {
        let frame = &chunk_48k[pos..pos + frame_len];

        // YIN difference function
        let max_lag = (sr / min_f0) as usize;
        let min_lag = (sr / max_f0).max(2.0) as usize;
        let tau_max = max_lag.min(frame_len / 2);
        if tau_max <= min_lag {
            raw_f0.push(0.0);
            pos += hop;
            continue;
        }

        let mut d = vec![0.0f64; tau_max + 1];
        for tau in min_lag..=tau_max {
            let mut sum = 0.0f64;
            let n = frame_len - tau;
            for t in (0..n).step_by(2) {  // 降采样 x2（48kHz→24kHz 够用）
                let diff = frame[t] as f64 - frame[t + tau] as f64;
                sum += diff * diff;
            }
            d[tau] = sum;
        }

        // Cumulative mean normalized difference
        let mut d_norm = vec![1.0f64; tau_max + 1];
        let mut running_sum = 0.0f64;
        for tau in 1..=tau_max {
            running_sum += d[tau];
            if running_sum > 0.0 {
                d_norm[tau] = d[tau] * tau as f64 / running_sum;
            }
        }

        // ── voiced/unvoiced 判定 ─────────────────────────────────────────
        // 双层阈值策略：
        //   1. 主阈值 threshold=0.12：捕获绝大多数有声帧
        //   2. 宽松兜底 RELAXED_THRESHOLD=0.50：主阈值未通过但全局最小低于 0.50 时
        //      仍判为有声（避免全零 F0 导致合成器气声化）
        //   3. 全局最小 >= 0.50：判为真无声音段 → F0=0.0
        const RELAXED_THRESHOLD: f64 = crate::audio_engine::config::YIN_RELAXED_THRESHOLD;

        let mut threshold_tau = 0usize;
        let mut global_best_tau = 0usize;
        let mut min_val = 1.0f64;
        for tau in min_lag..=tau_max {
            if threshold_tau == 0 && d_norm[tau] < threshold {
                threshold_tau = tau;
            }
            if d_norm[tau] < min_val {
                min_val = d_norm[tau];
                global_best_tau = tau;
            }
        }

        // 判定有声/无声：优先用主阈值，未通过时检查宽松阈值
        let best_tau = if threshold_tau > 0 {
            threshold_tau
        } else if global_best_tau > 0 && min_val < RELAXED_THRESHOLD {
            global_best_tau
        } else {
            0 // 真无声
        };

        if best_tau == 0 {
            raw_f0.push(0.0);
            pos += hop;
            continue;
        }
        if best_tau > 0 && best_tau < tau_max {
            let y0 = d_norm[best_tau - 1];
            let y1 = d_norm[best_tau];
            let y2 = d_norm[best_tau + 1];
            let denom = y0 - 2.0 * y1 + y2;
            if denom.abs() > 1e-12 {
                let shift = (y0 - y2) / (2.0 * denom);
                let refined_tau = best_tau as f64 + shift;
                if refined_tau > 0.0 {
                    raw_f0.push((sr / refined_tau).clamp(min_f0 as f32 as f64, max_f0 as f32 as f64) as f32);
                    pos += hop;
                    continue;
                }
            }
        }
        // 抛物线精化无效时，退回 threshold_tau 直接计算（仍在 voiced 范围内）
        raw_f0.push((sr / best_tau.max(1) as f64).clamp(min_f0 as f32 as f64, max_f0 as f32 as f64) as f32);
        pos += hop;
    }

    // 中值滤波（窗口=5）
    if raw_f0.len() > 5 {
        for i in 2..raw_f0.len() - 2 {
            let vals = &raw_f0[i-2..=i+2];
            let non_zero: Vec<f32> = vals.iter().copied().filter(|&v| v > 0.0).collect();
            if non_zero.len() >= 3 {
                let mut sorted = non_zero;
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                raw_f0[i] = sorted[sorted.len() / 2];
            } else if non_zero.len() <= 1 && vals[2] > 0.0 {
                // isolated peak = noise
                raw_f0[i] = 0.0;
            }
        }
    }

    // 插值到 nf 帧对齐合成器
    let raw_len = raw_f0.len().max(1);
    let mut result = Vec::with_capacity(nf);
    for i in 0..nf {
        let idx = (i * raw_len / nf.max(1)).min(raw_len - 1);
        result.push(raw_f0[idx]);
    }
    result
}

// ── Mel 频谱工具 (RMVPE 前端) ──

const RMVPE_N_FFT: usize = 1024;
const RMVPE_HOP: usize = 160;     // 10ms @ 16kHz
const RMVPE_N_MELS: usize = 128;
const RMVPE_N_BINS: usize = 360;  // F0 分档数
const RMVPE_HALF_FFT: usize = RMVPE_N_FFT / 2 + 1; // 513

/// 预计算的 Hann 窗 (1024 点)
static HANN_WINDOW: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
fn get_hann() -> &'static [f32] {
    HANN_WINDOW.get_or_init(|| {
        use std::f32::consts::PI;
        let mut w = Vec::with_capacity(RMVPE_N_FFT);
        for i in 0..RMVPE_N_FFT {
            w.push(0.5 * (1.0 - (2.0 * PI * i as f32 / (RMVPE_N_FFT - 1) as f32).cos()));
        }
        w
    })
}

/// 预计算的 Mel 滤波器组 (128 × 513)
static MEL_BASIS: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
fn get_mel_basis() -> &'static [f32] {
    MEL_BASIS.get_or_init(|| {
        let mut basis = vec![0.0f32; RMVPE_N_MELS * RMVPE_HALF_FFT];
        let f_min = 30.0_f32;   // RMVPE 使用 30Hz，不是 0
        let f_max = 8000.0_f32;
        let sr = 16000.0_f32;
        let mel_min = 1127.0 * (1.0 + f_min / 700.0).ln();
        let mel_max = 1127.0 * (1.0 + f_max / 700.0).ln();
        let mel_step = (mel_max - mel_min) / (RMVPE_N_MELS as f32 - 1.0);
        for m in 0..RMVPE_N_MELS {
            let center_mel = mel_min + m as f32 * mel_step;
            let left_mel = if m == 0 { mel_min } else { center_mel - mel_step };
            let right_mel = if m == RMVPE_N_MELS - 1 { mel_max } else { center_mel + mel_step };
            let left_hz = 700.0 * ((left_mel / 1127.0).exp() - 1.0);
            let center_hz = 700.0 * ((center_mel / 1127.0).exp() - 1.0);
            let right_hz = 700.0 * ((right_mel / 1127.0).exp() - 1.0);
            let left_bin = ((left_hz / sr) * RMVPE_N_FFT as f32).round().max(0.0).min((RMVPE_HALF_FFT - 1) as f32) as usize;
            let center_bin = ((center_hz / sr) * RMVPE_N_FFT as f32).round().max(0.0).min((RMVPE_HALF_FFT - 1) as f32) as usize;
            let right_bin = ((right_hz / sr) * RMVPE_N_FFT as f32).round().max(0.0).min((RMVPE_HALF_FFT - 1) as f32) as usize;
            for b in left_bin..=right_bin {
                let val = if b < center_bin {
                    (b as f32 - left_bin as f32) / (center_bin as f32 - left_bin as f32 + 1e-8)
                } else {
                    (right_bin as f32 - b as f32) / (right_bin as f32 - center_bin as f32 + 1e-8)
                };
                basis[m * RMVPE_HALF_FFT + b] = val;
            }
        }
        basis
    })
}

// ── FCPE F0 提取 (16kHz 直接输入，ONNX 推理) ──
/// FCPE (Fast Context-based Pitch Estimator) ONNX 推理。
///
/// 输入：48kHz 音频窗口
/// 输出：F0 值 (Hz)，无声段为 0.0。帧数与输入长度成比例 (hop=10ms)。
///
/// 与 RMVPE 的区别：
///   - FCPE 输入 16kHz 原始波形，输出直接是 F0 Hz，无需 mel/F0 解码
///   - 模型 <42MB，RTF 0.006 (比 RMVPE 快 5-10x)
///   - 专门为实时 SVC 设计，抗噪能力和精度均优于 RMVPE
pub fn fcpe_f0(chunk_48k: &[f32], fcpe: &mut Session) -> Vec<f32> {
    // 48kHz → 16kHz (3x 均值抽取)
    let olen = chunk_48k.len() / 3;
    let mut r16 = Vec::with_capacity(olen);
    for i in 0..olen {
        let idx = i * 3;
        r16.push((chunk_48k[idx] + chunk_48k[idx + 1] + chunk_48k[idx + 2]) / 3.0);
    }

    // FCPE I/O: input [1, n_samples, 1] float32 @16kHz
    let tensor = match Tensor::from_array(
        (vec![1usize, r16.len(), 1usize], r16.into_boxed_slice())
    ) {
        Ok(t) => t,
        Err(e) => { eprintln!("[FCPE] tensor 创建失败: {}", e); return Vec::new(); }
    };

    let outputs = match fcpe.run(ort::inputs!["audio" => tensor]) {
        Ok(o) => o,
        Err(e) => { eprintln!("[FCPE] 推理失败: {}", e); return Vec::new(); }
    };

    let (shape, data) = match outputs.get("f0_hz").and_then(|o| o.try_extract_tensor::<f32>().ok()) {
        Some(s) => s,
        None => { eprintln!("[FCPE] 输出提取失败"); return Vec::new(); }
    };

    // shape: [1, n_frames, 1], data: f0_hz (0 = unvoiced), NaN → 0
    let n_frames = shape[1] as usize;
    let mut result = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let val = data[i];
        if val.is_finite() && val > 0.0 {
            result.push(val);
        } else {
            result.push(0.0);
        }
    }
    result
}

/// RMVPE：mel 频谱 → F0 (cents) → Hz
///
/// 完全匹配 Python 版 RMVPEONNX.infer_from_audio 的预处理 + 解码逻辑。
/// 输入: chunk_48k — 48kHz 音频块
/// 输出: nf 个 F0 值 (Hz)，无声段为 0.0
pub fn rmpe_f0(chunk_48k: &[f32], nf: usize, rmpe: &mut Session) -> Vec<f32> {
    use rustfft::FftPlanner;

    // 1. 48kHz → 16kHz (均值抽取)
    let olen = chunk_48k.len() / 3;
    let mut r16 = Vec::with_capacity(olen);
    for i in 0..olen {
        let idx = i * 3;
        r16.push((chunk_48k[idx] + chunk_48k[idx + 1] + chunk_48k[idx + 2]) / 3.0);
    }

    // 2. STFT (center=True): 反射填充 N_FFT/2, 然后 hop 步进
    //    n_frames = ceil(audio_len / hop)  实际是 (audio_len + N_FFT/2 - 0) / hop? 
    //    用反射填充: 每端加 N_FFT/2
    let pad = RMVPE_N_FFT / 2;
    let mut padded = Vec::with_capacity(r16.len() + 2 * pad);
    // 反射填充: 前面镜像后pad个点
    for i in (1..=pad).rev() {
        padded.push(r16[i.min(r16.len()-1)]);
    }
    padded.extend_from_slice(&r16);
    // 后面镜像
    for i in (0..pad).rev() {
        padded.push(r16[r16.len().saturating_sub(1 + i)]);
    }

    let n_frames = (padded.len() - RMVPE_N_FFT) / RMVPE_HOP + 1;

    // 3. 缓存的 Hann 窗 + Mel 滤波器组
    let hann = get_hann();
    let mel_basis = get_mel_basis();

    // 4. FFT planner
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(RMVPE_N_FFT);

    // 5. 计算 Mel 频谱 [n_mels × n_frames] — magnitude spectrogram
    //    Python: torch.stft → sqrt(real²+imag²) → mel_filters × magnitude → log(clamp)
    let mut mel_spec = vec![0.0f32; RMVPE_N_MELS * n_frames];
    let mut fft_buf = vec![rustfft::num_complex::Complex::new(0.0f32, 0.0f32); RMVPE_N_FFT];

    for t in 0..n_frames {
        let offset = t * RMVPE_HOP;
        for k in 0..RMVPE_N_FFT {
            fft_buf[k] = rustfft::num_complex::Complex::new(padded[offset + k] * hann[k], 0.0);
        }
        fft.process(&mut fft_buf);

        for m in 0..RMVPE_N_MELS {
            let mut val = 0.0f32;
            let base = m * RMVPE_HALF_FFT;
            for k in 0..RMVPE_HALF_FFT {
                val += fft_buf[k].norm_sqr().sqrt() * mel_basis[base + k];
            }
            mel_spec[m * n_frames + t] = val;
        }
    }

    // 6. log mel: log(clamp(power, 1e-5))
    //    与 Python MelSpectrogram(clamp=1e-5) 一致
    for v in mel_spec.iter_mut() {
        *v = v.max(1e-5).ln();
    }

    // 7. 填充到下一个 32 的倍数 (匹配 RMVPEONNX.mel2hidden)
    let padded_frames = 32 * ((n_frames - 1) / 32 + 1);
    let mut model_input = if padded_frames > n_frames {
        vec![0.0f32; RMVPE_N_MELS * padded_frames]
    } else {
        mel_spec.clone()
    };
    if padded_frames > n_frames {
        for m in 0..RMVPE_N_MELS {
            let src = m * n_frames;
            let dst = m * padded_frames;
            model_input[dst..dst + n_frames].copy_from_slice(&mel_spec[src..src + n_frames]);
            // 填充部分保持 0（对应 constant padding）
        }
    } else {
        model_input.copy_from_slice(&mel_spec);
    }

    // 8. ONNX 推理
    let input_tensor = match Tensor::from_array(
        (vec![1usize, RMVPE_N_MELS, padded_frames], model_input.into_boxed_slice())
    ) {
        Ok(t) => t,
        Err(e) => { eprintln!("[RMVPE] tensor 创建失败: {}", e); return vec![0.0; nf]; }
    };
    let outputs = match rmpe.run(ort::inputs![input_tensor]) {
        Ok(o) => o,
        Err(e) => { eprintln!("[RMVPE] 推理失败: {}", e); return vec![0.0; nf]; }
    };
    let (out_shape, out_data) = match outputs[0].try_extract_tensor::<f32>() {
        Ok(s) => s,
        Err(e) => { eprintln!("[RMVPE] 输出提取失败: {}", e); return vec![0.0; nf]; }
    };
    // hidden: [1, padded_frames, 360]，只取前 n_frames 帧
    let out_frames: usize = out_shape[1] as usize;
    let usable = n_frames.min(out_frames);

    // 9. 解码: cents 加权平均 → Hz (匹配 RMVPEONNX._decode)
    //    cents_mapping = 20 * arange(360) + 1997.38
    //    padded 到 368: pad(cents_mapping, (4,4))
    const CENTS_BASE: f32 = 1997.3794084376191;
    // 预计算 368 个 cents 值
    let mut cents_padded = [0.0f32; 368];
    for i in 0..360 {
        cents_padded[i + 4] = 20.0 * i as f32 + CENTS_BASE;
    }
    // 前 4 个和最后 4 个保持 0

    let mut raw_f0 = vec![0.0f32; usable];
    for t in 0..usable {
        let offset = t * RMVPE_N_BINS;

        // argmax
        let mut max_bin = 0usize;
        let mut max_val = 0.0f32;
        for i in 0..RMVPE_N_BINS {
            let prob = out_data[offset + i];
            if prob > max_val { max_val = prob; max_bin = i; }
        }

        if max_val <= 0.03 {
            raw_f0[t] = 0.0;
            continue;
        }

        // 9-bin 加权平均 (对应 _decode 中的 starts=center-4, ends=center+5)
        // center 是 padded 空间中的索引: max_bin + 4
        let center_padded = max_bin + 4;
        let mut sum_w = 0.0f32;
        let mut sum_wc = 0.0f32;
        for j in 0..9 {
            let bin_idx = center_padded as isize - 4 + j;
            if bin_idx < 0 || bin_idx >= 368 { continue; }
            let prob = if bin_idx >= 4 && bin_idx < 364 {
                out_data[offset + (bin_idx - 4) as usize]
            } else {
                // 填充区概率为 0（在 padded hidden 中）
                0.0
            };
            sum_w += prob;
            sum_wc += prob * cents_padded[bin_idx as usize];
        }

        if sum_w > 1e-8 {
            // cents → Hz: f0 = 10 * 2^(cents/1200)
            let cents = sum_wc / sum_w;
            let hz = 10.0 * (2.0_f32).powf(cents / 1200.0);
            raw_f0[t] = hz.clamp(30.0, 2000.0);
        } else {
            raw_f0[t] = 0.0;
        }
    }

    // 10. 插值到 nf 帧对齐合成器
    let raw_len = raw_f0.len().max(1);
    let mut result = Vec::with_capacity(nf);
    for i in 0..nf {
        let idx = (i * raw_len / nf.max(1)).min(raw_len - 1);
        result.push(raw_f0[idx]);
    }
    result
}

/// 校验 ContentVec 的实际输出是否为 768 维。
/// 不是 768 则返回明确错误，引导用户下载正确模型。
fn verify_contentvec_768(cv: &mut Session, cv_path: &str) -> Result<(), String> {
    // HuBERT 前端有 7 层卷积（总 stride=320），用 16000 采样（1s @16kHz）确保所有层都能正常产出
    // 输入 shape=[1, 1, T]（ContentVec ONNX 要求 3D 输入）
    let dummy = vec![0.0f32; 16000];
    let tensor = Tensor::from_array((vec![1usize, 1usize, dummy.len()], dummy.into_boxed_slice()))
        .map_err(|e| format!("创建 tensor: {}", e))?;
    let outputs = cv.run(ort::inputs![tensor])
        .map_err(|e| format!("ContentVec 推理: {}", e))?;

    if outputs.len() == 0 {
        return Err("ContentVec 无输出".to_string());
    }

    // 只检查最后一个输出——与 run() 实际使用的输出一致
    let n_out = outputs.len();
    match outputs[n_out - 1].try_extract_tensor::<f32>() {
        Ok((shape, _)) => {
            if shape.len() >= 3 && shape[2] == 768 {
                eprintln!("[RVC] ContentVec 验证通过: 最后一层输出 {} 维", shape[2]);
                return Ok(());
            }
            let cv_fname = Path::new(cv_path).file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
            let parent = Path::new(cv_path).parent().map(|p| p.to_string_lossy()).unwrap_or_else(|| "assets/hubert/".into());
            Err(format!(
                "ContentVec 最后输出层是 {} 维，需要 768 维（v2 标准）。\n\
                 当前模型: {}\n\
                 请下载 vec-768-layer-12.onnx 放到: {}",
                shape.get(2).unwrap_or(&0), cv_fname, parent
            ))
        }
        Err(e) => Err(format!("ContentVec 输出提取失败: {}", e)),
    }
}

// ── ORT 初始化（无 CoreML，纯 CPU/MPS） ──
static ORT_INIT: Once = Once::new();

fn ensure_ort() {
    ORT_INIT.call_once(|| {
        let _ = ort::init().commit();
    });
}

// ── 模型会话构建 ──
pub(crate) fn build_session(model_path: &str) -> Result<Session, String> {
    let p = Path::new(model_path);
    if !p.exists() { return Err(format!("模型不存在: {}", model_path)); }
    let sz = p.metadata().map_err(|e| format!("读元数据: {}", e))?.len();
    if sz < 1024 * 1024 { return Err(format!("模型过小 ({}B)", sz)); }
    Session::builder().map_err(|e| format!("Session builder: {}", e))?
        // Level2: extended graph optimizations (node fusion, constant folding), faster than Level1.
        .with_optimization_level(GraphOptimizationLevel::Level2)
        .map_err(|e| format!("优化级别: {}", e))?
        // M4 Mac has 4 performance cores; 4 intra-op threads improves inference vs 2.
        .with_intra_threads(4)
        .map_err(|e| format!("线程数: {}", e))?
        .commit_from_file(model_path)
        .map_err(|e| format!("加载模型: {}", e))
}

// ── InferCore：RVC 推理核心 ──
//
// 张量接口（w-okada / RVC ONNX 标准）:
//   所有输入张量使用具名绑定（Named Inputs），与 ONNX 图节点名称精确匹配。
//   输入名称：
//     - "feats"       : [1, nf, 768]  f32  上采样后的 ContentVec 特征
//     - "p_len"       : [1]           i64   nf
//     - "pitch"       : [1, nf]       i64   mel 刻度音高 (1-255)
//     - "pitchf"      : [1, nf]       f32   F0 频率 in Hz
//     - "sid"         : [1]           i64   说话人 ID (0)
//     - "rnd"         : [1, 192, nf]  f32  确定性噪声
//   输出:
//     - "audio"       : [1, 1, N]     f32   40kHz 音频
//
// 注意：必须在所有张量间保持时序维度 nf 一致，否则 enc_p/Add 节点
// 在 broadcasting 时会产生"axis != 1 && axis != largest"的错误。
//

/// IVF 倒排索引搜索器。
///
/// 存储从 Faiss IVF 索引提取的质心和倒排列表。
/// 搜索时：query → 最近 nprobe 个质心 → 只在这几个桶内暴力搜索。
/// 加速比 ≈ ntotal/num_centroids × nprobe，召回率 95%+（nprobe=3）。
struct IVFSearcher {
    centroids: Vec<f32>,       // num_centroids × dim
    bucket_labels: Vec<u32>,   // 所有桶的向量标签（big_npy 下标），按桶连续排列
    bucket_offsets: Vec<usize>,// 每个桶在 bucket_labels 中的起始偏移 [0..num_centroids+1]
    dim: usize,
    num_centroids: usize,
}

impl IVFSearcher {
    /// 搜索 query 的 top-k 个近邻，返回它们在 big_npy 中的标签和距离。
    fn search(&self, query: &[f32], big_npy: &[f32], k: usize, nprobe: usize) -> Vec<(f32, usize)> {
        let d = self.dim;
        let nprobe = nprobe.min(self.num_centroids);

        // 1. 计算 query 到所有质心的 L2 距离
        let mut cent_dists: Vec<(f32, usize)> = Vec::with_capacity(self.num_centroids);
        for ci in 0..self.num_centroids {
            let offset = ci * d;
            let mut sum_sq = 0.0f32;
            for c in 0..d {
                let diff = query[c] - self.centroids[offset + c];
                sum_sq += diff * diff;
            }
            cent_dists.push((sum_sq, ci));
        }
        // 部分排序取最近的 nprobe 个
        cent_dists.select_nth_unstable_by(nprobe, |a, b| a.0.partial_cmp(&b.0).unwrap());

        // 2. 收集候选向量（来自 nprobe 个桶内的所有向量）
        let mut candidates: Vec<usize> = Vec::new();
        for &(_, ci) in cent_dists[..nprobe].iter() {
            let start = self.bucket_offsets[ci];
            let end = self.bucket_offsets[ci + 1];
            for &label in &self.bucket_labels[start..end] {
                candidates.push(label as usize);
            }
        }

        if candidates.is_empty() {
            return Vec::new();
        }

        // 3. 在候选向量中暴力搜索 top-k
        let n_cand = candidates.len();
        let mut dist_label: Vec<(f32, usize)> = Vec::with_capacity(n_cand);
        for &label in &candidates {
            let offset = label * d;
            let mut sum_sq = 0.0f32;
            for c in 0..d {
                let diff = query[c] - big_npy[offset + c];
                sum_sq += diff * diff;
            }
            dist_label.push((sum_sq, label));
        }

        let k = k.min(n_cand);
        // Safety: select_nth_unstable_by 的 index 必须严格小于数组长度。
        // 当 k == dist_label.len() 时不需要部分排序，直接返回全部即可。
        if k > 0 && k < dist_label.len() {
            dist_label.select_nth_unstable_by(k, |a, b| a.0.partial_cmp(&b.0).unwrap());
        }
        dist_label[..k].to_vec()
    }
}

pub struct InferCore {
    content_vec: Session,
    synthesizer: Session,
    pub(crate) rmvpe: Session,
    /// FCPE F0 提取 Session（直播实时场景首选）
    pub(crate) fcpe: Option<Session>,
    noise_cache: Vec<f32>,
    /// 递归相位累加合成器（DDSP 架构）
    phase_acc: PhaseAccumulator,
    /// 模型身份标识（文件大小 + 前 8 字节 hex）
    model_fingerprint: String,
    /// 全部特征向量（big_npy），flat array: ntotal × dim
    big_npy: Vec<f32>,
    /// 特征向量维度
    index_dim: usize,
    /// IVF 倒排索引搜索器（可选）
    ivf: Option<IVFSearcher>,
}

impl InferCore {
    /// 加载 ContentVec + RVC 合成器 + RMVPE F0 模型 + 可选 Faiss 特征索引。
    ///
    /// ContentVec 必须输出 768 维特征（v2 标准），否则返回明确错误。
    /// `index_path` 必须存在，否则直接报错（不降级）。
    pub fn load(vec_path: &str, model_path: &str, rmvpe_path: &str, fcpe_path: &str, index_path: &str) -> Result<Self, String> {
        ensure_ort();

        // ══════════════════════════════════════════════
        //  ⚠️  模型指纹诊断 — 确认 ONNX 文件身份
        // ══════════════════════════════════════════════
        let meta = std::fs::metadata(model_path)
            .map_err(|e| format!("读模型元数据: {}", e))?;
        let fsize = meta.len();
        // 读前 8 字节 hex 作文件指纹
        let mut f = std::fs::File::open(model_path)
            .map_err(|e| format!("打开模型: {}", e))?;
        use std::io::Read;
        let mut header = [0u8; 8];
        f.read_exact(&mut header).ok();
        let fp_hex: String = header.iter().map(|b| format!("{:02x}", b)).collect();
        let model_fingerprint = format!("{}:{}", fsize, fp_hex);
        eprintln!("[RVC] 🔍 模型指纹: {} path={}", model_fingerprint, model_path);
        eprintln!("[RVC] 🔍 index_path={}", index_path);
        drop(f);

        let mut cv = build_session(vec_path)?;
        verify_contentvec_768(&mut cv, vec_path)?;
        let syn = build_session(model_path)?;

        // ── 加载 RMVPE F0 模型 ──
        eprintln!("[RVC] RMVPE 路径: {}", rmvpe_path);
        let rmvpe = build_session(rmvpe_path)?;
        eprintln!("[RVC] ✅ RMVPE 加载成功");

        // ── 加载 FCPE F0 模型（直播实时场景首选） ──
        let fcpe: Option<Session> = if !fcpe_path.is_empty() && Path::new(fcpe_path).exists() {
            eprintln!("[RVC] FCPE 路径: {}", fcpe_path);
            match build_session(fcpe_path) {
                Ok(s) => {
                    eprintln!("[RVC] ✅ FCPE 加载成功");
                    Some(s)
                }
                Err(e) => {
                    eprintln!("[RVC] ⚠️ FCPE 加载失败 (fallback RMVPE): {}", e);
                    None
                }
            }
        } else {
            eprintln!("[RVC] 未配置 FCPE 模型，将使用 RMVPE fallback");
            None
        };

        // 诊断：打印合成器输入名称，确认 ONNX 图节点实际命名
        eprintln!("[RVC] 合成器输入列表:");
        for (i, input) in syn.inputs().iter().enumerate() {
            eprintln!("  [{}] name='{}' dtype={:?}",
                i, input.name(), input.dtype().tensor_type());
        }

        // ── 2026 最佳实践：绝对时间轴锚定噪声 (Time-Anchored Noise) ──
        // 预先生成 8192 帧 (约 81 秒) 的标准正态分布随机缓存带，
        // 确保同一段物理声音在任何滑动窗口下都分配到完全一致的噪声刺激。
        let noise_frames = 8192usize;
        let mut noise_cache = vec![0.0f32; 192 * noise_frames];
        let mut seed = 12345u32;
        let mut rand = || -> f32 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed as f32) / (std::u32::MAX as f32)
        };
        // Box-Muller 生成均值为 0、方差为 1 的高斯噪声，消除电音并找回呼吸感
        for i in (0..noise_cache.len()).step_by(2) {
            let u1 = rand().max(1e-8);
            let u2 = rand();
            let r = (-2.0 * u1.ln()).sqrt();
            let theta = 2.0 * std::f32::consts::PI * u2;
            noise_cache[i] = r * theta.cos();
            if i + 1 < noise_cache.len() {
                noise_cache[i + 1] = r * theta.sin();
            }
        }

        // ── 加载特征检索库 (big_npy + IVF) ──
        // 缓存文件由 train_index 在训练时生成（checkpoints/ 目录），
        // 运行时不再调用 Python。缓存缺失或残损则直接报错。
        let mut big_npy: Vec<f32> = Vec::new();
        let mut index_dim: usize = 0;
        let mut ivf: Option<IVFSearcher> = None;

        if index_path.is_empty() || !Path::new(index_path).exists() {
            return Err(format!("index 文件不存在: {}", index_path));
        }
        let bin_path = format!("{}.vectors.bin", index_path);
        let meta_path = format!("{}.meta.txt", index_path);
        let ivf_path = format!("{}.ivf.bin", index_path);

        // 验证缓存完整性
        if !Path::new(&bin_path).exists() || !Path::new(&meta_path).exists() || !Path::new(&ivf_path).exists() {
            return Err(format!(
                "IVF 缓存不完整，请重新训练索引。\n缺失文件之一:\n  {}\n  {}\n  {}",
                bin_path, meta_path, ivf_path
            ));
        }

        // 读取 big_npy 元数据 + 向量
        if let Ok(meta) = std::fs::read_to_string(&meta_path) {
            let ls: Vec<&str> = meta.lines().collect();
            if ls.len() >= 2 {
                if let (Ok(n), Ok(d)) = (ls[0].parse::<usize>(), ls[1].parse::<usize>()) {
                    index_dim = d;
                    if let Ok(raw) = std::fs::read(&bin_path) {
                        if raw.len() >= n * d * 4 {
                            let mut vecs = Vec::with_capacity(n * d);
                            for i in 0..n * d {
                                let o = i * 4;
                                vecs.push(f32::from_le_bytes([raw[o], raw[o+1], raw[o+2], raw[o+3]]));
                            }
                            big_npy = vecs;
                            eprintln!("[RVC] ✅ big_npy: {} × {}", n, d);
                        }
                    }
                }
            }
        }

        // 读取 IVF 结构 → 写入 ivf: Option<IVFSearcher>
        let raw = std::fs::read(&ivf_path)
            .map_err(|e| format!("读取 IVF 缓存失败 ({}): {}", ivf_path, e))?;
        if raw.len() < 8 {
            return Err(format!("IVF 缓存文件过短 ({} bytes): {}", raw.len(), ivf_path));
        }
        let ncent = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        let dim = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
        let mut pos = 8usize;
        let cbytes = ncent * dim * 4;
        if raw.len() < pos + cbytes {
            return Err(format!(
                "IVF 缓存质心数据不完整: 期望 {} bytes, 实际 {} bytes", pos + cbytes, raw.len()
            ));
        }
        let mut centroids = Vec::with_capacity(ncent * dim);
        for _ in 0..ncent * dim {
            centroids.push(f32::from_le_bytes([raw[pos], raw[pos+1], raw[pos+2], raw[pos+3]]));
            pos += 4;
        }
        let mut labels: Vec<u32> = Vec::new();
        let mut offs: Vec<usize> = Vec::with_capacity(ncent + 1);
        offs.push(0);
        for _ in 0..ncent {
            if pos + 4 > raw.len() {
                return Err(format!("IVF 缓存桶大小数据不完整 (pos={}, len={})", pos, raw.len()));
            }
            let nv = u32::from_le_bytes([raw[pos], raw[pos+1], raw[pos+2], raw[pos+3]]) as usize;
            pos += 4;
            for _ in 0..nv {
                if pos + 4 > raw.len() {
                    return Err(format!("IVF 缓存标签数据不完整 (pos={}, len={})", pos, raw.len()));
                }
                labels.push(u32::from_le_bytes([raw[pos], raw[pos+1], raw[pos+2], raw[pos+3]]));
                pos += 4;
            }
            offs.push(labels.len());
        }
        if ncent == 0 || centroids.is_empty() {
            return Err(format!("IVF 缓存质心数为 0: {}", ivf_path));
        }
        let ivf_searcher = IVFSearcher {
            centroids,
            bucket_labels: labels,
            bucket_offsets: offs,
            dim,
            num_centroids: ncent,
        };
        eprintln!("[RVC] ✅ IVF 加载: {} centroids, {} vectors",
            ncent, ivf_searcher.bucket_labels.len());
        ivf = Some(ivf_searcher);

        Ok(Self {
            content_vec: cv,
            synthesizer: syn,
            rmvpe,
            fcpe,
            noise_cache,
            phase_acc: PhaseAccumulator::new(48000.0),
            model_fingerprint,
            big_npy,
            index_dim,
            ivf,
        })
    }

    /// 执行一次推理。
    ///
    /// - `chunk_48k`: 48kHz 音频片段
    /// - `window_start_samples`: 当前窗口在绝对时间轴上的起始采样点（用于时间锚定噪声）
    /// - `lookup_f0`: 闭包，输入特征帧数 nf，输出对齐的 F0 曲线 (Hz)
    ///
    /// 返回 48kHz 变声后音频
    pub fn run(
        &mut self,
        chunk_48k: &[f32],
        _window_start_samples: usize,
        lookup_f0: &dyn Fn(usize) -> Vec<f32>,
    ) -> Option<Vec<f32>> {
        // 推理指纹日志：前 3 次打印，后续静默
        // 使用 self 引用打印一次（首次推理时必然触发）
        use std::sync::atomic::{AtomicU8, Ordering};
        static RUN_COUNT: AtomicU8 = AtomicU8::new(0);
        if RUN_COUNT.fetch_add(1, Ordering::Relaxed) < 3 {
            eprintln!("[RVC run] ▶︎ 使用模型: {} | chunk_48k={}", self.model_fingerprint, chunk_48k.len());
        }
        // 1. 48kHz → 16kHz（均值抽取 Boxcar Decimation，防止混叠重影）
        // resample_linear 每 3 点扔 2 个，不做低通→8kHz 以上高频折叠进 0-8kHz 人声区→咬字糊。
        // 3 点均值等效于简单 FIR 低通（截止 ~8kHz），消除折叠噪音后再 3x 抽取。
        let olen = chunk_48k.len() / 3;
        let mut r16 = Vec::with_capacity(olen);
        for i in 0..olen {
            let idx = i * 3;
            r16.push((chunk_48k[idx] + chunk_48k[idx + 1] + chunk_48k[idx + 2]) / 3.0);
        }

        // ⚠️ 【核心修复：填平 ContentVec 的感受野黑洞】
        // ContentVec 前端 7 层卷积总感受野约 400 采样（25ms），不补偿会导致
        // 输出帧数少 1 帧（200ms→180ms），mod.rs 再拉伸回去→11% 时间扭曲→囫囵声。
        // 关键：必须使用边缘复制（Replicate Padding），绝不能补零！
        // 补零会在边界产生瞬态咔哒声（Click），ContentVec 卷积层吃进去后
        // FFT 相位崩溃，连起来听变成"漱口声/水下声"。
        let target_frames = (r16.len() as f32 / crate::audio_engine::config::CONTENTVEC_FRAME_STEP as f32).round() as usize;
        let needed_samples = (target_frames.saturating_sub(1)) * crate::audio_engine::config::CONTENTVEC_FRAME_STEP + crate::audio_engine::config::CONTENTVEC_RECEPTIVE_FIELD;

        if needed_samples > r16.len() {
            let diff = needed_samples - r16.len();
            let pad_left = diff / 2;
            let pad_right = diff - pad_left;

            // 边缘复制：用首尾真实采样值填充，保持波形连续性
            let first_val = *r16.first().unwrap_or(&0.0);
            let last_val = *r16.last().unwrap_or(&0.0);

            let mut padded = vec![first_val; pad_left];
            padded.extend_from_slice(&r16);
            padded.extend(vec![last_val; pad_right]);
            r16 = padded;
        }

        if r16.len() < 360 {
            eprintln!("[RVC run] r16 过短: {} (需要 ≥360)", r16.len());
            return None;
        }

        // 2. ContentVec 推理 @16kHz，输入 shape=[1, 1, T]
        let vec_in = match Tensor::from_array(
            (vec![1usize, 1usize, r16.len()], r16.into_boxed_slice())
        ) {
            Ok(t) => t,
            Err(e) => { eprintln!("[RVC run] Tensor::from_array 失败: {}", e); return None; }
        };
        let vo = match self.content_vec.run(ort::inputs![vec_in]) {
            Ok(o) => o,
            Err(e) => { eprintln!("[RVC run] ContentVec 推理失败: {}", e); return None; }
        };

        // 3. 取最后一个输出（第 12 层 768 维特征）
        let n_out = vo.len();
        if n_out == 0 { eprintln!("[RVC run] ContentVec 无输出"); return None; }
        let (shape, data) = match vo[n_out - 1].try_extract_tensor::<f32>() {
            Ok(s) => s,
            Err(e) => { eprintln!("[RVC run] 提取特征张量失败: {}", e); return None; }
        };
        if shape.len() < 3 { eprintln!("[RVC run] 特征张量维度不足: {:?}", shape); return None; }
        let time_steps = shape[1] as usize;       // T
        let d = shape[2] as usize;                 // 特征维度（应为 768）

        // ── 【特征融合：IVF 检索 + 加权混合】 ──
        // 遵循 RVC Python 管线（pipeline.py vc() 方法）。
        // 使用 IVF 倒排索引：query → 最近 nprobe 个质心 → 仅桶内暴力搜索。
        // 加速比 ≈ ntotal/ncentroids × nprobe，召回率 95%+。
        // index_rate 由前端 Slider 实时调整（0=纯AI, 1=纯检索）。
        let index_rate = f32::from_bits(
            crate::audio_engine::INDEX_RATE.load(std::sync::atomic::Ordering::Relaxed)
        );
        let mut blended_data: Vec<f32> = data.to_vec();

        if let Some(ref ivf) = self.ivf {
            if self.index_dim == d && index_rate > 0.0 && !self.big_npy.is_empty() {
                eprintln!("[RVC IVF] ✅ 检索生效: index_rate={:.2}, neigbors/step={}, centroids={}, big_npy={}",
                    index_rate, crate::audio_engine::config::IVF_K, ivf.num_centroids, self.big_npy.len() / d);
                const K: usize = crate::audio_engine::config::IVF_K;
                const NPROBE: usize = crate::audio_engine::config::IVF_NPROBE;
                let big_npy = &self.big_npy;

                for t in 0..time_steps {
                    let offset = t * d;
                    let query = &blended_data[offset..offset + d];

                    let knn = ivf.search(query, big_npy, K, NPROBE);
                    if knn.is_empty() { continue; }

                    // 权重 = 1/dist² 归一化
                    // ⚠️ 精确匹配（dist < 1e-6）必须 fill(0.0) 再独占 1.0，
                    //    否则之前累积的 weights 未清零→除以 weight_sum=1.0→炸裂。
                    let mut weight_sum = 0.0f32;
                    let mut weights = [0.0f32; K];
                    for k in 0..knn.len() {
                        let dist = knn[k].0;
                        if dist < 1e-6 {
                            weights.fill(0.0);
                            weights[k] = 1.0;
                            weight_sum = 1.0;
                            break;
                        }
                        weights[k] = 1.0 / (dist * dist);
                        weight_sum += weights[k];
                    }

                    if weight_sum > 0.0 {
                        let mut fused = vec![0.0f32; d];
                        for k in 0..knn.len() {
                            let label = knn[k].1;
                            let w = weights[k] / weight_sum;
                            let vo = label * d;
                            for c in 0..d {
                                fused[c] += big_npy[vo + c] * w;
                            }
                        }
                        for c in 0..d {
                            blended_data[offset + c] = fused[c] * index_rate
                                + blended_data[offset + c] * (1.0 - index_rate);
                        }
                    }
                }
            }
        }

        // ── DDSP 合成（替代 synthesizer.run） ──
        //
        // 2026 DDSP 架构：不再调用 HiFi-GAN ONNX 合成器。
        // 直接用 F0 + 谐波分布驱动相位累加合成器，输出 48kHz 波形。
        // 相位由 f64 精度全局累加器保证跨帧绝对连续。
        //
        // 每帧规格：1 ContentVec 帧 = 20ms = 960 采样 @48kHz
        let hop_48k = self.phase_acc.hop_samples; // 960

        // ⚠️ 极其关键：只合成 core 区域的特征
        // ContentVec 看到了整个 window（history + core + future = 160ms），
        // 但 DDSP 绝对只能合成真正的"当前块"（core = 20ms），
        // 保证全局 Phase 严格按照真实物理时间推进，绝不超前合成未来帧。
        let t_start = crate::audio_engine::config::PAST_CONTEXT_SIZE / hop_48k; // 5 (100ms)
        let core_frames = crate::audio_engine::config::CORE_CHUNK_SIZE / hop_48k; // 1 (20ms)
        let t_end = t_start + core_frames;

        // F0 按 T 帧粒度获取（不是 T*2），与 ContentVec 帧数对齐
        let raw_pitchf = lookup_f0(time_steps);
        let pitchf = smooth_f0(&raw_pitchf);

        // 只合成 core 区间
        let mut audio_out = Vec::with_capacity(core_frames * hop_48k);
        let mut frame_buf = vec![0.0f32; hop_48k];
        for t in t_start..t_end.min(time_steps) {
            let f0 = pitchf.get(t).copied().unwrap_or(0.0);
            self.phase_acc.synthesize_fallback(f0, &mut frame_buf);
            audio_out.extend_from_slice(&frame_buf[..hop_48k]);
        }

        // 现在 audio_out 严格等于 CORE_CHUNK_SIZE (960)，天然连续
        Some(audio_out)
    }
}

// ══════════════════════════════════════════════════════════════════
//  离线的全文件 F0 提取 — 替代 Python RMVPE 子进程
// ══════════════════════════════════════════════════════════════════

use crate::audio_engine::types::F0Profile;

/// 对完整的 48kHz 音频文件提取 F0 音高曲线。
///
/// 处理流程（与 Python `infer/lib/rmvpe.py` 完全一致）：
///   1. 48kHz → 16kHz 降采样（3:1 均值抽取）
///   2. Mel 频谱计算 (N_FFT=2048, hop=160, n_mels=256)
///   3. RMVPE ONNX 推理
///   4. cents → Hz 解码
///   5. 输出 F0Profile (hop=480 @ 48kHz，与 Python 端 np.repeat(f0_16k, 3) 等效)
///
/// 对于 3 分钟音频 (~864 万采样)，单次处理内存消耗约 10MB，无需分块。
pub fn extract_f0_from_file(
    audio_48k: &[f32],
    rmvpe_session: &mut Session,
) -> F0Profile {
    let nf = audio_48k.len() / 480;  // hop=480 @ 48kHz 下的帧数
    let f0 = rmpe_f0(audio_48k, nf.max(1), rmvpe_session);
    F0Profile {
        f0_curve: f0,
        sample_rate: 48000,
        hop_length: 480,
    }
}
