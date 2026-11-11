//! 语音/唱歌状态检测器
//!
//! 结合 RMS + F0 连续性 + F0 稳定性 的多维状态机，
//! 用迟滞（hysteresis）防抖，确保 Silent/Speaking/Singing 切换干净利落，
//! 翻唱安全网据此控制混入/切出时机。

use std::sync::atomic::{AtomicBool, Ordering};

/// 修音线程发布给输出回调的唱歌标志（无锁跨线程）
pub static IS_SINGING: AtomicBool = AtomicBool::new(false);

/// 检测器状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Silent,
    Speaking,
    Singing,
}

/// 语音/唱歌状态检测器
pub struct VoiceStateDetector {
    /// 当前状态
    state: VoiceState,
    /// F0 历史环形缓冲（最近 50 帧 ~1s）
    f0_history: [f32; 50],
    f0_history_pos: usize,
    f0_history_count: usize,
    /// 连续有声帧计数（进入 Singing 的门控）
    voiced_frames: usize,
    /// 无声帧计数（退出状态的 hangover）
    silent_frames: usize,
    /// 当前帧的 F0
    current_f0: f32,
    current_rms: f32,
}

impl VoiceStateDetector {
    pub fn new() -> Self {
        Self {
            state: VoiceState::Silent,
            f0_history: [0.0; 50],
            f0_history_pos: 0,
            f0_history_count: 0,
            voiced_frames: 0,
            silent_frames: 0,
            current_f0: 0.0,
            current_rms: 0.0,
        }
    }

    /// 每帧调用一次，返回当前判定状态
    ///
    /// - `f0`: 本帧检测的基频 (Hz)，无声为 0
    /// - `rms`: 本帧 RMS 能量
    pub fn update(&mut self, f0: f32, rms: f32) -> VoiceState {
        self.current_f0 = f0;
        self.current_rms = rms;

        // 更新 F0 历史
        self.f0_history[self.f0_history_pos] = f0;
        self.f0_history_pos = (self.f0_history_pos + 1) % 50;
        self.f0_history_count = (self.f0_history_count + 1).min(50);

        // ═══ 特征提取 ═══

        // 1. 有声音检测
        // ⚠️ 核心修复：RMS 为主要的 voiced 判定依据，F0 只作为辅助。
        //    FCPE/RMVPE 对某些音素/辅音可能偶尔返回 0，
        //    如果依赖 F0 > 0 作为 voiced 条件，会导致状态在说话中反复跳 mute。
        //    阈值 0.0003（≈ -70dBFS）覆盖所有正常说话音量，
        //    对应 16bit 量化噪声（-96dBFS）之上，不会触发底噪。
        let rms_threshold = 0.0003;
        let voiced = rms > rms_threshold;

        // 2. 有稳定的 F0（唱歌特征）：需要 F0 > 0 且 RMS 足够
        let f0_present = f0 > 0.0 && rms > rms_threshold;

        // 3. F0 稳定性：最近有声帧的 F0 标准差 (越低越稳定 → 唱歌)
        let f0_stability = self.f0_stability();

        // 4. 是否有持续稳定的 F0（唱歌特征）
        let has_stable_f0 = f0_present && f0_stability < 0.15; // 15% 以内的波动视为稳定

        // 5. 能量是否达到唱歌级（比基本阈值约高 5 倍，过滤掉安静呼吸）
        let singing_energy = rms > rms_threshold * 5.0;

        // ═══ 计数更新 ═══
        if voiced {
            self.voiced_frames = self.voiced_frames.saturating_add(1);
            self.silent_frames = 0;
        } else {
            self.voiced_frames = 0;
            self.silent_frames = self.silent_frames.saturating_add(1);
        }

        // ═══ 状态机 ═══
        self.state = match self.state {
            VoiceState::Silent => {
                if has_stable_f0 && singing_energy && self.voiced_frames >= 3 {
                    // 连续 3 帧稳定 F0 + 足够能量 → 唱歌
                    VoiceState::Singing
                } else if rms > rms_threshold {
                    // 任何能量（包括辅音）→ 说话
                    // 但要防误触发（麦克风底噪），用 2 帧确认
                    if self.voiced_frames >= 2 || rms > rms_threshold * 10.0 {
                        VoiceState::Speaking
                    } else {
                        VoiceState::Silent
                    }
                } else {
                    VoiceState::Silent
                }
            }

            VoiceState::Speaking => {
                if has_stable_f0 && singing_energy && self.voiced_frames >= 3 {
                    // 稳定唱歌特征 → 切换到唱歌
                    VoiceState::Singing
                } else if self.silent_frames >= 20 {
                    // 400ms 完全无声 → Silent
                    VoiceState::Silent
                } else if rms > rms_threshold {
                    // 还有能量 → 继续说话
                    VoiceState::Speaking
                } else {
                    // 短暂停顿（辅音间隙等），保持 Speaking 等 recovery
                    VoiceState::Speaking
                }
            }

            VoiceState::Singing => {
                if self.silent_frames >= 8 {
                    // 160ms 无 F0 → 明确断句
                    if rms > rms_threshold {
                        VoiceState::Speaking
                    } else {
                        VoiceState::Silent
                    }
                } else if voiced && f0_stability > 0.25 {
                    // ⚠️ 核心修复：防止"唱歌死锁" (Singing Lock)
                    // 如果歌手没有断气（无静音停顿）直接从唱歌无缝切换到说话，
                    // F0 的变异系数会瞬间飙升。当变异超过 25% 时，强行降级为说话。
                    VoiceState::Speaking
                } else {
                    // 包含 !voiced 时的短暂无声（辅音间隙），以及 F0 稳定时的连续歌唱
                    VoiceState::Singing
                }
            }
        };

        // 发布到全局标志
        IS_SINGING.store(self.state == VoiceState::Singing, Ordering::Relaxed);

        self.state
    }

    /// 最近有声帧的 F0 变异系数（标准差/均值）
    /// 值越小越稳定：唱歌 < 0.15, 说话通常 > 0.3
    fn f0_stability(&self) -> f32 {
        let n = self.f0_history_count.min(20); // 只看最近 ~400ms
        if n < 3 {
            return 1.0; // 帧数太少 → 不稳定
        }

        let mut sum = 0.0f32;
        let mut count = 0;
        for i in 0..n {
            let idx = (self.f0_history_pos + 50 - 1 - i) % 50;
            let f = self.f0_history[idx];
            if f > 0.0 {
                sum += f;
                count += 1;
            }
        }
        if count < 3 {
            return 1.0; // 有声帧太少 → 不稳定
        }
        let mean = sum / count as f32;

        let mut var = 0.0f32;
        for i in 0..n {
            let idx = (self.f0_history_pos + 50 - 1 - i) % 50;
            let f = self.f0_history[idx];
            if f > 0.0 {
                let d = f - mean;
                var += d * d;
            }
        }
        var /= count as f32;
        (var.sqrt() / mean).min(1.0) // 归一化变异系数
    }

    pub fn current_state(&self) -> VoiceState {
        self.state
    }
}
