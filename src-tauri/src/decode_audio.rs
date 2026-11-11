//! 音频解码子命令 — 用伴生 ffmpeg 解码任意音频格式，输出 raw f32 PCM
//!
//! 用法: `cantarix decode-audio --input /path/to/audio.wav --sr 16000`
//!
//! 输出: stdout 写入 f32 LE PCM（单声道，目标采样率）
//! 返回值: 0=成功, 1=错误

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn run(args: &[String]) {
    let input = match args.iter().position(|a| a == "--input").and_then(|i| args.get(i + 1)) {
        Some(v) => v.clone(),
        None => { eprintln!("用法: cantarix decode-audio --input <path> --sr <hz>"); std::process::exit(1); }
    };
    let target_sr: u32 = match args.iter().position(|a| a == "--sr").and_then(|i| args.get(i + 1)) {
        Some(v) => v.parse().unwrap_or(16000),
        None => 16000,
    };

    let ffmpeg_path = find_ffmpeg_bin();

    let output = Command::new(&ffmpeg_path)
        .args(&[
            "-y",
            "-i", &input,
            "-f", "f32le",       // 输出格式：32位浮点 PCM
            "-ac", "1",          // 单声道
            "-ar", &target_sr.to_string(), // 目标采样率
            "-",                 // 输出到 stdout
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())  // 捕获 stderr 以便调试
        .output()
        .unwrap_or_else(|e| {
            eprintln!("[decode-audio] ffmpeg 调用失败: {}", e);
            std::process::exit(1);
        });

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("[decode-audio] ffmpeg 失败 (exit={}): {}", output.status, stderr.trim());
        std::process::exit(1);
    }

    // 将 f32 LE PCM 输出到 stdout
    std::io::stdout().write_all(&output.stdout).ok();
}

fn find_ffmpeg_bin() -> PathBuf {
    // 优先级 1：伴生路径（DMG 模式）
    if let Ok(exe) = std::env::current_exe() {
        // Tauri v2: Resources/bin/ffmpeg（直接资源）
        let r1 = exe.parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("Resources").join("bin").join("ffmpeg"));
        if let Some(ref p) = r1 { if p.exists() { return p.clone(); } }
        // Tauri v2: Resources/_up_/bin/ffmpeg（上级资源）
        let r2 = exe.parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("Resources").join("_up_").join("bin").join("ffmpeg"));
        if let Some(ref p) = r2 { if p.exists() { return p.clone(); } }
        // 同目录 sidecar
        let r3 = exe.parent().map(|p| p.join("bin").join("ffmpeg"));
        if let Some(ref p) = r3 { if p.exists() { return p.clone(); } }
    }
    // 优先级 2：开发模式
    let dev = PathBuf::from("bin/ffmpeg");
    if dev.exists() { return dev; }
    // 优先级 3：系统 PATH
    PathBuf::from("ffmpeg")
}
