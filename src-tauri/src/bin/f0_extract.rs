//! F0 提取工具 — WAV → YIN F0 → .f0.json
//!
//! 用法:
//!   cargo run --bin f0-extract -- /path/to/vocal.wav
//!
//! 输出:
//!   /path/to/vocal.f0.json  (元数据)
//!   /path/to/vocal.f0.dat   (f32 LE F0 曲线, 可选)

use hound;
use serde::Serialize;
use std::path::Path;
use std::fs;

// ── F0Profile 格式（与 types.rs 兼容） ──
#[derive(Serialize)]
struct F0Profile {
    f0_curve: Vec<f32>,
    sample_rate: u32,
    hop_length: usize,
}

// ── 线性重采样 ──
fn resample(input: &[f32], from: f64, to: f64) -> Vec<f32> {
    if (from - to).abs() < 0.5 { return input.to_vec(); }
    let ratio = to / from;
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

// ── YIN 差分 F0 提取 ──
// 步长 10ms, 搜索范围 50Hz~1100Hz
fn extract_f0_yin(input_16k: &[f32], sr: u32) -> Vec<f32> {
    let hop = sr as usize / 100; // 10ms
    if input_16k.len() < hop * 2 { return vec![]; }
    let n_frames = input_16k.len() / hop;
    let min_tau = sr as usize / 1100; // ~15 @16kHz
    let max_tau = sr as usize / 50;   // ~320 @16kHz
    let mut f0 = Vec::with_capacity(n_frames);

    for i in 0..n_frames {
        let start = i * hop;
        let end = (start + hop * 4).min(input_16k.len());
        if end - start < hop + max_tau { f0.push(0.0); continue; }
        let seg = &input_16k[start..end];
        let w = seg.len() - max_tau;

        let mut d_min = f32::MAX;
        let mut best_tau = 0usize;

        for tau in min_tau..=max_tau.min(w - 1) {
            let mut diff = 0.0f32;
            for j in 0..w {
                let d = seg[j] - seg[j + tau];
                diff += d * d;
            }
            if diff < d_min {
                d_min = diff;
                best_tau = tau;
            }
            // 早期终止
            if tau > min_tau + 2 && diff > d_min * 1.3 { break; }
        }

        // 抛物线插值
        if best_tau > 0 && best_tau < max_tau {
            let t0 = best_tau.saturating_sub(1);
            let t2 = (best_tau + 1).min(max_tau);
            let mut d0 = 0.0f32;
            let mut d2 = 0.0f32;
            for j in 0..w {
                let d = seg[j] - seg[j + t0]; d0 += d * d;
                let d = seg[j] - seg[j + t2]; d2 += d * d;
            }
            let denom = 2.0 * (d0 + d2 - 2.0 * d_min);
            if denom.abs() > 1e-10 {
                let delta = (d0 - d2) / denom;
                let tau_f = best_tau as f32 + delta;
                f0.push(if tau_f > 0.0 { sr as f32 / tau_f } else { 0.0 });
            } else {
                f0.push(sr as f32 / best_tau as f32);
            }
        } else {
            f0.push(0.0);
        }
    }

    // 简单中值滤波 (5 帧)
    let mut smoothed = vec![0.0f32; f0.len()];
    for i in 0..f0.len() {
        let lo = i.saturating_sub(2);
        let hi = (i + 3).min(f0.len());
        let mut vals: Vec<f32> = f0[lo..hi].iter().copied().filter(|&v| v > 0.0).collect();
        smoothed[i] = if vals.is_empty() {
            0.0
        } else {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap());
            vals[vals.len() / 2]
        };
    }

    // 插值填补 0 值空洞
    let mut out = smoothed.clone();
    let mut i = 0;
    while i < out.len() {
        if out[i] <= 0.0 {
            let start = if i > 0 { i - 1 } else { i };
            let mut end = i + 1;
            while end < out.len() && out[end] <= 0.0 { end += 1; }
            let v_start = if start < out.len() && out[start] > 0.0 { out[start] } else { 200.0 };
            let v_end = if end < out.len() && out[end] > 0.0 { out[end] } else { 200.0 };
            let len = end - start;
            for j in start..end {
                let t = (j - start) as f32 / len as f32;
                out[j] = v_start * (1.0 - t) + v_end * t;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: f0-extract <input.wav>");
        eprintln!("");
        eprintln!("从 WAV 文件提取 F0 曲线并保存为 .f0.json");
        eprintln!("输出文件将与输入同名，后缀改为 .f0.json");
        eprintln!("");
        eprintln!("示例:");
        eprintln!("  cargo run --bin f0-extract -- /path/to/vocal.wav");
        std::process::exit(1);
    }

    let wav_path = Path::new(&args[1]);
    if !wav_path.exists() {
        eprintln!("错误: 文件不存在: {}", wav_path.display());
        std::process::exit(1);
    }

    println!("[F0] 读取: {}", wav_path.display());

    // 读取 WAV
    let mut reader = hound::WavReader::open(wav_path).expect("无法打开 WAV");
    let spec = reader.spec();
    let sr = spec.sample_rate;
    let channels = spec.channels as usize;
    println!("[F0] 采样率: {} Hz, 声道: {}, 位深: {}",
             sr, channels, spec.bits_per_sample);

    // 读取所有样本，取第一个声道
    let all_samples: Vec<i16> = reader.samples::<i16>()
        .map(|s| s.unwrap_or(0))
        .collect();
    // 取第一个声道（每隔 channels 个取一个）
    let samples: Vec<f32> = all_samples.iter()
        .step_by(channels)
        .map(|&s| s as f32 / 32768.0)
        .collect();

    let duration = samples.len() as f64 / sr as f64;
    println!("[F0] 时长: {:.1}s, 总采样: {}", duration, samples.len());

    // 重采样到 16kHz
    let r16 = if sr != 16000 {
        println!("[F0] 重采样 {}Hz → 16000Hz...", sr);
        resample(&samples, sr as f64, 16000.0)
    } else {
        samples
    };

    // YIN F0 提取
    println!("[F0] 提取 F0 (YIN算法, 10ms步长)...");
    let f0 = extract_f0_yin(&r16, 16000);
    println!("[F0] F0 帧数: {} (≈{:.1}s)", f0.len(), f0.len() as f32 * 0.01);

    // 统计
    let non_zero: Vec<f32> = f0.iter().filter(|&&v| v > 0.0).copied().collect();
    if !non_zero.is_empty() {
        let avg: f32 = non_zero.iter().sum::<f32>() / non_zero.len() as f32;
        let mut min = f32::MAX;
        let mut max = 0.0f32;
        for &v in &non_zero {
            if v < min { min = v; }
            if v > max { max = v; }
        }
        println!("[F0] 统计: avg={:.1}Hz, min={:.1}Hz, max={:.1}Hz, 有声帧={}%",
                 avg, min, max, non_zero.len() * 100 / f0.len());
    }

    // 保存 .f0.json
    let profile = F0Profile {
        f0_curve: f0,
        sample_rate: 48000,  // 引擎使用 48kHz 查表
        hop_length: 480,     // 10ms @48kHz
    };

    let json_path = wav_path.with_extension("f0.json");
    let json_str = serde_json::to_string_pretty(&profile).expect("JSON 序列化失败");
    fs::write(&json_path, &json_str).expect("写入 .f0.json 失败");
    println!("[F0] ✅ 保存: {}", json_path.display());
    println!("[F0] 文件大小: {:.1}KB", json_str.len() as f64 / 1024.0);
    println!("");
    println!("将此文件与伴奏路径关联即可使用:");
    println!("  伴奏: {}", wav_path.with_extension("wav").display());
    println!("  F0:   {}", json_path.display());
}
