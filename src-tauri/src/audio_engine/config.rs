//! 音频引擎统一配置
//!
//! 所有与引擎行为相关的常量集中在单一数据源，避免散落在各模块中的硬编码。
//! 基准采样率：48000 Hz（所有与时间相关的常量均以此计算）。

use std::sync::atomic::AtomicU32;

// ── 运行时可调参数 ──

/// 索引检索混合比例 (0=pure AI, 1=pure retrieval)，前端 Slider 实时控制。
pub(crate) static INDEX_RATE: AtomicU32 = AtomicU32::new(f32::to_bits(0.5));

// ── 采样率与帧尺寸 ──

/// 引擎内部基准采样率 (Hz)
pub const SAMPLE_RATE: u32 = 48000;

/// Core Chunk：核心修改——缩减为 20ms (960 @48kHz)
pub const CORE_CHUNK_SIZE: usize = 960;

/// 历史上下文：100ms 不变 (喂给 ContentVec)
pub const PAST_CONTEXT_SIZE: usize = 4800;

/// 未来上下文：40ms 不变
pub const FUTURE_CONTEXT_SIZE: usize = 1920;

/// 推理窗口总大小：history + core + future = 160ms
pub const WINDOW_SIZE: usize = PAST_CONTEXT_SIZE + CORE_CHUNK_SIZE + FUTURE_CONTEXT_SIZE;

/// 块间交叉融合宽度 (40ms Hann 窗，消除相位抵消导致的漱口声)
pub const CHUNK_CROSSFADE: usize = 1920;

/// F0 分析输出帧数（20 帧对应 200ms 窗口，对齐合成器 nf）
pub const YIN_FRAMES: usize = 20;

/// ContentVec 特征时间步长（帧长 20ms @16kHz = 320 采样）
pub const CONTENTVEC_FRAME_STEP: usize = 320;

/// ContentVec 感受野补偿采样数（7 层卷积总感受野 ~25ms）
pub const CONTENTVEC_RECEPTIVE_FIELD: usize = 400;

// ── 实时门控 ──

/// 硬静音 RMS 阈值 (-46dBFS)，屏蔽声卡底噪（-60~-50dBFS）
pub const HARD_SILENCE_RMS: f32 = 0.005;

/// 静音滞后块数：低于静音阈值后最多保留 N 块仍做推理
pub const SILENCE_HANGOVER_CHUNKS: usize = 3;

/// 最小人声比例：YIN 帧中有周期性的比例 ≥ 此值才执行推理
pub const MIN_VOICED_RATIO: f32 = 0.10;

/// 每块推理预算 (ms)，超时计为 over-budget
pub const HOP_BUDGET_MS: f64 = 100.0;

// ── 环形缓冲区 ──

/// 输入环形缓冲区容量（麦克风采集）
pub const INPUT_RING_CAPACITY: usize = 32768;

/// 输出环形缓冲区容量（变声后音频）
pub const OUTPUT_RING_CAPACITY: usize = 65536;

/// 伴奏环形缓冲区容量
pub const ACC_RING_CAPACITY: usize = 131072;

/// 人声预填充采样数 — Phase Vocoder 不需要预填，设为 0
pub const VOCAL_PREFILL_SAMPLES: usize = 0;

/// ⚠️ 最大 vocal ring buffer 消费延迟（采样数）。
/// 输出回调每次处理前检查 buffer 水位，超过此值则跳过最旧数据。
/// 4800 采样 @48kHz = 100ms。即使用户听 100ms 前的音频也是可接受的实时感。
/// 太小（< 2400）容易在 CPU 抖动时 underrun，太大（> 9600）延迟明显。
pub const MAX_VOCAL_LATENCY_SAMPLES: usize = 4800;

/// 伴奏预填充采样数 (~85ms @48kHz)
pub const ACC_PREFILL_SAMPLES: usize = 4096;

/// 输入缓冲上限：恢复正常大小
pub const MAX_LOCAL_INPUT_SAMPLES: usize = 43200; // (从 96000 改回 43200)

// ── 输出交叉淡入淡出 ──

/// 混合器交叉淡入淡出采样数 (10.7ms @48kHz)
pub const MIXER_FADE_SAMPLES: usize = 512;

// ── IVF 检索 ──

/// IVF 搜索最近邻数
pub const IVF_K: usize = 8;

/// IVF 搜索 nprobe（最近质心数）
pub const IVF_NPROBE: usize = 3;

// ── YIN F0 提取 ──

/// YIN 宽松阈值：主阈值未通过时使用的兜底阈值
pub const YIN_RELAXED_THRESHOLD: f64 = 0.50;

/// F0 平滑窗口（5 帧 ≈ 50ms，增强对长元音微小抖动的过滤）
pub const F0_SMOOTH_WINDOW: usize = 5;
