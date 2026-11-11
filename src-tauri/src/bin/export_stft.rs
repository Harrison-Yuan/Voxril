//! 导出人声的 STFT 频谱，供直播修音 Phase Vocoder 使用。
//!
//! 用法: cargo run --bin export-stft -- <input.wav> [output.npy]
//!
//! 输出: .stft.npy (float32, shape=[n_frames, 257, 2])
//!   FFT=512, hop=128, hann窗, sr=48000
//!   帧长 ≈ 2.7ms，每帧对应 128 个采样
//!
//! 不需要 Python，不需要 librosa/soundfile/resampy。

use std::path::{Path, PathBuf};
use rustfft::{FftPlanner, num_complex::Complex};

const TARGET_SR: u32 = 48000;
const FFT_SIZE: usize = 2048;
const HOP_SIZE: usize = 512;
const N_BINS: usize = FFT_SIZE / 2 + 1; // 1025

fn build_hann_window(n: usize) -> Vec<f32> {
    let mut w = Vec::with_capacity(n);
    for i in 0..n {
        let v = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos());
        w.push(v);
    }
    w
}

/// 线性重采样（仅用于前置处理，质量足够）
fn resample(input: &[f32], from_sr: u32, to_sr: u32) -> Vec<f32> {
    if from_sr == to_sr {
        return input.to_vec();
    }
    let ratio = to_sr as f64 / from_sr as f64;
    let olen = (input.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(olen);
    for i in 0..olen {
        let sp = i as f64 / ratio;
        let si = sp as usize;
        let fr = (sp - si as f64) as f32;
        if si + 1 < input.len() {
            out.push(input[si] * (1.0 - fr) + input[si + 1] * fr);
        } else if si < input.len() {
            out.push(input[si]);
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: cargo run --bin export-stft -- <input.wav> [output.npy]");
        std::process::exit(1);
    }

    let wav_path = Path::new(&args[1]);
    if !wav_path.exists() {
        eprintln!("错误: 文件不存在: {}", wav_path.display());
        std::process::exit(1);
    }

    let out_path: PathBuf = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        wav_path.with_extension("stft.npy")
    };

    eprintln!("[STFT] 读取: {}", wav_path.display());

    // 读 WAV
    let mut reader = hound::WavReader::open(wav_path).expect("无法打开 WAV");
    let spec = reader.spec();
    let sr = spec.sample_rate;
    let channels = spec.channels as usize;
    eprintln!("[STFT] 采样率: {} Hz, 声道: {}, 位深: {}",
        sr, channels, spec.bits_per_sample);

    let all_samples: Vec<i16> = reader.samples::<i16>()
        .map(|s| s.unwrap_or(0))
        .collect();

    // 取第一声道，归一化到 [-1, 1]
    let audio: Vec<f32> = all_samples.iter()
        .step_by(channels)
        .map(|&s| s as f32 / 32768.0)
        .collect();
    let duration = audio.len() as f64 / sr as f64;
    eprintln!("[STFT] 时长: {:.1}s, 总采样: {}", duration, audio.len());

    // 重采样到 48kHz
    let audio = if sr != TARGET_SR {
        eprintln!("[STFT] 重采样 {}Hz → {}Hz...", sr, TARGET_SR);
        resample(&audio, sr, TARGET_SR)
    } else {
        audio
    };

    // 计算帧数
    let n_frames = if audio.len() < FFT_SIZE {
        0
    } else {
        (audio.len() - FFT_SIZE) / HOP_SIZE + 1
    };
    eprintln!("[STFT] 帧数: {}, hop={}, fft={}", n_frames, HOP_SIZE, FFT_SIZE);

    if n_frames == 0 {
        eprintln!("[STFT] 错误: 音频过短");
        std::process::exit(1);
    }

    // 预计算 Hann 窗
    let window = build_hann_window(FFT_SIZE);

    // FFT planner
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    // 逐帧 STFT
    let mut spectrum = vec![0.0f32; n_frames * N_BINS * 2];

    for t in 0..n_frames {
        let start = t * HOP_SIZE;
        let mut frame = vec![Complex::new(0.0f32, 0.0); FFT_SIZE];
        for i in 0..FFT_SIZE {
            let val = if start + i < audio.len() { audio[start + i] } else { 0.0 };
            frame[i] = Complex::new(val * window[i], 0.0);
        }

        fft.process(&mut frame);

        let base = t * N_BINS * 2;
        for bin in 0..N_BINS {
            spectrum[base + bin * 2] = frame[bin].re;
            spectrum[base + bin * 2 + 1] = frame[bin].im;
        }
    }

    // 写 NPY
    write_npy(&out_path, &spectrum, n_frames, N_BINS);

    let size_mb = spectrum.len() as f64 * 4.0 / (1024.0 * 1024.0);
    eprintln!("[STFT] ✅ 保存: {} ({:.1}MB, {} 帧 × {} bins)",
        out_path.display(), size_mb, n_frames, N_BINS);
}

/// 写 NPY (numpy 格式, float32, shape=[n_frames, n_bins, 2])
fn write_npy(path: &Path, data: &[f32], n_frames: usize, n_bins: usize) {
    let header = format!(
        "{{'descr': '<f4', 'fortran_order': False, 'shape': ({}, {}, 2), }}",
        n_frames, n_bins
    );
    // NPY 魔数 + 版本 + header_len
    let magic: [u8; 6] = [0x93, b'N', b'U', b'M', b'P', b'Y'];
    let ver: [u8; 2] = [1, 0];
    let header_bytes = header.as_bytes();
    let header_len = header_bytes.len() as u16;
    // 填充到 64 字节对齐
    let total_prefix = magic.len() + ver.len() + 2 + header_bytes.len();
    let pad = (64 - (total_prefix % 64)) % 64;
    let mut padded_header = Vec::with_capacity(total_prefix + pad);
    padded_header.extend_from_slice(&magic);
    padded_header.extend_from_slice(&ver);
    padded_header.extend_from_slice(&header_len.to_le_bytes());
    padded_header.extend_from_slice(header_bytes);
    padded_header.extend(std::iter::repeat(b' ').take(pad));
    padded_header.push(b'\n'); // NPY 规范要求换行结尾

    let mut out = std::fs::File::create(path).expect("无法创建输出文件");
    use std::io::Write;
    out.write_all(&padded_header).expect("写入头失败");

    // 写 float32 数据（小端序）
    for &v in data {
        out.write_all(&v.to_le_bytes()).expect("写入数据失败");
    }
}
