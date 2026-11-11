//! 全局伴奏播放控制
//!
//! 直播引擎运行时，伴奏由一个独立的线程从 ACC_AUDIO_GLOBAL 读取并推送到 ring buffer。
//! 此模块提供动态加载/切换伴奏音频的能力。
//! 同时管理全局 LRC 歌词数据，使引擎能在运行时动态加载歌词。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::Mutex;

/// 歌曲版本号 — 每次换歌或停止时递增
/// 所有引擎线程（伴奏、翻唱、歌词、输出回调）监听此值，
/// 检测到变化立即执行 reset：pos=0、drain RingBuffer、清空歌词计数
pub static SONG_VERSION: AtomicUsize = AtomicUsize::new(0);

/// 全局歌曲播放时钟（48kHz 采样数），由引擎输出回调累加，换歌时重置
pub static PLAYBACK_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 伴奏线程当前推流位置（48kHz 采样数），由伴奏线程写入，cover 线程 seek 时读取
/// PLAYBACK_COUNTER 是输出回调消耗后的位置（滞后 ~100ms），
/// ACC_PUSH_POS 是伴奏线程实时推流位置，cover seek 到此处才能和伴奏同步
pub static ACC_PUSH_POS: AtomicUsize = AtomicUsize::new(0);

// ── 伴奏 ──

/// 当前伴奏音频数据 (48kHz 单声道 f32)
static ACC_AUDIO_GLOBAL: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// 是否需要重新加载伴奏（引擎线程检查此标志）
pub static ACC_NEED_RELOAD: AtomicBool = AtomicBool::new(false);

/// 伴奏播放暂停标志（引擎线程检查此标志）
pub static ACC_PAUSED: AtomicBool = AtomicBool::new(false);

/// 加载新伴奏音频到全局缓冲区（自动播放）
pub fn load_audio(samples: Vec<f32>) {
    if let Ok(mut guard) = ACC_AUDIO_GLOBAL.lock() {
        *guard = samples;
    }
    PLAYBACK_COUNTER.store(0, Ordering::SeqCst);
    ACC_PUSH_POS.store(0, Ordering::SeqCst);
    ACC_PAUSED.store(false, Ordering::Relaxed);
    SONG_VERSION.fetch_add(1, Ordering::SeqCst);
    ACC_NEED_RELOAD.store(true, Ordering::Relaxed);
}

/// 开始播放当前已加载的伴奏
pub fn play() {
    ACC_PAUSED.store(false, Ordering::Relaxed);
}

/// 引擎线程读取当前伴奏数据（返回克隆，避免锁争用）
pub fn get_audio() -> Vec<f32> {
    if let Ok(guard) = ACC_AUDIO_GLOBAL.lock() {
        guard.clone()
    } else {
        Vec::new()
    }
}

/// 检查当前是否有伴奏数据载入
pub fn has_audio() -> bool {
    if let Ok(guard) = ACC_AUDIO_GLOBAL.lock() {
        !guard.is_empty()
    } else {
        false
    }
}

/// 清除伴奏（停止播放）— 同时递增 SONG_VERSION 触发所有线程重置
pub fn clear() {
    if let Ok(mut guard) = ACC_AUDIO_GLOBAL.lock() {
        guard.clear();
    }
    // 重置播放时钟
    PLAYBACK_COUNTER.store(0, Ordering::SeqCst);
    ACC_PUSH_POS.store(0, Ordering::SeqCst);
    ACC_PAUSED.store(false, Ordering::Relaxed);
    // ⚠️ 递增 SONG_VERSION：引擎线程检测到后 drain buffer + 发送 stopped 事件
    SONG_VERSION.fetch_add(1, Ordering::SeqCst);
    ACC_NEED_RELOAD.store(true, Ordering::Relaxed);
}

// ── 歌词 ──

/// LRC 歌词文件路径（歌词线程从 LRC_PATH 读取文件）
/// LRC_VERSION 每次路径变化时递增，歌词线程通过版本号检测变化
/// 避免路径"先清空再设置"发生在同一 tick 内无法被检测的问题
static LRC_PATH: Mutex<String> = Mutex::new(String::new());
pub static LRC_VERSION: AtomicUsize = AtomicUsize::new(0);

/// 设置歌词文件路径（歌词线程自动加载）
pub fn set_lrc_path(path: String) {
    if let Ok(mut guard) = LRC_PATH.lock() {
        *guard = path;
    }
    LRC_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// 获取当前歌词文件路径
pub fn get_lrc_path() -> String {
    if let Ok(guard) = LRC_PATH.lock() {
        guard.clone()
    } else {
        String::new()
    }
}

/// 清除歌词
pub fn clear_lrc() {
    if let Ok(mut guard) = LRC_PATH.lock() {
        guard.clear();
    }
    LRC_VERSION.fetch_add(1, Ordering::Relaxed);
}

// ── 翻唱人声 ──

/// 当前翻唱人声音频数据 (48kHz 单声道 f32)
static COVER_AUDIO_GLOBAL: Mutex<Vec<f32>> = Mutex::new(Vec::new());

/// 是否需要重新加载翻唱人声（引擎线程检查此标志）
pub static COVER_NEED_RELOAD: AtomicBool = AtomicBool::new(false);

/// 翻唱人声播放标志
pub static COVER_PLAYING: AtomicBool = AtomicBool::new(false);

/// 加载翻唱人声音频到全局缓冲区
pub fn load_cover(samples: Vec<f32>) {
    if let Ok(mut guard) = COVER_AUDIO_GLOBAL.lock() {
        *guard = samples;
    }
    COVER_NEED_RELOAD.store(true, Ordering::Relaxed);
}

/// 开始/停止播放翻唱人声
pub fn set_cover_playing(playing: bool) {
    COVER_PLAYING.store(playing, Ordering::Relaxed);
}

/// 引擎线程读取当前翻唱人声数据
pub fn get_cover_audio() -> Vec<f32> {
    if let Ok(guard) = COVER_AUDIO_GLOBAL.lock() {
        guard.clone()
    } else {
        Vec::new()
    }
}

/// 清除翻唱人声
pub fn clear_cover() {
    if let Ok(mut guard) = COVER_AUDIO_GLOBAL.lock() {
        guard.clear();
    }
    COVER_NEED_RELOAD.store(true, Ordering::Relaxed);
    COVER_PLAYING.store(false, Ordering::Relaxed);
}

// ── 伴奏调性（由 play_accompaniment 写入，修音线程读取） ──

/// 伴奏检测到的调性根音 0=C..11=B，-1 表示未检测
pub static ACCOMPANIMENT_KEY_ROOT: AtomicI32 = AtomicI32::new(-1);
/// 伴奏检测到的调性大小调
pub static ACCOMPANIMENT_KEY_MAJOR: AtomicBool = AtomicBool::new(true);

/// 写入伴奏调性检测结果
pub fn set_accompaniment_key(root: u8, is_major: bool) {
    ACCOMPANIMENT_KEY_ROOT.store(root as i32, Ordering::Relaxed);
    ACCOMPANIMENT_KEY_MAJOR.store(is_major, Ordering::Relaxed);
}

// ── 播放进度 Seek ──
pub static ACC_SEEK_POSITION: AtomicI32 = AtomicI32::new(-1);
pub static ACC_SEEK_VERSION: AtomicUsize = AtomicUsize::new(0);
