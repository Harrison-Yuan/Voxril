//! 实时修音引擎 — Phase Vocoder 音高移动
//!
//! 参数: FFT=2048, hop=512, sr=48000, bin_width=23.4Hz
//! 延迟: ~10.7ms (512/48000)
//!
//! 信号链:
//!   麦克风 20ms块 → 累积 → FFT2048 → 频域移调(pitch_ratio) → IFFT → WOLA → 输出

use rustfft::{FftPlanner, num_complex::Complex};
use std::path::Path;

// ── STFT 参数 ──
// 2048 点 FFT = 23.4Hz/bin @48kHz，足以精准分离人声基频和各次谐波
pub const PV_FFT_SIZE: usize = 2048;
pub const PV_HOP: usize = 512;

/// 预计算的 STFT 频谱模板（从完美合成的 AI 歌声提取）
///
/// 在直播前通过 `cargo run --bin export-stft converted_xxx.wav` 生成 `.stft.npy` 文件。
/// 直播时加载此模板，Phase Vocoder 使用模板的相位信息抑制 phasiness 失真。
pub struct StftTemplate {
    /// 复数频谱 [n_frames][n_bins] = n_frames × 1025 个 complex64
    pub spectrum: Vec<Complex<f32>>,
    pub n_frames: usize,
    pub n_bins: usize,
    /// 每帧对应的时间 (秒)
    frame_duration: f64,
}

impl StftTemplate {
    /// 从 .stft.npy 文件加载模板
    pub fn load(path: &Path) -> Result<Self, String> {
        let data: Vec<f32> = npy_loader::load_npy(path)
            .map_err(|e| format!("读取 STFT 模板失败: {}", e))?;

        // shape: [n_frames, n_bins, 2] (real, imag)
        let total = data.len();
        if total % 2 != 0 {
            return Err("STFT 模板数据长度异常".into());
        }
        let n_pairs = total / 2;
        // n_bins = FFT_SIZE/2 + 1 = 1025
        let n_bins = PV_FFT_SIZE / 2 + 1;
        if n_pairs % n_bins != 0 {
            return Err(format!("STFT 模板 bin 数不匹配: {} 不能被 {} 整除", n_pairs, n_bins));
        }
        let n_frames = n_pairs / n_bins;

        let mut spectrum = Vec::with_capacity(n_pairs);
        for i in 0..n_pairs {
            spectrum.push(Complex::new(data[i * 2], data[i * 2 + 1]));
        }

        Ok(Self {
            spectrum,
            n_frames,
            n_bins,
            frame_duration: PV_HOP as f64 / 48000.0,
        })
    }

    /// 根据时间戳 (秒) 获取对应帧的频谱
    pub fn frame_at(&self, time_secs: f64) -> Option<&[Complex<f32>]> {
        let frame = (time_secs / self.frame_duration) as usize;
        if frame >= self.n_frames { return None; }
        let start = frame * self.n_bins;
        Some(&self.spectrum[start..start + self.n_bins])
    }
}

/// 实时相位声码器
///
/// 每个处理帧：
///   1. FFT 分析 → 瞬时幅度 + 绝对瞬时频率
///   2. 按 ratio 同步插值映射到新频域
///   3. 瞬时频率相位传播（保证时域连续）
///   4. IFFT → WOLA → 输出
pub struct PitchCorrector {
    /// 输入累积缓冲区
    input_buf: Vec<f32>,
    /// OLA 输出尾部
    output_tail: Vec<f32>,
    /// 分析相位 [0..n_bins]
    ana_phase: Vec<f64>,
    /// 合成相位 [0..n_bins]
    syn_phase: Vec<f64>,
    /// Hann 分析窗
    window: Vec<f32>,
    /// FFT planner
    planner: FftPlanner<f32>,
    /// 已处理的帧数 (用于模板同步)
    frame_count: usize,
}

impl PitchCorrector {
    pub fn new() -> Self {
        let n_bins = PV_FFT_SIZE / 2 + 1;
        let mut window = Vec::with_capacity(PV_FFT_SIZE);
        let n = PV_FFT_SIZE as f32;
        for i in 0..PV_FFT_SIZE {
            // ⚠️ 修复：使用 n 而非 n-1，确保完美周期窗（首尾为 0）
            let v = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n).cos());
            window.push(v);
        }

        Self {
            input_buf: Vec::with_capacity(PV_FFT_SIZE * 2),
            output_tail: Vec::new(),
            ana_phase: vec![0.0; n_bins],
            syn_phase: vec![0.0; n_bins],
            window,
            planner: FftPlanner::new(),
            frame_count: 0,
        }
    }

    /// 处理音频，移动音高
    ///
    /// - `audio`: 输入音频 (48kHz, 任意长度)
    /// - `pitch_ratio`: 目标/原始音高比 (>1=升调, <1=降调, 1=不变)
    ///
    /// 返回修正后的音频，长度与输入近似
    pub fn process(
        &mut self,
        audio: &[f32],
        pitch_ratio: f32,
    ) -> Vec<f32> {
        self.input_buf.extend_from_slice(audio);
        let mut output = Vec::with_capacity(audio.len() + PV_HOP);
        let ratio = pitch_ratio.clamp(0.5, 2.0);
        let n_bins = PV_FFT_SIZE / 2 + 1;

        // ⚠️ 核心修复：完美直通判定。不需要变调时，绝对不破坏原始相位
        let is_passthrough = (ratio - 1.0).abs() < 0.005;

        while self.input_buf.len() >= PV_FFT_SIZE {
            let mut fft_in = vec![Complex::new(0.0, 0.0); PV_FFT_SIZE];
            for i in 0..PV_FFT_SIZE {
                fft_in[i] = Complex::new(self.input_buf[i] * self.window[i], 0.0);
            }

            let fft = self.planner.plan_fft_forward(PV_FFT_SIZE);
            fft.process(&mut fft_in);

            let mut fft_out = vec![Complex::new(0.0, 0.0); PV_FFT_SIZE];
            let mut ana_mag = vec![0.0f32; n_bins];
            let mut ana_freq = vec![0.0f64; n_bins];

            // 1. 分析频率
            for bin in 0..n_bins {
                let mag = fft_in[bin].norm();
                let phase = fft_in[bin].arg() as f64;

                // ⚠️ 修复：初始化相位锁，确保启动第一帧不发生任何相位错位
                if self.frame_count == 0 {
                    self.ana_phase[bin] = phase;
                    self.syn_phase[bin] = phase;
                }

                let base_advance = 2.0 * std::f64::consts::PI * bin as f64 * PV_HOP as f64 / PV_FFT_SIZE as f64;
                let mut phase_diff = phase - self.ana_phase[bin] - base_advance;

                while phase_diff > std::f64::consts::PI { phase_diff -= 2.0 * std::f64::consts::PI; }
                while phase_diff < -std::f64::consts::PI { phase_diff += 2.0 * std::f64::consts::PI; }

                self.ana_phase[bin] = phase;
                ana_mag[bin] = mag;
                ana_freq[bin] = base_advance + phase_diff;
            }

            // 2. 映射与合成
            for bin in 0..n_bins {
                if is_passthrough {
                    // ⚠️ 修复：100% 物理级直通，保留所有真实唇齿音和瞬态能量
                    fft_out[bin] = fft_in[bin];
                    self.syn_phase[bin] = fft_in[bin].arg() as f64;
                } else {
                    let src_bin = bin as f32 / ratio;
                    let (interp_mag, interp_freq) = if src_bin < (n_bins - 1) as f32 {
                        let lo = src_bin.floor().max(0.0) as usize;
                        let hi = lo + 1;
                        let frac = src_bin - lo as f32;
                        let mag = ana_mag[lo] * (1.0 - frac) + ana_mag[hi] * frac;
                        let freq = ana_freq[lo] * (1.0 - frac as f64) + ana_freq[hi] * (frac as f64);
                        (mag, freq)
                    } else if (src_bin as usize) < n_bins {
                        (ana_mag[src_bin as usize], ana_freq[src_bin as usize])
                    } else {
                        (0.0, 0.0)
                    };

                    self.syn_phase[bin] += interp_freq * ratio as f64;
                    self.syn_phase[bin] %= 2.0 * std::f64::consts::PI;
                    fft_out[bin] = Complex::from_polar(interp_mag, self.syn_phase[bin] as f32);
                }
            }

            // 3. 严格的共轭对称（如果是变声状态，必须修补物理对称性）
            if !is_passthrough {
                fft_out[0].im = 0.0; // ⚠️ 修复：强制剔除直流电平的虚部畸变
                if n_bins > 0 {
                    fft_out[n_bins - 1].im = 0.0; // 强制剔除奈奎斯特极限的虚部畸变
                }
                for bin in 1..(PV_FFT_SIZE / 2) {
                    let mirror_bin = PV_FFT_SIZE - bin;
                    fft_out[mirror_bin] = fft_out[bin].conj();
                }
            }

            // --- 完美 WOLA 逆变换 ---
            let ifft = self.planner.plan_fft_inverse(PV_FFT_SIZE);
            ifft.process(&mut fft_out);

            let scale = 1.0 / PV_FFT_SIZE as f32;
            let mut frame = vec![0.0f32; PV_FFT_SIZE];
            for i in 0..PV_FFT_SIZE {
                frame[i] = fft_out[i].re * scale * self.window[i];
            }

            let mut mixed = vec![0.0f32; PV_FFT_SIZE];
            let copy_len = self.output_tail.len().min(PV_FFT_SIZE);
            for i in 0..copy_len {
                mixed[i] = self.output_tail[i];
            }
            for i in 0..PV_FFT_SIZE {
                mixed[i] += frame[i] * (2.0 / 3.0);
            }
            output.extend_from_slice(&mixed[..PV_HOP]);
            self.output_tail = mixed[PV_HOP..].to_vec();

            self.input_buf.drain(..PV_HOP);
            self.frame_count += 1;
        }

        output
    }

    /// 直通模式：绕过 FFT 管线，直接输出
    /// 不经过 FFT/IFFT/WOLA，避免任何频谱处理失真
    pub fn process_passthrough(&mut self, audio: &[f32]) -> Vec<f32> {
        // 直接追加到输入缓冲区，但以正常速率输出
        // 保持与 process() 相同的内部缓冲节奏，避免时间轴错位
        self.input_buf.extend_from_slice(audio);

        // 当缓冲区足够时，按 HOP 大小正常输出（不做频谱处理）
        let mut output = Vec::with_capacity(audio.len());
        while self.input_buf.len() >= PV_HOP {
            // 直接从输入缓冲区取出 HOP 个采样输出
            let out: Vec<f32> = self.input_buf.drain(..PV_HOP).collect();
            output.extend_from_slice(&out);
        }
        // 剩余的留在 input_buf 中等待下次处理

        // 保持 frame_count 同步，避免模板/调试计数器失步
        //（此处不递增 frame_count，因为直通没有真正的帧处理）
        output
    }

    /// 冲刷缓冲区：输出所有剩余音频
    pub fn flush(&mut self) -> Vec<f32> {
        let mut out = Vec::new();
        // 输出 OLA 尾部
        if !self.output_tail.is_empty() {
            out.extend(self.output_tail.drain(..));
        }
        // 输出输入缓冲剩余
        if !self.input_buf.is_empty() {
            out.extend(self.input_buf.drain(..));
        }
        self.frame_count = 0;
        out
    }
}

/// 读取 WAV 并导出 STFT 频谱 (.stft.npy)
/// 可直接在 Tauri 命令中同步调用，无需子进程。
pub fn export_stft_to_file(wav_path: &Path, out_path: &Path) -> Result<(), String> {
    let mut reader = hound::WavReader::open(wav_path)
        .map_err(|e| format!("无法打开 WAV: {}", e))?;
    let spec = reader.spec();

    let all_samples: Vec<i16> = reader.samples::<i16>()
        .map(|s| s.unwrap_or(0))
        .collect();

    // 取第一声道，归一化
    let channels = spec.channels as usize;
    let audio: Vec<f32> = all_samples.iter()
        .step_by(channels)
        .map(|&s| s as f32 / 32768.0)
        .collect();

    // 重采样到 48kHz（线性插值）
    let sr = spec.sample_rate;
    let audio = if sr != 48000 {
        let ratio = 48000.0 / sr as f64;
        let olen = (audio.len() as f64 * ratio) as usize;
        let mut out = Vec::with_capacity(olen);
        for i in 0..olen {
            let sp = i as f64 / ratio;
            let si = sp as usize;
            let fr = (sp - si as f64) as f32;
            if si + 1 < audio.len() {
                out.push(audio[si] * (1.0 - fr) + audio[si + 1] * fr);
            } else if si < audio.len() {
                out.push(audio[si]);
            }
        }
        out
    } else {
        audio
    };

    // STFT 参数
    let fft_size = PV_FFT_SIZE;
    let hop = PV_HOP;
    let n_bins = fft_size / 2 + 1;
    let n_frames = if audio.len() < fft_size { 0 } else { (audio.len() - fft_size) / hop + 1 };
    if n_frames == 0 {
        return Err("音频过短，无法计算 STFT".into());
    }

    // Hann 窗
    let window: Vec<f32> = (0..fft_size).map(|i| {
        0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size - 1) as f32).cos())
    }).collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut spectrum = vec![0.0f32; n_frames * n_bins * 2];

    for t in 0..n_frames {
        let start = t * hop;
        let mut frame = vec![Complex::new(0.0f32, 0.0); fft_size];
        for i in 0..fft_size {
            let val = if start + i < audio.len() { audio[start + i] } else { 0.0 };
            frame[i] = Complex::new(val * window[i], 0.0);
        }

        fft.process(&mut frame);

        let base = t * n_bins * 2;
        for bin in 0..n_bins {
            spectrum[base + bin * 2] = frame[bin].re;
            spectrum[base + bin * 2 + 1] = frame[bin].im;
        }
    }

    // NPY 格式：魔数 + 版本 + header_len + header + data
    let header = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({}, {}, 2), }}", n_frames, n_bins);
    let magic: [u8; 6] = [0x93, b'N', b'U', b'M', b'P', b'Y'];
    let ver: [u8; 2] = [1, 0];
    let header_bytes = header.as_bytes();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&magic);
    prefix.extend_from_slice(&ver);
    prefix.extend_from_slice(&(header_bytes.len() as u16).to_le_bytes());
    prefix.extend_from_slice(header_bytes);
    // 填充到 64 字节对齐
    let pad = (64 - (prefix.len() % 64)) % 64;
    for _ in 0..pad { prefix.push(b' '); }
    prefix.push(b'\n');

    let mut f = std::fs::File::create(out_path).map_err(|e| format!("创建文件失败: {}", e))?;
    use std::io::Write;
    f.write_all(&prefix).map_err(|e| format!("写入头失败: {}", e))?;

    for &v in &spectrum {
        f.write_all(&v.to_le_bytes()).map_err(|e| format!("写入数据失败: {}", e))?;
    }

    Ok(())
}

// ── 简易 .npy 加载器（仅支持 float32 小端序） ──
mod npy_loader {
    use std::io::Read;

    pub fn load_npy(path: &std::path::Path) -> Result<Vec<f32>, String> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("打开失败: {}", e))?;
        let mut magic = [0u8; 6];
        file.read_exact(&mut magic)
            .map_err(|e| format!("读取魔数失败: {}", e))?;
        if &magic != b"\x93NUMPY" {
            return Err("不是 NPY 文件".into());
        }

        let mut header_len_bytes = [0u8; 2];
        file.read_exact(&mut header_len_bytes)
            .map_err(|e| format!("读取头部长度失败: {}", e))?;
        let header_len = u16::from_le_bytes(header_len_bytes) as usize;

        let mut header = vec![0u8; header_len];
        file.read_exact(&mut header)
            .map_err(|e| format!("读取头部失败: {}", e))?;
        let header_str = String::from_utf8_lossy(&header);

        if !header_str.contains("f4") && !header_str.contains("<f4") {
            return Err(format!("不支持的 dtype: {}", header_str));
        }

        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .map_err(|e| format!("读取数据失败: {}", e))?;

        let f32_count = data.len() / 4;
        let mut floats = Vec::with_capacity(f32_count);
        for i in 0..f32_count {
            let bytes: [u8; 4] = [data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]];
            floats.push(f32::from_le_bytes(bytes));
        }

        Ok(floats)
    }
}
