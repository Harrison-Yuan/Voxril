//! 混音器 — 多路音频轨道 → 混合输出
//!
//! 这是整个直播引擎的"混音控制台"。
//! 所有修音后的人声、翻唱人声、伴奏都在这里按音量混合。
//!
//! # 排错指南
//! - 某路不出声 → 检查 `mix_into()` 中的音量系数
//! - 破音/削波 → 检查 `mix_into()` 中的软限幅 `x / (1+|x|)`
//! - 爆音/噼啪 → 检查 `mix_into()` 中 `StreamSmoother` 的启停

use crate::audio_engine::smoother::StreamSmoother;

/// 混音音量系数（可调）
pub const ACC_VOL: f32 = 0.5;
pub const VOCAL_VOL: f32 = 0.75;
pub const COVER_VOL: f32 = 0.5;

/// 软限幅：tanh 近似，防削波
#[inline(always)]
fn soft_clip(x: f32) -> f32 {
    x / (1.0 + x * x).sqrt()
}

/// 三路音频 → 混合写入 `out`（长度即本次混音帧数）
///
/// 参数说明：
/// - `acc_buf`  / `acc_read`: 伴奏数据及本轮有效帧数
/// - `acc_paused`: 伴奏暂停时整路静音（不消费数据由调用方保证）
/// - `vocal_buf` / `vocal_read`: 修音后人声数据及本轮有效帧数
/// - `cover_buf` / `cover_read`: 翻唱人声数据及本轮有效帧数
/// - `cover_enabled`: 由调用方在进入回调时读取一次 `AUDIO_SOURCE`，避免每采样读原子
/// - `acc_smoother` / `vocal_smoother` / `cover_smoother`: 启停平滑器（防爆音）
///
/// 实时约束：不分配内存、不读取原子、不加锁。
pub fn mix_into(
    acc_buf: &[f32],
    acc_read: usize,
    acc_paused: bool,

    vocal_buf: &[f32],
    vocal_read: usize,

    cover_buf: &[f32],
    cover_read: usize,
    cover_enabled: bool,

    acc_smoother: &mut StreamSmoother,
    vocal_smoother: &mut StreamSmoother,
    cover_smoother: &mut StreamSmoother,

    out: &mut [f32],
) {
    for i in 0..out.len() {
        // 伴奏：断流/暂停时用 smoothing 淡出
        let a = if acc_paused || i >= acc_read {
            acc_smoother.next(None)
        } else {
            acc_smoother.next(Some(acc_buf[i]))
        };

        // 人声：断流时用 smoothing 淡出
        let v = if i >= vocal_read {
            vocal_smoother.next(None)
        } else {
            vocal_smoother.next(Some(vocal_buf[i]))
        };

        // 翻唱人声：由 cover_enabled 门控 audible（0=真人，1=翻唱）
        let c = if cover_enabled && i < cover_read {
            cover_smoother.next(Some(cover_buf[i]))
        } else {
            cover_smoother.next(None)
        };

        out[i] = soft_clip(a * ACC_VOL + v * VOCAL_VOL + c * COVER_VOL);
    }
}

/// BlackHole 镜像混音（48kHz 输入，线性插值到设备原生采样率）
///
/// 参数与 `mix_into` 类似，但多出 `frames`（设备帧数）和 `sr_cb`（设备采样率）。
/// `sp = f * 48000 / sr_cb` 将 48kHz 索引映射到设备采样率空间。
pub fn mix_bh_monitor(
    frames: usize,
    sr_cb: u32,

    acc_buf: &[f32],
    acc_n: usize,
    acc_paused: bool,

    vocal_buf: &[f32],
    vocal_n: usize,

    cover_buf: &[f32],
    cover_n: usize,
    cover_enabled: bool,

    vocal_smoother: &mut StreamSmoother,
    cover_smoother: &mut StreamSmoother,

    data: &mut [f32],
    ch: usize,
) {
    for f in 0..frames {
        let sp = (f as f64) * 48000.0 / sr_cb as f64;
        let si = sp as usize;
        let fr = (sp - si as f64) as f32;

        // 伴奏（线性插值）
        let a = if si + 1 < acc_n && !acc_paused {
            (acc_buf[si] * (1.0 - fr) + acc_buf[si + 1] * fr) * ACC_VOL
        } else if si < acc_n && !acc_paused {
            acc_buf[si] * ACC_VOL
        } else {
            0.0
        };

        // 人声（线性插值 + smoothing）
        let v = if si + 1 < vocal_n {
            let interp = vocal_buf[si] * (1.0 - fr) + vocal_buf[si + 1] * fr;
            vocal_smoother.next(Some(interp)) * VOCAL_VOL
        } else if si < vocal_n {
            vocal_smoother.next(Some(vocal_buf[si])) * VOCAL_VOL
        } else {
            vocal_smoother.next(None)
        };

        // 翻唱（线性插值 + smoothing，由 cover_enabled 门控）
        let c = if cover_enabled && si + 1 < cover_n {
            let interp = cover_buf[si] * (1.0 - fr) + cover_buf[si + 1] * fr;
            cover_smoother.next(Some(interp)) * COVER_VOL
        } else if cover_enabled && si < cover_n {
            cover_smoother.next(Some(cover_buf[si])) * COVER_VOL
        } else {
            cover_smoother.next(None)
        };

        let sum = a + v + c;
        let s = soft_clip(sum);
        for c in 0..ch {
            data[f * ch + c] = s;
        }
    }
}
