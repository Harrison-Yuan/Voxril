//! 输出级 — 伴奏 / 修音人声 / 翻唱人声 → 设备采样率 → 声卡
//!
//! 这是引擎唯一「离开音频线程」的出口，集中承担四项职责：
//! 1. **换歌 / seek 同步**：清空环形缓冲，杜绝上一首残留串音；
//! 2. **暂停语义**：暂停时只静音、不消费伴奏与翻唱数据，
//!    恢复播放时进度与音频天然对齐（旧实现暂停仍消费，恢复后进度跳变）；
//! 3. **实时安全**：音频回调内零分配、零锁，所有缓冲在构造期一次性分配；
//! 4. **单一时钟**：以「实际消耗的伴奏采样数」推进 `PLAYBACK_COUNTER`，
//!    长期零漂移；歌曲播完自动判定结束并冻结播放。
//!
//! 引擎回调因此退化为一行 `stage.fill(data)`，所有细节收敛在此模块。

use crate::audio_engine::accompaniment::{
    ACC_ENDED, ACC_PAUSED, ACC_SEEK_POSITION, ACC_SEEK_VERSION, PLAYBACK_COUNTER, SONG_VERSION,
};
use crate::audio_engine::mixer;
use crate::audio_engine::resampler::StreamResampler;
use crate::audio_engine::smoother::StreamSmoother;
use crate::audio_engine::AUDIO_SOURCE;
use ringbuf::HeapConsumer;
use std::sync::atomic::Ordering;

/// 单次回调最大处理帧数（远大于常见声卡的 1024 帧，作为预分配基准）
pub const MAX_STAGE_BLOCK: usize = 4096;

/// 输出级：持有三路消费端、混音器、重采样器与全部实时缓冲
pub struct OutputStage {
    acc: HeapConsumer<f32>,
    vocal: HeapConsumer<f32>,
    cover: HeapConsumer<f32>,

    acc_smoother: StreamSmoother,
    vocal_smoother: StreamSmoother,
    cover_smoother: StreamSmoother,

    /// `engine_sr != out_sr` 时存在；否则直通
    resampler: Option<StreamResampler>,
    out_channels: usize,

    // ── 预分配实时缓冲（构造后不再扩容，除非声卡给出超常规帧数）──
    acc_buf: Vec<f32>,
    vocal_buf: Vec<f32>,
    cover_buf: Vec<f32>,
    /// 引擎采样率的混音结果
    mixed: Vec<f32>,
    /// 设备采样率的单调输出暂存
    out_mono: Vec<f32>,

    last_song_ver: usize,
    last_seek_ver: usize,
    ended_reported: bool,
}

impl OutputStage {
    pub fn new(
        acc: HeapConsumer<f32>,
        vocal: HeapConsumer<f32>,
        cover: HeapConsumer<f32>,
        engine_sr: u32,
        out_sr: u32,
        out_channels: usize,
    ) -> Self {
        Self {
            acc,
            vocal,
            cover,
            acc_smoother: StreamSmoother::new(),
            vocal_smoother: StreamSmoother::new(),
            cover_smoother: StreamSmoother::new(),
            resampler: if engine_sr != out_sr {
                Some(StreamResampler::new(engine_sr, out_sr, 512))
            } else {
                None
            },
            out_channels,
            acc_buf: vec![0.0; MAX_STAGE_BLOCK],
            vocal_buf: vec![0.0; MAX_STAGE_BLOCK],
            cover_buf: vec![0.0; MAX_STAGE_BLOCK],
            mixed: vec![0.0; MAX_STAGE_BLOCK],
            out_mono: vec![0.0; MAX_STAGE_BLOCK],
            // 首帧必然与当前版本不同，从而执行一次完整同步
            last_song_ver: usize::MAX,
            last_seek_ver: usize::MAX,
            ended_reported: false,
        }
    }

    /// 填充设备输出缓冲（交错多声道）
    pub fn fill(&mut self, data: &mut [f32]) {
        let out_ch = self.out_channels.max(1);
        let frames = data.len() / out_ch;
        if frames == 0 {
            return;
        }
        if frames > self.out_mono.len() {
            // 非常规声卡驱动给出超大缓冲：仅首次扩容，之后不再分配
            self.acc_buf.resize(frames, 0.0);
            self.vocal_buf.resize(frames, 0.0);
            self.cover_buf.resize(frames, 0.0);
            self.mixed.resize(frames, 0.0);
            self.out_mono.resize(frames, 0.0);
        }

        self.sync_transport();

        let paused = ACC_PAUSED.load(Ordering::Relaxed);
        let cover_enabled = AUDIO_SOURCE.load(Ordering::Relaxed) == 1;

        // 临时取出重采样器，避免与 `&mut self` 的方法调用产生借用冲突
        let mut resampler = self.resampler.take();
        if let Some(ref mut r) = resampler {
            let mut filled = 0usize;
            // 兜底上限：正常情况下 2~3 轮即可填满，异常时也不会死循环
            let mut guard = 0usize;
            while filled < frames && guard <= frames {
                filled += r.drain_into(&mut self.out_mono[filled..frames]);
                if filled >= frames {
                    break;
                }
                guard += 1;
                let need = r.input_frames_next().min(self.mixed.len());
                if need == 0 {
                    break;
                }
                self.render_chunk(need, paused, cover_enabled);
                r.feed(&self.mixed[..need]);
            }
            // 未填满部分补静音
            for s in self.out_mono[filled..frames].iter_mut() {
                *s = 0.0;
            }
            write_interleaved(data, &self.out_mono[..frames], out_ch);
        } else {
            let need = frames.min(self.mixed.len());
            self.render_chunk(need, paused, cover_enabled);
            write_interleaved(data, &self.mixed[..need], out_ch);
            for f in need..frames {
                for c in 0..out_ch {
                    data[f * out_ch + c] = 0.0;
                }
            }
        }
        self.resampler = resampler;

        self.check_song_end(paused);
    }

    /// 处理换歌 / seek：清空陈旧缓冲，同步播放时钟
    fn sync_transport(&mut self) {
        let song_ver = SONG_VERSION.load(Ordering::Acquire);
        if song_ver != self.last_song_ver {
            self.last_song_ver = song_ver;
            drain(&mut self.acc);
            drain(&mut self.vocal);
            drain(&mut self.cover);
            if let Some(ref mut r) = self.resampler {
                r.reset();
            }
            PLAYBACK_COUNTER.store(0, Ordering::Release);
            self.ended_reported = false;
            // 换歌同日可能带着 seek 版本变化，避免随后重复处理
            self.last_seek_ver = ACC_SEEK_VERSION.load(Ordering::Acquire);
            return;
        }

        let seek_ver = ACC_SEEK_VERSION.load(Ordering::Acquire);
        if seek_ver != self.last_seek_ver {
            self.last_seek_ver = seek_ver;
            drain(&mut self.acc);
            drain(&mut self.cover);
            let target = ACC_SEEK_POSITION.load(Ordering::Acquire);
            if target >= 0 {
                PLAYBACK_COUNTER.store(target as usize, Ordering::Release);
            }
            self.ended_reported = false;
        }
    }

    /// 渲染 `need` 帧引擎采样到 `self.mixed[..need]`，并推进播放时钟
    fn render_chunk(&mut self, need: usize, paused: bool, cover_enabled: bool) {
        // 拆解为互不重叠的字段借用，满足混音器同时读写多路缓冲
        let Self {
            acc,
            vocal,
            cover,
            acc_smoother,
            vocal_smoother,
            cover_smoother,
            acc_buf,
            vocal_buf,
            cover_buf,
            mixed,
            ..
        } = self;

        // 暂停时不消费伴奏/翻唱，保证恢复播放后进度与音频对齐；
        // 修音人声始终消费，维持「麦克风 → 声卡」的实时通路。
        let acc_read = if paused {
            0
        } else {
            acc.pop_slice(&mut acc_buf[..need])
        };
        let cover_read = if paused {
            0
        } else {
            cover.pop_slice(&mut cover_buf[..need])
        };
        let vocal_read = vocal.pop_slice(&mut vocal_buf[..need]);

        mixer::mix_into(
            &acc_buf[..need],
            acc_read,
            paused,
            &vocal_buf[..need],
            vocal_read,
            &cover_buf[..need],
            cover_read,
            cover_enabled,
            acc_smoother,
            vocal_smoother,
            cover_smoother,
            &mut mixed[..need],
        );

        // 唯一播放时钟：只按真正被播放的伴奏采样推进，长期零漂移
        if !paused && acc_read > 0 {
            PLAYBACK_COUNTER.fetch_add(acc_read, Ordering::Release);
        }
    }

    /// 歌曲播完判定：伴奏已到底且环形缓冲排空 → 冻结播放并上报一次
    fn check_song_end(&mut self, paused: bool) {
        if self.ended_reported || paused {
            return;
        }
        if !ACC_ENDED.load(Ordering::Acquire) {
            return;
        }
        if self.acc.len() > 0 || self.cover.len() > 0 {
            return;
        }
        self.ended_reported = true;
        ACC_PAUSED.store(true, Ordering::Relaxed);
    }
}

/// 清空消费端残留数据
fn drain(cons: &mut HeapConsumer<f32>) {
    let mut scratch = [0.0f32; 512];
    while cons.pop_slice(&mut scratch) > 0 {}
}

/// 单声道 → 交错多声道
fn write_interleaved(data: &mut [f32], mono: &[f32], out_ch: usize) {
    for (f, &s) in mono.iter().enumerate() {
        let base = f * out_ch;
        for c in 0..out_ch {
            data[base + c] = s;
        }
    }
}
