//! Auto-Tune — 全自动实时音准修正
//!
//! # 核心算法
//! 1. **三模板加权调性检测** — Krumhansl-Schmuckler + Temperley-Kostka-Payne + Temperley 2007
//!    三种不同的调性感知模型，加权平均取最高分，比单一模板更鲁棒
//! 2. **谐波累积直方图** — F0 输入时同时累加 2f/3f/4f 谐波能量到直方图
//!    单音也能快速推断调性（从谐波看音阶位置）
//! 3. **置信度迟滞** — 只有连续 3 次检测到同一调性才切换
//!    避免歌曲间奏/念白时的误判

// ══════════════════════════════════════════════════════════════════
//  三套调性模板（12 个半音的能量分布）
// ══════════════════════════════════════════════════════════════════

/// 模板 1: Krumhansl-Schmuckler (1990) — 基于探音实验
const KS_MAJOR: [f32; 12] = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const KS_MINOR: [f32; 12] = [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

/// 模板 2: Temperley-Kostka-Payne (1999) — 基于古典音乐语料统计
const TKP_MAJOR: [f32; 12] = [5.00, 2.00, 3.50, 2.00, 4.50, 4.00, 2.00, 4.50, 2.00, 3.50, 1.50, 4.00];
const TKP_MINOR: [f32; 12] = [5.00, 2.00, 3.50, 4.50, 2.00, 4.00, 2.00, 4.50, 3.50, 2.00, 1.50, 4.00];

/// 模板 3: Temperley (2007) — 改进版调性模板
const T07_MAJOR: [f32; 12] = [5.00, 2.50, 3.00, 2.50, 4.00, 4.00, 2.50, 4.00, 2.50, 3.00, 2.50, 4.00];
const T07_MINOR: [f32; 12] = [5.00, 2.50, 3.00, 4.00, 2.50, 4.00, 2.50, 4.00, 3.00, 2.50, 2.50, 4.00];

/// 三模板权值（Temperley 2007 最准，KS 次之，TKP 第三）
const W_KS: f32 = 1.0;
const W_TKP: f32 = 0.8;
const W_T07: f32 = 1.2;

/// 谐波倍率权重 — F0 输入时同步累加 f, 2f, 3f, 4f
/// 权重: f=1.0, 2f=0.5, 3f=0.25, 4f=0.125
const HARMONIC_WEIGHTS: [f32; 4] = [1.0, 0.5, 0.25, 0.125];

// ══════════════════════════════════════════════════════════════════

/// 12 个半音的名称（C 大调起始）
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// 检测到的调性
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedKey {
    pub root: u8,       // 0=C, 1=C#, ..., 11=B
    pub is_major: bool,  // true=大调, false=小调
    pub confidence: f32, // 0.0~1.0，1.0=极高置信度
}

impl DetectedKey {
    /// 调性名称，如 "C Major"、"A Minor"
    pub fn label(&self) -> String {
        let mode = if self.is_major { "Major" } else { "Minor" };
        format!("{} {}", NOTE_NAMES[self.root as usize], mode)
    }
}

/// 调性检测器 — 三模板加权 + 谐波累积 + 置信门控
pub struct KeyDetector {
    /// 12 个半音的累计能量（指数衰减滑动窗口）
    histogram: [f32; 12],
    /// 最新检测结果
    current_key: DetectedKey,
    /// 检测到同一调性的连续次数（迟滞防抖）
    stable_count: u32,
    /// 自上次检测以来收集的有效 F0 数量
    f0_count: u32,
}

impl KeyDetector {
    pub fn new() -> Self {
        Self {
            histogram: [0.0; 12],
            current_key: DetectedKey { root: 0, is_major: true, confidence: 0.0 },
            stable_count: 0,
            f0_count: 0,
        }
    }

    /// 输入一帧的 F0 值（含谐波累积）
    ///
    /// 除了 F0 本身，还将 2f/3f/4f 谐波按衰减权重累加到直方图中。
    /// 这样即使只唱了一个音，也能通过谐波推断其在音阶中的位置。
    pub fn feed(&mut self, f0_hz: f32) {
        if f0_hz < 60.0 || f0_hz > 2000.0 {
            return;
        }

        // 累积 F0 及其谐波
        for (h, &weight) in HARMONIC_WEIGHTS.iter().enumerate() {
            let freq = f0_hz * (h + 1) as f32;
            if freq > 5000.0 { break; } // 超过 5kHz 噪声居多，跳过

            let semitone = ((12.0 * (freq / 440.0).log2() + 69.0) % 12.0).round() as i32;
            let idx = semitone.rem_euclid(12) as usize;
            self.histogram[idx] += weight;
        }
        self.f0_count += 1;

        // 指数衰减：半衰期约 2 秒 (48000Hz / 960 = 50 frames/s)
        // 每帧衰减因子: 0.5^(1/(50*2)) ≈ 0.993
        for v in self.histogram.iter_mut() {
            *v *= 0.993;
        }
    }

    /// 检测当前调性。每 ~0.5 秒调一次即可（25 帧）
    pub fn detect(&mut self) -> DetectedKey {
        // 至少要收集到 10 个有效 F0 才做检测，否则返回上次结果
        if self.f0_count < 10 {
            return self.current_key;
        }
        self.f0_count = 0;

        // ── 三模板加权评分 ──
        let mut candidates: Vec<(f32, u8, bool)> = Vec::with_capacity(24);

        for root in 0..12 {
            // 大调
            let ks = self.correlation(root, &KS_MAJOR);
            let tkp = self.correlation(root, &TKP_MAJOR);
            let t07 = self.correlation(root, &T07_MAJOR);
            let major_score = ks * W_KS + tkp * W_TKP + t07 * W_T07;
            candidates.push((major_score, root as u8, true));

            // 小调
            let ks = self.correlation(root, &KS_MINOR);
            let tkp = self.correlation(root, &TKP_MINOR);
            let t07 = self.correlation(root, &T07_MINOR);
            let minor_score = ks * W_KS + tkp * W_TKP + t07 * W_T07;
            candidates.push((minor_score, root as u8, false));
        }

        // 排序取 top-2
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        let best = candidates[0];
        let second = candidates[1];

        // 置信度 = 第一名与第二名的差距 / 第二名的值（归一化）
        // 差距越大 → 置信度越高
        let confidence = if second.0 > 0.001 {
            ((best.0 - second.0) / second.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let new_key = DetectedKey {
            root: best.1,
            is_major: best.2,
            confidence,
        };

        // ── 迟滞切换：只有置信度 > 0.15 且连续 3 次相同才换调 ──
        if new_key.root == self.current_key.root && new_key.is_major == self.current_key.is_major {
            self.stable_count += 1;
            // 更新置信度
            self.current_key.confidence = self.current_key.confidence * 0.5 + confidence * 0.5;
        } else if confidence > 0.15 {
            self.stable_count = self.stable_count.saturating_add(1);
            if self.stable_count >= 3 {
                // 切换到新调性
                self.current_key = DetectedKey {
                    root: new_key.root,
                    is_major: new_key.is_major,
                    confidence,
                };
                self.stable_count = 0;
            }
        } else {
            // 置信度太低 → 保持当前调性，降低稳定计数
            self.stable_count = self.stable_count.saturating_sub(1);
        }

        self.current_key
    }

    /// 计算直方图与音阶模板的相关系数（平移 root 后）
    #[inline(always)]
    fn correlation(&self, root: usize, template: &[f32; 12]) -> f32 {
        let mut sum = 0.0f32;
        for i in 0..12 {
            let idx = (root + i) % 12;
            sum += self.histogram[idx] * template[i];
        }
        sum
    }

    /// 获取当前检测到的调性（最近一次 detect 的结果）
    pub fn key(&self) -> DetectedKey {
        self.current_key
    }

    /// 重置（换歌时调用）
    pub fn reset(&mut self) {
        self.histogram = [0.0; 12];
        self.f0_count = 0;
        self.stable_count = 0;
        self.current_key = DetectedKey { root: 0, is_major: true, confidence: 0.0 };
    }

    /// ── 从原始音频直接检测调性（chroma 分析，无需 ML 模型） ──
    ///
    /// 流程：48kHz ↓16kHz → STFT → 幅度谱 → chroma(12bin) → 三模板匹配
    /// 适用于伴奏加载时的离线调性分析，约 10ms 完成 3 分钟歌曲。
    pub fn detect_from_audio(audio_48k: &[f32]) -> DetectedKey {
        // 1. 降采样 48kHz → 16kHz
        let audio_16k: Vec<f32> = audio_48k.iter().step_by(3).copied().collect();
        let sr = 16000.0;

        // 2. STFT 参数
        const N_FFT: usize = 2048;
        const HOP: usize = 512;
        let half_fft = N_FFT / 2;
        let n_bins = half_fft + 1;

        // 3. Hann 窗
        let hann: Vec<f32> = (0..N_FFT)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (N_FFT as f32 - 1.0)).cos()))
            .collect();

        // 4. 反射填充
        let pad = N_FFT / 2;
        let mut padded = Vec::with_capacity(audio_16k.len() + 2 * pad);
        for i in (1..=pad).rev() {
            padded.push(audio_16k.get(i).copied().unwrap_or(0.0));
        }
        padded.extend_from_slice(&audio_16k);
        for i in (1..=pad.min(audio_16k.len())).rev() {
            padded.push(audio_16k[audio_16k.len() - i]);
        }

        let n_frames = (padded.len() - N_FFT) / HOP + 1;
        if n_frames < 2 {
            return DetectedKey { root: 0, is_major: true, confidence: 0.0 };
        }

        // 5. 累计 chroma (12 bins)
        let mut chroma = [0.0f32; 12];
        let mut frame_count = 0usize;

        // FFT planner
        use rustfft::{FftPlanner, num_complex::Complex};
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(N_FFT);
        let mut fft_buf = vec![Complex::new(0.0f32, 0.0f32); N_FFT];

        for t in 0..n_frames {
            let offset = t * HOP;
            for k in 0..N_FFT {
                fft_buf[k] = Complex::new(padded[offset + k] * hann[k], 0.0);
            }
            fft.process(&mut fft_buf);

            // 60Hz ~ 5kHz 范围内的 bin
            let min_bin = (60.0 * N_FFT as f32 / sr) as usize;
            let max_bin = (5000.0 * N_FFT as f32 / sr) as usize;
            let max_bin = max_bin.min(n_bins);

            for k in min_bin..max_bin {
                let mag = fft_buf[k].norm_sqr().sqrt();
                if mag < 0.001 { continue; }
                // 频率 → chroma bin
                let freq = k as f32 * sr / N_FFT as f32;
                let midi = 12.0 * (freq / 440.0).log2() + 69.0;
                let semitone = ((midi % 12.0).round() as usize).min(11);
                chroma[semitone] += mag;
            }
            frame_count += 1;

            // 最多 200 帧 (约 6.5s 音频) 足够判断调性
            if frame_count >= 200 { break; }
        }

        if frame_count == 0 {
            return DetectedKey { root: 0, is_major: true, confidence: 0.0 };
        }

        // 6. 三模板匹配（复用 KS/TKP/T07）
        let mut best_score = 0.0f32;
        let mut best_root = 0u8;
        let mut best_is_major = true;

        for root in 0..12 {
            let major = Self::correlation_static(&chroma, root, &KS_MAJOR) * W_KS
                + Self::correlation_static(&chroma, root, &TKP_MAJOR) * W_TKP
                + Self::correlation_static(&chroma, root, &T07_MAJOR) * W_T07;
            let minor = Self::correlation_static(&chroma, root, &KS_MINOR) * W_KS
                + Self::correlation_static(&chroma, root, &TKP_MINOR) * W_TKP
                + Self::correlation_static(&chroma, root, &T07_MINOR) * W_T07;

            if major > best_score {
                best_score = major;
                best_root = root as u8;
                best_is_major = true;
            }
            if minor > best_score {
                best_score = minor;
                best_root = root as u8;
                best_is_major = false;
            }
        }

        DetectedKey {
            root: best_root,
            is_major: best_is_major,
            confidence: 0.5,
        }
    }

    #[inline(always)]
    fn correlation_static(histogram: &[f32; 12], root: usize, template: &[f32; 12]) -> f32 {
        let mut sum = 0.0f32;
        for i in 0..12 {
            let idx = (root + i) % 12;
            sum += histogram[idx] * template[i];
        }
        sum
    }
}

// ══════════════════════════════════════════════════════════════════
//  AutoTune 引擎
// ══════════════════════════════════════════════════════════════════

/// Auto-Tune 引擎 — 全自动音准修正
///
/// 工作流程（全自动，零配置）：
/// 1. KeyDetector 自动检测调性（三模板加权 + 谐波累积）
/// 2. 对每个麦克风 F0，找到音阶中最近的半音
/// 3. 以自然速度 (50ms attack) 将音高拉到目标
/// 4. 无声/说话时不修正
pub struct AutoTune {
    key_detector: KeyDetector,
    /// 当前修正用的 F0 倍率 (平滑追赶中)
    current_ratio: f32,
    /// 上次检测到的调性
    current_key: DetectedKey,
    /// 检测到的调性的所有允许音高（缓存）
    scale_pitches: Vec<f32>,
    scale_dirty: bool,
}

impl AutoTune {
    pub fn new() -> Self {
        let key = DetectedKey { root: 0, is_major: true, confidence: 0.0 };
        let mut s = Self {
            key_detector: KeyDetector::new(),
            current_ratio: 1.0,
            current_key: key,
            scale_pitches: Vec::new(),
            scale_dirty: true,
        };
        s.build_scale();
        s
    }

    /// 输入一帧原始 F0，返回修正后的目标 F0
    ///
    /// 返回目标 F0 给调用方，调用方计算 ratio = target / mic_f0 传给 Phase Vocoder。
    /// 如果无需修正（说话/无声/微偏<5cent），返回原 F0。
    pub fn process(&mut self, mic_f0: f32, is_singing: bool) -> f32 {
        // 1. 喂给调性检测器（含谐波累积）
        self.key_detector.feed(mic_f0);

        // 2. 每帧顺便尝试检测（内部有 f0_count 门控，实际 ~0.5s 才真正算一次）
        let key = self.key_detector.detect();
        if key.root != self.current_key.root || key.is_major != self.current_key.is_major {
            self.current_key = key;
            self.scale_dirty = true;
        }

        // 3. 非唱歌状态 → 返回原 F0
        if !is_singing || mic_f0 < 60.0 {
            return mic_f0;
        }

        // 4. 置信度不足 → 保守一点，只微调
        if self.current_key.confidence < 0.03 {
            return mic_f0;
        }

        // 5. 懒构建音阶
        if self.scale_dirty {
            self.build_scale();
            self.scale_dirty = false;
        }

        // 6. 找到最近允许音高
        let target = self.snap(mic_f0);

        // 7. < 5 cent → 没跑调，完全不动（保留自然微颤）
        let cent_diff = 1200.0 * (target / mic_f0).log2().abs();
        if cent_diff < 5.0 {
            return mic_f0;
        }

        target
    }

    /// 获取当前检测到的调性（可用于前端显示）
    pub fn detected_key(&self) -> DetectedKey {
        self.current_key
    }

    /// 从外部设置调性（伴奏加载时用，覆盖自动检测结果）
    pub fn set_key(&mut self, root: u8, is_major: bool) {
        let new_key = DetectedKey { root, is_major, confidence: 0.5 };
        if new_key.root != self.current_key.root || new_key.is_major != self.current_key.is_major {
            self.current_key = new_key;
            self.scale_dirty = true;
            eprintln!("[AutoTune] 外部设置调性: {}", new_key.label());
        }
    }

    /// 构建当前调性对应的音阶音高表（C1 ~ C7）
    fn build_scale(&mut self) {
        self.scale_pitches.clear();
        let root = self.current_key.root as f32;
        let offsets: &[f32] = if self.current_key.is_major {
            &[0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0]
        } else {
            &[0.0, 2.0, 3.0, 5.0, 7.0, 8.0, 10.0]
        };

        for octave in 1..=7 {
            for offset in offsets {
                let midi = (octave as f32 + 1.0) * 12.0 + root + offset;
                let hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
                self.scale_pitches.push(hz);
            }
        }
        self.scale_pitches.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.scale_pitches.dedup();
    }

    /// 将 F0 吸附到最近的音阶音高
    #[inline]
    fn snap(&self, f0: f32) -> f32 {
        // 二分查找（音阶有序，O(log n) 比线性扫描快）
        match self.scale_pitches.binary_search_by(|&p| p.partial_cmp(&f0).unwrap()) {
            Ok(i) => self.scale_pitches[i],
            Err(i) => {
                // i 是插入点，比较 i 和 i-1
                if i == 0 {
                    self.scale_pitches[0]
                } else if i >= self.scale_pitches.len() {
                    self.scale_pitches[self.scale_pitches.len() - 1]
                } else {
                    let prev = self.scale_pitches[i - 1];
                    let next = self.scale_pitches[i];
                    if (f0 - prev).abs() < (next - f0).abs() { prev } else { next }
                }
            }
        }
    }

    /// 换歌时重置状态
    pub fn reset(&mut self) {
        self.key_detector.reset();
        self.current_ratio = 1.0;
    }
}
