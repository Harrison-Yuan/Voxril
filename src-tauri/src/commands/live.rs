use crate::audio_engine::{self, LiveSingingConfig};
use crate::state::{AppState, LivePhase};
use crate::workspace_layout::{OutputKind, WorkspaceLayout};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{Emitter, State};

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveSingingRequest {
    pub input_device: String,
    pub output_device: String,
    pub f0_up_key: i32,
    pub accompaniment_path: String,
    pub cover_path: String,
    /// 歌词文件路径（空串表示该歌曲没有歌词）
    pub lrc_path: String,
}

/// 分离歌曲条目（供直播页面选择）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeparatedSong {
    pub name: String,                      // 歌曲名
    pub instrumental_path: String,         // 伴奏路径
    pub vocal_path: String,                // 人声路径
    pub lrc_exists: bool,                  // 歌词是否已提取
    pub lrc_path: String,                  // 歌词路径（可直接读取）
    pub converted_path: String,            // 已转换的人声路径（RVC 合成结果）
}

/// 从文件名中去掉常见前缀，提取歌曲名
fn extract_song_name(filename: &str) -> &str {
    // 按顺序尝试各前缀，取第一个匹配的
    for prefix in &["instrumental_", "instrument_", "vocal_", "Vocals_"] {
        if let Some(rest) = filename.strip_prefix(prefix) {
            return rest;
        }
    }
    filename
}

/// 列出已分离的歌曲（伴奏和人声均存在才展示）
#[tauri::command]
pub async fn list_separated_songs(state: State<'_, AppState>) -> Result<Vec<SeparatedSong>, String> {
    let workspace = state.workspace_root.read().await;
    let datasets_dir = WorkspaceLayout::detect(&*workspace).datasets_root();
    let ins_dir = datasets_dir.join("instrumental");
    let vocal_dir = datasets_dir.join("vocals");

    if !ins_dir.is_dir() || !vocal_dir.is_dir() {
        eprintln!("[list_separated_songs] 目录不存在: ins={} vocal={}", ins_dir.display(), vocal_dir.display());
        return Ok(vec![]);
    }

    // 允许的音频扩展名
    let is_audio = |p: &std::path::Path| -> bool {
        let exts = ["wav", "flac", "mp3", "ogg", "m4a", "wma"];
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| exts.contains(&e))
            .unwrap_or(false)
    };

    // 扫描 vocal_dir → 歌名列表
    let mut vocal_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(entries) = std::fs::read_dir(&vocal_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_audio(&path) { continue; }
            let fname = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let name = extract_song_name(fname).to_string();
            if !name.is_empty() {
                vocal_names.insert(name);
            }
        }
    }
    eprintln!("[list_separated_songs] 人声文件数: {}", vocal_names.len());

    // 扫描 instrumental_dir，与 vocal_names 取交集
    let mut songs: Vec<SeparatedSong> = Vec::new();
    let mut found_count = 0u32;
    if let Ok(entries) = std::fs::read_dir(&ins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_audio(&path) { continue; }
            let fname = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let song_name = extract_song_name(fname).to_string();
            if song_name.is_empty() { continue; }
            if !vocal_names.contains(&song_name) { continue; }
            found_count += 1;

            // 找对应人声（可能扩展名不同）
            let vocal_path = vocal_dir.join(format!("vocal_{}.flac", song_name));
            let vocal_path = if vocal_path.exists() { vocal_path }
            else {
                vocal_dir.join(format!("vocal_{}.wav", song_name))
            };

            // 查找已转换的人声（converted/ 目录）
            // 转换输出格式: {歌名}_{音色}_{批次}_key{转调}_{时分}.wav，歌名已去掉 vocal_ 前缀，
            // 因此文件名一定包含 {歌名} 子串
            let converted_dir = WorkspaceLayout::detect(&*workspace).output_dir(OutputKind::Vocal);
            let converted_path = if let Ok(entries) = std::fs::read_dir(&converted_dir) {
                // 对每个 wav 文件检查 stem 是否包含歌名
                // 不依赖 regex，直接 contains 匹配更可靠
                let mut matched: Vec<_> = entries.flatten()
                    .filter(|e| e.path().is_file())
                    .filter(|e| {
                        let p = e.path();
                        p.extension().and_then(|s| s.to_str()) == Some("wav")
                    })
                    .filter(|e| {
                        let p = e.path();
                        let stem = p.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        stem.contains(&song_name)
                    })
                    .collect();
                // 按修改时间排序，取最新文件
                matched.sort_by(|a, b| {
                    let am = a.metadata().and_then(|m| m.modified()).ok();
                    let bm = b.metadata().and_then(|m| m.modified()).ok();
                    am.cmp(&bm).reverse()
                });
                match matched.into_iter().next() {
                    Some(e) => {
                        let cpath = e.path().to_string_lossy().to_string();
                        eprintln!("[list_separated_songs] ✅ 匹配到翻唱: {} (song={})", cpath, song_name);
                        cpath
                    }
                    None => {
                        eprintln!("[list_separated_songs] ⚠️ converted/ 目录无匹配: song={}", song_name);
                        String::new()
                    }
                }
            } else {
                eprintln!("[list_separated_songs] ⚠️ converted/ 目录不存在: {}", converted_dir.display());
                String::new()
            };

            let lrc_path = vocal_dir.join(format!("vocal_{}.lrc", song_name));
            let lrc_exists = lrc_path.exists();
            let lrc_path_str = lrc_path.to_string_lossy().to_string();
            songs.push(SeparatedSong {
                name: song_name.clone(),
                instrumental_path: path.to_string_lossy().to_string(),
                vocal_path: vocal_path.to_string_lossy().to_string(),
                lrc_exists,
                lrc_path: lrc_path_str,
                converted_path,
            });
        }
    }
    eprintln!("[list_separated_songs] 匹配歌曲数: {}", found_count);

    songs.sort_by(|a, b| a.name.cmp(&b.name));
    drop(workspace);
    Ok(songs)
}

#[tauri::command]
pub async fn start_live_singing(
    request: LiveSingingRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    eprintln!("[RUST] start_live_singing 命令被调用");

    // 状态机仲裁：仅 Idle 可启动，避免两个引擎实例同时操作 CoreAudio
    {
        let phase_now = *state.live_singing_phase.read().await;
        match phase_now {
            LivePhase::Idle => {}
            LivePhase::Stopping => {
                return Err("直播引擎正在停止，请稍后重试".into());
            }
            LivePhase::Starting | LivePhase::Running => {
                return Err("直播唱歌引擎已在运行中".into());
            }
        }
    }
    // 清理可能的残留停止信号（Idle 时理论上为空）
    let _ = state.live_singing_stop.lock().await.take();
    *state.live_singing_phase.write().await = LivePhase::Starting;

    let data_dir = state.data_dir.clone();

    let rmvpe_path = {
        let candidates = vec![
            data_dir.join("assets/rmvpe/rmvpe.onnx"),
            data_dir.join("assets/pretrained/rmvpe.onnx"),
        ];
        candidates.iter().find(|p| p.exists()).cloned()
            .unwrap_or_else(|| data_dir.join("assets/rmvpe/rmvpe.onnx"))
            .to_string_lossy().to_string()
    };

    let fcpe_path = {
        let candidates = vec![
            data_dir.join("assets/fcpe/fcpe.onnx"),
        ];
        candidates.iter().find(|p| p.exists()).cloned()
            .unwrap_or_else(|| data_dir.join("assets/fcpe/fcpe.onnx"))
            .to_string_lossy().to_string()
    };

    let config = LiveSingingConfig {
        input_device: request.input_device,
        output_device: request.output_device,
        rmvpe_path,
        fcpe_path,
        lrc_path: request.lrc_path,
        f0_up_key: request.f0_up_key,
        accompaniment_path: request.accompaniment_path,
        cover_path: request.cover_path,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    *state.live_singing_stop.lock().await = Some(stop_tx);

    let phase = state.live_singing_phase.clone();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        eprintln!("[RUST] 引擎线程已启动，停止独立伴奏输出流...");
        // ⚠️ 先停止独立伴奏输出流，再用引擎接管
        crate::audio_engine::accompaniment::stop_standalone_output();
        std::thread::sleep(std::time::Duration::from_millis(200)); // 等 stream 释放

        eprintln!("[RUST] 开始调用 start_live_engine...");
        let result = audio_engine::engine::start_live_engine(
            config,
            app_handle.clone(),
            phase.clone(),
            stop_rx,
        );

        // 线程退出：无论成功失败都回落 Idle，并广播结果
        tauri::async_runtime::block_on(async {
            *phase.write().await = LivePhase::Idle;
        });
        match result {
            Ok(_) => {
                let _ = app_handle.emit("live-singing-stopped", "引擎已停止");
            }
            Err(e) => {
                eprintln!("[RUST] 引擎启动失败: {}", e);
                let _ = app_handle.emit("live-singing-error", format!("引擎启动失败: {}", e));
            }
        }
    });

    Ok(())
}

/// 直播中实时调整 F0 音高转调量（半音数）。
/// 前端 Slider 拖动时调用，引擎即时响应，无需下播重连。
#[tauri::command]
pub async fn set_live_f0_up_key(semitones: i32) -> Result<(), String> {
    crate::audio_engine::F0_UP_KEY.store(semitones, std::sync::atomic::Ordering::Relaxed);
    eprintln!("[SET] f0_up_key = {} (半音)", semitones);
    Ok(())
}

/// 设置音源模式：0=真人演唱，1=AI 翻唱
/// 内部自动处理 MIC_MUTED 和 cover audible
#[tauri::command]
pub async fn set_audio_source(source: u32) -> Result<(), String> {
    let source = if source == 0 { 0 } else { 1 };
    crate::audio_engine::AUDIO_SOURCE.store(source, std::sync::atomic::Ordering::Relaxed);
    crate::audio_engine::MIC_MUTED.store(source == 1, std::sync::atomic::Ordering::Relaxed);
    // ⚠️ COVER_PLAYING 与 AUDIO_SOURCE 对齐，确保独立输出流正确混入 cover
    crate::audio_engine::accompaniment::set_cover_playing(source == 1);
    eprintln!("[SET] audio_source = {} (真人={}, cover={}, mic_muted={})", source, source == 0, source == 1, source == 1);
    Ok(())
}

#[tauri::command]
pub async fn stop_live_singing(state: State<'_, AppState>) -> Result<(), String> {
    // 幂等：已 Idle 则仅清理残留信号
    {
        let phase_now = *state.live_singing_phase.read().await;
        if phase_now == LivePhase::Idle {
            let _ = state.live_singing_stop.lock().await.take();
            converge_playback();
            return Ok(());
        }
        if phase_now != LivePhase::Stopping {
            *state.live_singing_phase.write().await = LivePhase::Stopping;
        }
    }

    if let Some(stop_tx) = state.live_singing_stop.lock().await.take() {
        let _ = stop_tx.send(());
    }

    // 等待引擎线程真正退出（最多 6 秒）
    for _ in 0..200 {
        if *state.live_singing_phase.read().await == LivePhase::Idle {
            converge_playback();
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    Err("正在停止直播引擎，请稍后再试".into())
}

/// 下播收敛：停止独立伴奏输出并复位播放标志，
/// 避免残留的 paused/ended 状态影响下次开播。
fn converge_playback() {
    use std::sync::atomic::Ordering;
    crate::audio_engine::accompaniment::stop_standalone_output();
    crate::audio_engine::accompaniment::ACC_PAUSED.store(false, Ordering::Relaxed);
    crate::audio_engine::accompaniment::ACC_ENDED.store(false, Ordering::Relaxed);
}

/// 暂停/恢复伴奏播放
#[tauri::command]
pub async fn set_accompaniment_pause(paused: bool, app: tauri::AppHandle) -> Result<(), String> {
    crate::audio_engine::accompaniment::ACC_PAUSED.store(paused, std::sync::atomic::Ordering::Relaxed);
    let status = if paused { "paused" } else { "playing" };
    let _ = app.emit("player-state", serde_json::json!({
        "status": status,
    }));
    Ok(())
}

/// 获取伴奏播放状态
#[tauri::command]
pub async fn get_accompaniment_status() -> Result<bool, String> {
    Ok(crate::audio_engine::accompaniment::ACC_PAUSED.load(std::sync::atomic::Ordering::Relaxed))
}

/// 动态加载并播放伴奏（直播中调用，无需重启引擎）
#[tauri::command]
pub async fn play_accompaniment(audio_path: String, lrc_path: Option<String>, cover_path: Option<String>, app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    use std::io::BufReader;

    let src = std::path::Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("音频文件不存在: {}", audio_path));
    }

    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    if ext != "wav" && ext != "flac" {
        return Err("仅支持 WAV/FLAC 格式".to_string());
    }

    let file = std::fs::File::open(src).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut reader = hound::WavReader::new(BufReader::new(file))
        .map_err(|e| format!("读取 WAV 失败: {}", e))?;

    let sr = reader.spec().sample_rate;
    let channels = reader.spec().channels as usize;

    let mut acc_buf: Vec<f32> = Vec::new();
    while let Some(Ok(s)) = reader.samples::<i16>().next() {
        acc_buf.push(s as f32 / 32768.0);
    }

    // 多声道混合
    let mono: Vec<f32> = if channels > 1 {
        acc_buf.chunks(channels).map(|ch| (ch.iter().sum::<f32>() * 1.5).clamp(-1.0, 1.0)).collect()
    } else { acc_buf };

    // 重采样到 48kHz
    let resampled: Vec<f32> = if sr != 48000 {
        let ratio = 48000.0 / sr as f64;
        let olen = (mono.len() as f64 * ratio) as usize;
        (0..olen).map(|i| {
            let sp = i as f64 / ratio;
            let si = sp as usize;
            let fr = (sp - si as f64) as f32;
            if si + 1 < mono.len() { mono[si] * (1.0 - fr) + mono[si + 1] * fr }
            else if si < mono.len() { mono[si] }
            else { *mono.last().unwrap_or(&0.0) }
        }).collect()
    } else { mono };

    let dur = resampled.len() as f64 / 48000.0;
    let _ = app.emit("live-singing-log", format!("[伴奏] 加载: {:.1}s, {}Hz {}ch", dur, sr, channels));

    // ── 检测伴奏调性（chroma 分析，10ms 完成） ──
    let acc_key = crate::audio_engine::auto_tune::KeyDetector::detect_from_audio(&resampled);
    let _ = app.emit("live-singing-log", format!("[调性] 伴奏: {}", acc_key.label()));
    let _ = app.emit("accompaniment-key", serde_json::json!({
        "key": acc_key.label(),
        "root": acc_key.root,
        "is_major": acc_key.is_major,
        "confidence": acc_key.confidence,
    }));
    // ⚠️ 写入全局原子，修音线程自动读取并设置 AutoTune
    crate::audio_engine::accompaniment::set_accompaniment_key(acc_key.root, acc_key.is_major);

    // 重置全局播放时钟（防止歌词跳到末尾）
    crate::audio_engine::accompaniment::PLAYBACK_COUNTER.store(0, std::sync::atomic::Ordering::SeqCst);

    // 写入全局缓冲，通知引擎线程切换
    crate::audio_engine::accompaniment::load_audio(resampled);

    // 同时加载歌词（优先使用传入的 lrc_path，其次从伴奏路径推导）
    let resolved_lrc = lrc_path.or_else(|| {
        // 没有歌词时从伴奏文件路径名推导（兼容旧前端）
        let fname = std::path::Path::new(&audio_path).file_stem()
            .and_then(|s| s.to_str()).unwrap_or("").to_string();
        let vocal_name = fname
            .replacen("instrumental_", "vocal_", 1)
            .replacen("instrument_", "vocal_", 1)
            .replacen("instrument", "vocal", 1);
        let parent = std::path::Path::new(&audio_path).parent()
            .unwrap_or(std::path::Path::new(""));
        let candidate1 = parent.join(format!("{}.lrc", vocal_name));
        if candidate1.exists() { return Some(candidate1.to_string_lossy().to_string()); }
        let vocals_dir = parent.to_string_lossy().to_string()
            .replacen("instrumental", "vocals", 1)
            .replacen("instrument", "vocals", 1);
        let candidate2 = std::path::Path::new(&vocals_dir).join(format!("{}.lrc", vocal_name));
        if candidate2.exists() { Some(candidate2.to_string_lossy().to_string()) } else { None }
    });

    if let Some(ref path) = resolved_lrc {
        let _ = app.emit("live-singing-log", format!("[歌词] 设置路径: {}", path));
        crate::audio_engine::accompaniment::set_lrc_path(path.clone());
    } else {
        let _ = app.emit("live-singing-log", "[歌词] 未找到 LRC 文件");
        crate::audio_engine::accompaniment::clear_lrc();
    }

    // 翻唱人声可选：预解码加载，cover 线程自动同步推流
    if let Some(ref cover_path) = cover_path {
        if std::path::Path::new(cover_path).exists() {
            match crate::audio_engine::common_decode_cover(cover_path, &app) {
                Ok(samples) => {
                    crate::audio_engine::accompaniment::load_cover(samples);
                    let _ = app.emit("live-singing-log", format!("[翻唱] 预加载: {}", cover_path));
                }
                Err(e) => {
                    let _ = app.emit("live-singing-log", format!("[翻唱] 解码失败: {}", e));
                }
            }
        }
    }

    // 存储当前歌曲路径信息（用于同步到桌面歌词窗口）
    let lrc_path_str = resolved_lrc.as_deref().unwrap_or("");
    let cover_path_str = cover_path.as_deref().unwrap_or("");
    crate::audio_engine::accompaniment::set_current_song_paths(&audio_path, lrc_path_str, cover_path_str);

    // ⚠️ play_accompaniment 不直接 emit player-state
    // load_audio() 递增 SONG_VERSION，输出回调 drain buffer + reset counter
    // 歌词线程下一 tick 检测到 has_audio()=true，自动发送完整 player-state
    // 确保歌词线程已启动（独立生命周期，无需引擎运行）
    crate::audio_engine::engine::spawn_lyrics_thread(&app);

    // 如果引擎未运行（Idle），启动独立伴奏输出流
    if *state.live_singing_phase.read().await == LivePhase::Idle {
        crate::audio_engine::accompaniment::start_standalone_output();
    }

    Ok(())
}

/// 停止当前伴奏播放（也清除翻唱人声）
#[tauri::command]
pub async fn stop_accompaniment(_app: tauri::AppHandle) -> Result<(), String> {
    crate::audio_engine::accompaniment::clear();
    crate::audio_engine::accompaniment::clear_lrc();
    crate::audio_engine::accompaniment::clear_cover();
    crate::audio_engine::accompaniment::ACCOMPANIMENT_KEY_ROOT.store(-1, std::sync::atomic::Ordering::Relaxed);
    crate::audio_engine::AUDIO_SOURCE.store(0, std::sync::atomic::Ordering::Relaxed);
    crate::audio_engine::MIC_MUTED.store(false, std::sync::atomic::Ordering::Relaxed);
    // ⚠️ 不再直接 emit player-state：
    // clear() 递增 SONG_VERSION + 清空 ACC_AUDIO_GLOBAL
    // 输出回调 drain buffer + reset counter（~21ms）
    // 歌词线程下一 tick 检测到 has_audio()=false → status=stopped → emit
    // 停止独立伴奏输出流（引擎未运行时）
    crate::audio_engine::accompaniment::stop_standalone_output();
    Ok(())
}

/// 加载翻唱人声并开始播放（自动静音麦克风）
/// 已废弃，改用 preload_cover + set_audio_source(1)
#[tauri::command]
pub async fn play_cover(audio_path: String, app: tauri::AppHandle) -> Result<(), String> {
    let src = std::path::Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("翻唱文件不存在: {}", audio_path));
    }
    let samples = crate::audio_engine::common_decode_cover(&audio_path, &app)?;
    crate::audio_engine::accompaniment::load_cover(samples);
    Ok(())
}

/// 预加载翻唱人声（不播放，用于随伴奏同时加载保持同步）
#[tauri::command]
pub async fn preload_cover(audio_path: String, app: tauri::AppHandle) -> Result<(), String> {
    let src = std::path::Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("翻唱文件不存在: {}", audio_path));
    }
    let samples = crate::audio_engine::common_decode_cover(&audio_path, &app)?;
    crate::audio_engine::accompaniment::load_cover(samples);
    // 不设 COVER_PLAYING 和 MIC_MUTED，仅解码加载
    // cover 线程会从 pos=0 开始和伴奏同步推流
    let _ = app.emit("live-singing-log", format!("[翻唱] 预加载: {}", audio_path));
    Ok(())
}

/// 停止翻唱人声
#[tauri::command]
pub async fn stop_cover(app: tauri::AppHandle) -> Result<(), String> {
    crate::audio_engine::accompaniment::set_cover_playing(false);
    crate::audio_engine::accompaniment::clear_cover();
    let _ = app.emit("live-singing-log", "[翻唱] 已停止");
    Ok(())
}

/// 播放当前已加载的伴奏
#[tauri::command]
pub async fn play_current_accompaniment(app: tauri::AppHandle) -> Result<(), String> {
    crate::audio_engine::accompaniment::play();
    let _ = app.emit("player-state", serde_json::json!({
        "status": "playing",
    }));
    Ok(())
}

/// 获取当前播放状态（用于桌面歌词等子窗口挂载时获取初始状态）
#[tauri::command]
pub async fn get_player_state() -> Result<serde_json::Value, String> {
    use std::sync::atomic::Ordering;
    let has_audio = crate::audio_engine::accompaniment::has_audio();
    let acc_paused = crate::audio_engine::accompaniment::ACC_PAUSED.load(Ordering::Relaxed);
    let source = crate::audio_engine::AUDIO_SOURCE.load(Ordering::Relaxed);
    let pb = crate::audio_engine::accompaniment::PLAYBACK_COUNTER.load(Ordering::SeqCst);

    let status = if !has_audio { "stopped" } else if acc_paused { "paused" } else { "playing" };

    Ok(serde_json::json!({
        "status": status,
        "audio_source": source,
        "progress_secs": pb as f64 / 48000.0,
        "instrumental_path": crate::audio_engine::accompaniment::get_current_instrumental_path(),
        "lrc_path": crate::audio_engine::accompaniment::get_current_lrc_path(),
        "cover_path": crate::audio_engine::accompaniment::get_current_cover_path(),
    }))
}

/// 跳转伴奏播放进度到指定秒数
#[tauri::command]
pub async fn seek_accompaniment(position_secs: f64, app: tauri::AppHandle) -> Result<(), String> {
    let sample_pos = (position_secs * 48000.0) as i32;
    crate::audio_engine::accompaniment::ACC_SEEK_POSITION.store(sample_pos, std::sync::atomic::Ordering::Relaxed);
    crate::audio_engine::accompaniment::ACC_SEEK_VERSION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // 也预写入全局计数器，让状态推送事件能立即返回正确进度（在输出回调更新之前的短暂空窗）
    crate::audio_engine::accompaniment::PLAYBACK_COUNTER.store(sample_pos as usize, std::sync::atomic::Ordering::SeqCst);

    // 广播最新的播放状态事件，通知前端更新进度
    let acc_paused = crate::audio_engine::accompaniment::ACC_PAUSED.load(std::sync::atomic::Ordering::Relaxed);
    let status = if acc_paused { "paused" } else { "playing" };
    let source = crate::audio_engine::AUDIO_SOURCE.load(std::sync::atomic::Ordering::Relaxed);
    let has_audio = crate::audio_engine::accompaniment::has_audio();

    let _ = app.emit("player-state", serde_json::json!({
        "status": status,
        "loaded": has_audio,
        "audio_source": source,
        "progress_secs": position_secs,
    }));

    Ok(())
}

#[tauri::command]
pub async fn get_live_singing_status(state: State<'_, AppState>) -> Result<bool, String> {
    let phase = *state.live_singing_phase.read().await;
    Ok(phase == LivePhase::Running)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub format: String,
    pub exists: bool,
}

#[tauri::command]
pub async fn list_live_models(state: State<'_, AppState>) -> Result<Vec<ModelEntry>, String> {
    let mut models = Vec::new();

    let _project_root = state.project_root.clone();
    let data_dir = state.data_dir.clone();
    let workspace_root = state.workspace_root.read().await;

    let exp_root = WorkspaceLayout::detect(&*workspace_root).assets_root();

    // 1. 扫描 ContentVec / HuBERT 特征模型 (.onnx)
    for dir in &[data_dir.join("assets/hubert")] {
        scan_models(dir, dir, "特征模型", &["onnx"], &mut models);
    }

    // 2. 扫描 RVC 音色 ONNX 模型（pretrained 用简短名；experiments 用层次名）
    let pretrained_base = data_dir.join("assets/pretrained");
    let pretrained_v2_base = data_dir.join("assets/pretrained_v2");
    scan_models(&pretrained_base, &pretrained_base, "音色模型", &["onnx"], &mut models);
    scan_models(&pretrained_v2_base, &pretrained_v2_base, "音色模型", &["onnx"], &mut models);
    if exp_root.is_dir() {
        scan_models(&exp_root, &exp_root, "音色模型", &["onnx"], &mut models);
    }

    // 3. 扫描实验目录中的 .pth 模型（用户训练的模型）
    if exp_root.is_dir() {
        let _ = walk_pth_models(&exp_root, &exp_root, &mut models);
    }

    // 4. 如果没有任何模型，添加 vec-768-layer-12 占位
    if !models.iter().any(|m| m.kind == "特征模型") {
        let vec_768_path = data_dir.join("assets/hubert/vec-768-layer-12.onnx");
        models.push(ModelEntry {
            name: "vec-768-layer-12".into(),
            path: vec_768_path.to_string_lossy().to_string(),
            kind: "特征模型".into(),
            format: "onnx".into(),
            exists: vec_768_path.exists(),
        });
    }

    models.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    drop(workspace_root);
    Ok(models)
}

/// 递归扫描模型文件。base_dir 用于生成带路径层次的可读名称：
/// - 文件在 base_dir 的直接子目录 → `子目录名/文件名`
/// - 文件在 base_dir 下两层以上 → `父目录链/文件名`
/// - 文件直接在 base_dir 中 → `文件名`
fn scan_models(base_dir: &Path, dir: &Path, kind: &str, exts: &[&str], models: &mut Vec<ModelEntry>) {
    if !dir.is_dir() {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_models(base_dir, &path, kind, exts, models);
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if exts.contains(&ext) {
                    let name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    // 生成带路径层次的名字，方便区分不同实验来源
                    let display_name = if let Ok(rel) = path.strip_prefix(base_dir) {
                        let rel_str = rel.to_string_lossy();
                        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        if rel_str == stem || rel.parent().map_or(true, |p| p.as_os_str().is_empty()) {
                            // 直接在 base_dir 下，只用文件名
                            name
                        } else {
                            // 有目录层次 → 实验名/版本/文件名
                            let parent = rel.parent().unwrap_or(rel);
                            format!("{}/{}", parent.to_string_lossy(), stem)
                        }
                    } else {
                        name
                    };
                    let full_path = path.to_string_lossy().to_string();
                    if !models.iter().any(|m| m.path == full_path) {
                        models.push(ModelEntry {
                            name: display_name,
                            path: full_path,
                            kind: kind.to_string(),
                            format: ext.to_string(),
                            exists: true,
                        });
                    }
                }
            }
        }
    }
}

fn walk_pth_models(base_dir: &Path, dir: &Path, models: &mut Vec<ModelEntry>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = walk_pth_models(base_dir, &path, models);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "pth" {
                let fname = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if fname.starts_with("D_") {
                    continue;
                }
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(fname);
                // 从 base_dir 生成层次名
                let display_name = if let Ok(rel) = path.strip_prefix(base_dir) {
                    let parent = rel.parent().unwrap_or(rel);
                    if parent.as_os_str().is_empty() || parent.to_string_lossy() == stem {
                        stem.to_string()
                    } else {
                        format!("{}/{}", parent.to_string_lossy(), stem)
                    }
                } else {
                    stem.to_string()
                };
                let full_path = path.to_string_lossy().to_string();
                if !models.iter().any(|m| m.path == full_path) {
                    models.push(ModelEntry {
                        name: display_name,
                        path: full_path,
                        kind: "音色模型 (待导出)".into(),
                        format: "pth".into(),
                        exists: true,
                    });
                }
            }
        }
    }
    Ok(())
}

/// 将 .pth 模型导出为 ONNX。
/// index 文件保持在训练生成的实验根目录，通过 parent/checkpoints 回退发现。
#[tauri::command]
pub async fn export_pth_to_onnx(
    source_path: String,
    output_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if !Path::new(&source_path).exists() {
        return Err(format!("模型文件不存在: {}", source_path));
    }

    let python_cmd = state.python_cmd.clone();
    let project_root = state.project_root.clone();
    let data_dir = state.data_dir.clone();

    let python_code = format!(
        r#"
import sys, os
os.environ['HUBERT_MODEL_PATH'] = r'{}/assets/hubert/vec-768-layer-12.onnx'
sys.argv = ['export']
sys.path.insert(0, r'{}')
from infer.modules.onnx.export import export_onnx
export_onnx(r'{}', r'{}')
print("[DONE] 导出完成")
"#,
        data_dir.to_string_lossy(),
        project_root.to_string_lossy(),
        source_path,
        output_path
    );

    let output = std::process::Command::new(&python_cmd)
        .arg("-c")
        .arg(&python_code)
        .current_dir(&project_root)
        .output()
        .map_err(|e| format!("执行导出脚本失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("导出失败: {}", stderr));
    }

    if !Path::new(&output_path).exists() {
        return Err("导出完成但输出文件不存在".into());
    }

    Ok(())
}

/// 歌词提取结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LyricsPayload {
    pub lrc_path: String,
    pub line_count: usize,
}

/// 歌词搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongSearchResult {
    pub id: i64,
    pub name: String,
    pub artist: String,
}

/// 在线搜索歌曲：返回搜索结果列表供用户选择
#[tauri::command]
pub async fn search_lyrics(query: String) -> Result<Vec<SongSearchResult>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .cookie_store(true)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    // 先访问首页获取 cookie
    let _ = client.get("https://music.163.com/")
        .header("Referer", "https://music.163.com/")
        .send()
        .await
        .map_err(|e| format!("获取 cookie 失败: {}", e))?;

    let params = [
        ("s", query.as_str()),
        ("type", "1"),
        ("offset", "0"),
        ("limit", "10"),
    ];

    let resp = client.post("https://music.163.com/api/search/get/web")
        .header("Referer", "https://music.163.com/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("搜索请求失败: {}", e))?;

    let json: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;

    let songs_array = json.get("result")
        .and_then(|res| res.get("songs"))
        .and_then(|songs| songs.as_array())
        .ok_or_else(|| {
            let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            if code == -460 {
                "触发网易云反爬机制，请稍后再试".to_string()
            } else {
                format!("未找到与「{}」相关的歌曲", query)
            }
        })?;

    let mut results = Vec::new();
    for song in songs_array {
        let id = song.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        if id == 0 { continue; }
        let name = song.get("name").and_then(|v| v.as_str()).unwrap_or("未知歌曲").to_string();
        let artist = song.get("artists")
            .and_then(|a| a.as_array())
            .and_then(|arr| arr.first())
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知歌手")
            .to_string();
        results.push(SongSearchResult { id, name, artist });
    }

    if results.is_empty() {
        return Err(format!("未找到与「{}」相关的歌曲", query));
    }

    Ok(results)
}

/// 根据选中的 song_id 下载 LRC 歌词到指定音频文件同目录
#[tauri::command]
pub async fn download_lyrics(
    song_id: i64,
    audio_path: String,
    app: tauri::AppHandle,
) -> Result<LyricsPayload, String> {
    let src = Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("音频文件不存在: {}", audio_path));
    }

    let filename = src.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let song_name = filename
        .strip_prefix("vocal_")
        .or_else(|| filename.strip_prefix("instrumental_"))
        .or_else(|| filename.strip_prefix("instrument_"))
        .or_else(|| filename.strip_prefix("Vocals_"))
        .unwrap_or(&filename)
        .to_string();

    let lrc_path = src.parent().unwrap_or(Path::new("")).join(format!("vocal_{}.lrc", song_name));
    let lrc_str = lrc_path.to_string_lossy().to_string();

    let _ = app.emit("live-singing-log", format!("[歌词] 正在下载: ID={}", song_id));
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 30, "message": "正在下载歌词..."}));

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .cookie_store(true)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let _ = client.get("https://music.163.com/")
        .header("Referer", "https://music.163.com/")
        .send()
        .await.ok();

    let id_str = song_id.to_string();
    let lyric_params = [
        ("id", id_str.as_str()),
        ("lv", "-1"),
        ("kv", "-1"),
        ("tv", "-1"),
        ("os", "pc"),
    ];

    let resp = client.post("https://music.163.com/api/song/lyric")
        .header("Referer", "https://music.163.com/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&lyric_params)
        .send()
        .await
        .map_err(|e| format!("歌词请求失败: {}", e))?;

    let mut json: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析歌词结果失败: {}", e))?;

    // 回退新 API
    let code = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 200 {
        if let Ok(resp2) = client.post("https://music.163.com/api/song/lyric/new")
            .header("Referer", "https://music.163.com/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&lyric_params)
            .send()
            .await
        {
            if let Ok(j2) = resp2.json::<serde_json::Value>().await {
                if j2.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) == 200 {
                    json = j2;
                }
            }
        }
    }

    let raw_lrc = json.get("lrc").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");
    let raw_klyric = json.get("klyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");
    let raw_tlyric = json.get("tlyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");

    let lrc_source = if !raw_lrc.is_empty() {
        raw_lrc
    } else if !raw_klyric.is_empty() {
        raw_klyric
    } else if !raw_tlyric.is_empty() {
        raw_tlyric
    } else {
        return Err("该歌曲没有歌词（纯音乐或未收录）".to_string());
    };

    let lrc_text = lrc_source.trim().to_string();

    tokio::fs::write(&lrc_path, &lrc_text).await
        .map_err(|e| format!("写入 LRC 文件失败: {}", e))?;

    // ⚠️ 设置歌词路径，歌词线程自动读文件解析并 emit lyrics-preview 到前端
    crate::audio_engine::accompaniment::set_lrc_path(lrc_str.clone());

    let data = crate::audio_engine::lrc::LrcData::parse(&lrc_text);

    let _ = app.emit("live-singing-log", format!("[歌词] ✅ 已保存: {} 行", data.len()));
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 100, "message": format!("歌词获取完成 ({} 行)", data.len())}));
    let _ = app.emit("lyrics-downloaded", serde_json::json!({"lrc_path": lrc_str, "line_count": data.len()}));

    Ok(LyricsPayload {
        lrc_path: lrc_str,
        line_count: data.len(),
    })
}

/// 在线搜索歌词（旧版，自动选第一个结果下载）
///
/// 输出路径规则：`{audio_path}.lrc`（与音频文件同目录）。
/// 无需任何本地模型，0 算力消耗，100% 官方时间戳。
#[tauri::command]
pub async fn extract_lyrics(
    audio_path: String,
    force: Option<bool>,
    app: tauri::AppHandle,
) -> Result<LyricsPayload, String> {
    let src = Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("音频文件不存在: {}", audio_path));
    }

    // 从文件名中提取歌曲名（去掉 vocal_/instrumental_ 前缀）
    let filename = src.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let song_name = filename
        .strip_prefix("vocal_")
        .or_else(|| filename.strip_prefix("instrumental_"))
        .or_else(|| filename.strip_prefix("instrument_"))
        .or_else(|| filename.strip_prefix("Vocals_"))
        .unwrap_or(&filename)
        .to_string();

    // LRC 路径与 list_separated_songs 中的检查路径保持一致
    let lrc_path = src.parent()
        .unwrap_or(Path::new(""))
        .join(format!("vocal_{}.lrc", song_name));
    let lrc_str = lrc_path.to_string_lossy().to_string();

    // 非强制模式：如果 LRC 已存在且比音频新，直接返回
    if !force.unwrap_or(false) && lrc_path.exists() {
        if let (Ok(src_meta), Ok(lrc_meta)) = (src.metadata(), lrc_path.metadata()) {
            if let (Ok(src_mtime), Ok(lrc_mtime)) = (src_meta.modified(), lrc_meta.modified()) {
                if lrc_mtime >= src_mtime {
                    let content = tokio::fs::read_to_string(&lrc_path).await
                        .map_err(|e| format!("读取已有 LRC 失败: {}", e))?;
                    let data = crate::audio_engine::lrc::LrcData::parse(&content);
                    return Ok(LyricsPayload {
                        lrc_path: lrc_str,
                        line_count: data.len(),
                    });
                }
            }
        }
    }

    let _ = app.emit("live-singing-log", format!("[歌词] 在线搜索: {}", song_name));
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 10, "message": format!("正在搜索: {}", song_name)}));

    // Step 1: 搜索歌曲 (POST + Form 表单，解决编码和反爬问题)
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .cookie_store(true)
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    // 先访问首页获取 cookie
    let _ = client.get("https://music.163.com/")
        .header("Referer", "https://music.163.com/")
        .send()
        .await
        .map_err(|e| format!("获取 cookie 失败: {}", e))?;

    let search_url = "https://music.163.com/api/search/get/web";
    let params = [
        ("s", song_name.as_str()),
        ("type", "1"),
        ("offset", "0"),
        ("limit", "5"),
    ];

    let search_resp = client.post(search_url)
        .header("Referer", "https://music.163.com/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("搜索请求失败: {}", e))?;

    let search_json: serde_json::Value = search_resp.json().await
        .map_err(|e| format!("解析搜索结果失败: {}", e))?;

    // 🏆 安全的 JSON 链式解包 (绝不 Panic 闪退)
    let songs_array = search_json.get("result")
        .and_then(|res| res.get("songs"))
        .and_then(|songs| songs.as_array())
        .ok_or_else(|| {
            let code = search_json["code"].as_i64().unwrap_or(-1);
            if code == -460 {
                format!("触发网易云反爬机制，请稍后再试")
            } else {
                format!("未找到歌曲: {}（网易云可能没有收录）", song_name)
            }
        })?;

    let first_song = songs_array.first()
        .ok_or_else(|| format!("搜索结果为空，未找到: {}", song_name))?;

    let song_id = first_song.get("id")
        .and_then(|id| id.as_i64())
        .ok_or_else(|| "搜索结果中缺少歌曲 ID".to_string())?;

    let song_id_str = song_id.to_string();
    let song_title = first_song.get("name")
        .and_then(|n| n.as_str())
        .unwrap_or(&song_name)
        .to_string();
    let artist = first_song.get("artists")
        .and_then(|a| a.as_array())
        .and_then(|arr| arr.first())
        .and_then(|artist| artist.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("未知歌手")
        .to_string();

    let _ = app.emit("live-singing-log", format!("[歌词] 找到: {} - {} (ID: {})", artist, song_title, song_id_str));
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 30, "message": format!("正在下载歌词...")}));

    // Step 2: 获取歌词（POST + Form，带完整参数和 cookie）
    let lyric_params = [
        ("id", song_id_str.as_str()),
        ("lv", "-1"),
        ("kv", "-1"),
        ("tv", "-1"),
        ("os", "pc"),
    ];
    let lyric_url = "https://music.163.com/api/song/lyric";

    let lyric_resp = client.post(lyric_url)
        .header("Referer", "https://music.163.com/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&lyric_params)
        .send()
        .await
        .map_err(|e| format!("歌词请求失败: {}", e))?;

    let mut lyric_json: serde_json::Value = lyric_resp.json().await
        .map_err(|e| format!("解析歌词结果失败: {}", e))?;

    // 如果旧 API 返回异常 code，尝试新 API
    let api_code = lyric_json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if api_code != 200 {
        let new_url = "https://music.163.com/api/song/lyric/new";
        if let Ok(resp2) = client.post(new_url)
            .header("Referer", "https://music.163.com/")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&lyric_params)
            .send()
            .await
        {
            if let Ok(json2) = resp2.json::<serde_json::Value>().await {
                if json2.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) == 200 {
                    lyric_json = json2;
                }
            }
        }
    }

    // 调试：打印返回字段，帮助排查歌词为空的问题
    let debug_info = format!("lyric code={}, has_lrc={}, has_klyric={}, has_tlyric={}",
        lyric_json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1),
        lyric_json.get("lrc").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).map(|s| !s.is_empty()).unwrap_or(false),
        lyric_json.get("klyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).map(|s| !s.is_empty()).unwrap_or(false),
        lyric_json.get("tlyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).map(|s| !s.is_empty()).unwrap_or(false),
    );
    let _ = app.emit("live-singing-log", format!("[歌词] {}", debug_info));

    // 获取 LRC 文本：多级回退 + 非空校验
    let raw_lrc = lyric_json.get("lrc").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");
    let raw_klyric = lyric_json.get("klyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");
    let raw_tlyric = lyric_json.get("tlyric").and_then(|l| l.get("lyric")).and_then(|s| s.as_str()).unwrap_or("");

    let lrc_source = if !raw_lrc.is_empty() {
        raw_lrc
    } else if !raw_klyric.is_empty() {
        raw_klyric
    } else if !raw_tlyric.is_empty() {
        raw_tlyric
    } else {
        return Err(format!("{} - {} 没有找到歌词（纯音乐或未收录）", artist, song_title));
    };

    let lrc_text = lrc_source.trim().to_string();

    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 70, "message": "正在保存歌词..."}));

    // Step 3: 写入 LRC 文件
    tokio::fs::write(&lrc_path, &lrc_text).await
        .map_err(|e| format!("写入 LRC 文件失败: {}", e))?;

    // Step 4: 解析统计行数
    let data = crate::audio_engine::lrc::LrcData::parse(&lrc_text);

    // ⚠️ 设置歌词路径，歌词线程自动读文件解析并 emit lyrics-preview
    crate::audio_engine::accompaniment::set_lrc_path(lrc_str.clone());

    let _ = app.emit("live-singing-log", format!("[歌词] 已保存: {} 行", data.len()));
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 100, "message": format!("歌词获取完成 ({} 行)", data.len())}));
    let _ = app.emit("lyrics-downloaded", serde_json::json!({"lrc_path": lrc_str, "line_count": data.len()}));

    Ok(LyricsPayload {
        lrc_path: lrc_str,
        line_count: data.len(),
    })
}

/// 打开桌面歌词悬浮窗（透明无框，始终置顶）
/// 窗口实际由前端使用 @tauri-apps/api/webviewWindow 创建
/// 此命令仅作为 Rust 端的空桩，供前端 invoke 检测命令是否注册
#[tauri::command]
pub async fn open_desktop_lyrics() -> Result<(), String> {
    Ok(())
}

/// 读取 LRC 歌词文件，返回所有歌词行（供前端预览）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LrcLinePayload {
    pub time_secs: f64,
    pub text: String,
}

#[tauri::command]
pub async fn read_lrc_file(lrc_path: String) -> Result<Vec<LrcLinePayload>, String> {
    let path = std::path::Path::new(&lrc_path);
    if !path.exists() {
        return Err("歌词文件不存在".to_string());
    }
    let content = tokio::fs::read_to_string(path).await
        .map_err(|e| format!("读取 LRC 文件失败: {}", e))?;
    let data = crate::audio_engine::lrc::LrcData::parse(&content);
    Ok(data.lines.iter().map(|l| LrcLinePayload {
        time_secs: l.time_secs,
        text: l.text.clone(),
    }).collect())
}
