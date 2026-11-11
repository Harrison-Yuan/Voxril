use crate::audio_engine::{self, LiveSingingConfig};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{Emitter, State};

fn default_mode() -> String { "correct".to_string() }
fn default_scale_name() -> String { "chromatic".to_string() }

#[derive(Debug, Serialize, Deserialize)]
pub struct LiveSingingRequest {
    pub input_device: String,
    pub output_device: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub scale_root: i32,
    #[serde(default = "default_scale_name")]
    pub scale_name: String,
    pub model_path: String,
    pub f0_up_key: i32,
    pub accompaniment_path: String,
    pub cover_path: String,
    pub duck_gain: f32,
    pub index_path: Option<String>,
    #[serde(default)]
    pub index_rate: f64,
}

/// 分离歌曲条目（供直播页面选择）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeparatedSong {
    pub name: String,                      // 歌曲名
    pub instrumental_path: String,         // 伴奏路径
    pub vocal_path: String,                // 人声路径
    pub f0_path: String,                   // F0 谱子路径
    pub f0_exists: bool,                   // F0 是否已提取
    pub lrc_exists: bool,                  // 歌词是否已提取
    pub lrc_path: String,                  // 歌词路径（可直接读取）
    pub converted_path: String,            // 已转换的人声路径（RVC 合成结果）
    pub stft_exists: bool,                 // STFT 相位模板是否已导出
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
    let ins_dir = workspace.join("separated/instrumental");
    let vocal_dir = workspace.join("separated/vocals");

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

            let f0_path = vocal_dir.join(format!("vocal_{}.f0.json", song_name));

            // 查找已转换的人声（converted/ 目录）
            // 转换输出格式: {source_stem}_{模型名}_key{转调}_{时间戳}.wav
            // source_stem 通常是 vocal_{歌名}，文件名一定包含 {歌名} 子串
            let converted_dir = workspace.join("converted");
            let (converted_path, stft_exists) = if let Ok(entries) = std::fs::read_dir(&converted_dir) {
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
                        let epath = e.path();
                        let cpath = epath.to_string_lossy().to_string();
                        eprintln!("[list_separated_songs] ✅ 匹配到翻唱: {} (song={})", cpath, song_name);
                        let stem_os = epath.file_stem().unwrap_or_default();
                        let stem = stem_os.to_str().unwrap_or(&song_name);
                        let stft = converted_dir.join(format!("{}.stft.npy", stem));
                        (cpath, stft.exists())
                    }
                    None => {
                        eprintln!("[list_separated_songs] ⚠️ converted/ 目录无匹配: song={}", song_name);
                        (String::new(), false)
                    }
                }
            } else {
                eprintln!("[list_separated_songs] ⚠️ converted/ 目录不存在: {}", converted_dir.display());
                (String::new(), false)
            };

            let lrc_path = vocal_dir.join(format!("vocal_{}.lrc", song_name));
            let lrc_exists = lrc_path.exists();
            let lrc_path_str = lrc_path.to_string_lossy().to_string();
            songs.push(SeparatedSong {
                name: song_name.clone(),
                instrumental_path: path.to_string_lossy().to_string(),
                vocal_path: vocal_path.to_string_lossy().to_string(),
                f0_path: f0_path.to_string_lossy().to_string(),
                f0_exists: f0_path.exists(),
                lrc_exists,
                lrc_path: lrc_path_str,
                converted_path,
                stft_exists,
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
    eprintln!("[RUST] start_live_singing 命令被调用，mode={}", request.mode);

    // ⚠️ 抢先停止：如果之前有引擎未完全退出，确保它先停掉再启动新的。
    // 防止两个引擎实例同时操作 CoreAudio 导致设备格式冲突。
    {
        let stop_tx = state.live_singing_stop.lock().await.take();
        if let Some(stop_tx) = stop_tx {
            eprintln!("[RUST] 发现残留引擎，正在停止...");
            let _ = stop_tx.send(());
            // 等待引擎完全退出（等 active 变 false），最多 3 秒
            for _ in 0..100 {
                if !*state.live_singing_active.read().await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            }
            *state.live_singing_active.write().await = false;
            eprintln!("[RUST] 残留引擎已停止");
        }
    }

    let active = state.live_singing_active.write().await;
    if *active {
        return Err("直播唱歌引擎已在运行中".into());
    }

    let data_dir = state.data_dir.clone();
    let workspace_root = state.workspace_root.read().await;

    // 修音模式不需要模型路径解析
    let model_path = String::new();

    // ContentVec 特征模型 — 修音模式不需要
    let vec_path = String::new();

    // 先保存工作目录路径，后面还要用
    let workspace_str = workspace_root.to_string_lossy().to_string();
    drop(workspace_root);

    // 自动解析 F0 路径：如果 accompaniment 在 separated/instrumental 下，
    // 自动从 separated/vocals 找同名 .f0.json
    let f0_score_path = {
        let ins_path = std::path::Path::new(&request.accompaniment_path);
        if ins_path.exists() && !request.accompaniment_path.is_empty() {
            let ws = std::path::Path::new(&workspace_str);
            let vocal_dir = ws.join("separated/vocals");
            // 取文件名，把 "instrumental_" 替换为 "vocal_"
            let fname = ins_path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let vocal_fname = fname
                .replacen("instrumental_", "vocal_", 1)
                .replacen("instrument_", "vocal_", 1);
            // vocal_枪火.flac → vocal_枪火.f0.json
            let vocal_fname_noext = vocal_fname
                .rsplit_once('.')
                .map(|(name, _)| name)
                .unwrap_or(&vocal_fname);
            let f0_candidate = vocal_dir.join(format!("{}.f0.json", vocal_fname_noext));
            let f0_local = ins_path.with_extension("f0.json");
            if f0_candidate.exists() {
                f0_candidate.to_string_lossy().to_string()
            } else if f0_local.exists() {
                f0_local.to_string_lossy().to_string()
            } else {
                // 留空：让引擎自动从 accompaniment_path 推断
                String::new()
            }
        } else {
            String::new()
        }
    };

    // 修音模式不需要 Index（特征检索），设为空
    let index_path = String::new();

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

    eprintln!("[LIVE] 解析后 index_path  = {}", index_path);
    eprintln!("[LIVE] index 文件是否存在 = {}", Path::new(&index_path).exists());

    // Index 文件校验：
    //   - 修音模式 (correct) 不需要模型/index，跳过校验
    //   - 训练模型（experiments/ 目录）：强制要求 index，无则拒绝上播
    //   - 预训练基础模型（pretrained/pretrained_v2 目录）：index 为可选，仅记录警告
    if request.mode != "correct" {
        let is_pretrained = model_path.contains("/pretrained");
        if (index_path.is_empty() || !Path::new(&index_path).exists()) && !is_pretrained {
            return Err(format!(
                "所选模型缺少特征索引文件 (added_*.index)。模型与索引应位于同一目录。\n模型路径: {}",
                model_path
            ));
        }
        if is_pretrained && index_path.is_empty() {
            eprintln!("[RUST] pretrained 基础模型，无 index 文件，跳过特征检索（仅用 AI 特征）");
        } else if !index_path.is_empty() {
            eprintln!("[RUST] 使用 Index 文件: {}", index_path);
        }
    }

    let config = LiveSingingConfig {
        input_device: request.input_device,
        output_device: request.output_device,
        mode: request.mode,
        scale_root: request.scale_root,
        scale_name: request.scale_name,
        model_path,
        vec_path,
        rmvpe_path,
        fcpe_path,
        f0_up_key: request.f0_up_key,
        accompaniment_path: request.accompaniment_path,
        cover_path: request.cover_path,
        f0_score_path,
        duck_gain: request.duck_gain,
        index_path,
        index_rate: request.index_rate,
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    *state.live_singing_stop.lock().await = Some(stop_tx);
    // 不在这里设置 active——等引擎真正就绪后再设

    // Pass a clone of the active flag so the thread can set/reset it
    let active_flag = state.live_singing_active.clone();
    drop(active);

    let app_handle = app.clone();

    std::thread::spawn(move || {
        eprintln!("[RUST] 引擎线程已启动，开始调用 start_live_engine...");
        match audio_engine::engine::start_live_engine(config, app_handle.clone(), active_flag.clone(), stop_rx) {
            Ok(_) => {
                // 引擎正常退出（收到 stop）后才走到这里
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    *active_flag.write().await = false;
                });
                let _ = app_handle.emit("live-singing-stopped", "引擎已停止");
            }
            Err(e) => {
                eprintln!("[RUST] 引擎启动失败: {}", e);
                // 引擎启动失败，确保 active=false
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    *active_flag.write().await = false;
                });
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

/// 直播中实时调整特征检索融合比例。
/// 前端 Slider 拖动时调用，0.0=纯AI特征，1.0=纯检索特征，立即生效。
#[tauri::command]
pub async fn set_live_index_rate(rate: f64) -> Result<(), String> {
    let rate = rate.clamp(0.0, 1.0) as f32;
    crate::audio_engine::INDEX_RATE.store(f32::to_bits(rate), std::sync::atomic::Ordering::Relaxed);
    eprintln!("[SET] index_rate = {:.2}", rate);
    Ok(())
}

/// 设置音源模式：0=真人演唱，1=AI 翻唱
/// 内部自动处理 MIC_MUTED 和 cover audible
#[tauri::command]
pub async fn set_audio_source(source: u32) -> Result<(), String> {
    let source = if source == 0 { 0 } else { 1 };
    crate::audio_engine::AUDIO_SOURCE.store(source, std::sync::atomic::Ordering::Relaxed);
    crate::audio_engine::MIC_MUTED.store(source == 1, std::sync::atomic::Ordering::Relaxed);
    eprintln!("[SET] audio_source = {} (真人={}, mic_muted={})", source, source == 0, source == 1);
    Ok(())
}

#[tauri::command]
pub async fn stop_live_singing(state: State<'_, AppState>) -> Result<(), String> {
    let stop_tx = state.live_singing_stop.lock().await.take();
    if let Some(stop_tx) = stop_tx {
        let _ = stop_tx.send(());

        for _ in 0..100 {
            if !*state.live_singing_active.read().await {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }

        return Err("正在停止直播引擎，请稍后再试".into());
    }

    *state.live_singing_active.write().await = false;
    Ok(())
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
pub async fn play_accompaniment(audio_path: String, lrc_path: Option<String>, cover_path: Option<String>, app: tauri::AppHandle) -> Result<(), String> {
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

    // ⚠️ play_accompaniment 不直接 emit player-state
    // load_audio() 递增 SONG_VERSION，输出回调 drain buffer + reset counter
    // 歌词线程下一 tick 检测到 has_audio()=true，自动发送完整 player-state

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
    Ok(())
}

/// 设置歌词文件路径（歌词线程自动加载预览，引擎启动前后均可调用）
#[tauri::command]
pub async fn set_lrc_path(lrc_path: String) -> Result<(), String> {
    crate::audio_engine::accompaniment::set_lrc_path(lrc_path);
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
    let active = *state.live_singing_active.read().await;
    Ok(active)
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

    let exp_root = workspace_root.join("experiments");

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

// ─── F0 音高预提取 ───

/// F0 音高曲线响应（供前端展示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct F0ProfilePayload {
    pub f0_curve: Vec<f32>,
    pub sample_rate: u32,
    pub hop_length: usize,
    pub duration_secs: f32,
    pub frame_count: usize,
    pub json_path: String,
}

#[tauri::command]
pub async fn extract_f0_profile(
    audio_path: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<F0ProfilePayload, String> {
    let src = Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("音频文件不存在: {}", audio_path));
    }

    // 输出 JSON 路径：与音频同目录，后缀 .f0.json
    let json_path = src.with_extension("f0.json");
    let json_str = json_path.to_string_lossy().to_string();

    // 如果已存在且比音频新，直接读取返回
    if json_path.exists() {
        if let Ok(src_meta) = src.metadata() {
            if let Ok(json_meta) = json_path.metadata() {
                if let (Ok(src_mtime), Ok(json_mtime)) = (src_meta.modified(), json_meta.modified()) {
                    if json_mtime >= src_mtime {
                        let content = tokio::fs::read_to_string(&json_path).await
                            .map_err(|e| format!("读取已有 F0 Profile 失败: {}", e))?;
                        let profile: crate::audio_engine::types::F0Profile = serde_json::from_str(&content)
                            .map_err(|e| format!("解析 F0 Profile JSON 失败: {}", e))?;
                        let frame_count = profile.f0_curve.len();
                        let duration_secs = profile.duration_secs();
                        return Ok(F0ProfilePayload {
                            f0_curve: profile.f0_curve,
                            sample_rate: profile.sample_rate,
                            hop_length: profile.hop_length,
                            duration_secs,
                            frame_count,
                            json_path: json_str,
                        });
                    }
                }
            }
        }
    }

    let _ = app.emit("live-singing-log", "[F0] 开始提取原唱 F0 音高曲线（Rust ONNX 引擎）...");
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 0, "message": "正在用 RMVPE 提取原唱 F0 音高曲线..."}));

    let data_dir = state.data_dir.clone();
    let json_path_clone = json_path.clone();

    // ── Rust 原生 F0 提取（替代 Python 子进程） ──
    let profile = tokio::task::spawn_blocking(move || {
        // 1. 定位 RMVPE ONNX 模型
        let mut rmvpe_path = data_dir.join("assets").join("rmvpe").join("rmvpe.onnx");
        if !rmvpe_path.exists() {
            rmvpe_path = data_dir.join("assets").join("rmvpe").join("rmvpe.pt");
        }
        if !rmvpe_path.exists() {
            return Err(format!("RMVPE 模型不存在: {}", rmvpe_path.display()));
        }

        // 2. 加载 RMVPE ONNX session
        let rmvpe = crate::audio_engine::rvc::build_session(
            &rmvpe_path.to_string_lossy()
        ).map_err(|e| format!("RMVPE 加载失败: {}", e))?;
        let mut rmvpe = rmvpe;

        // 3. 用 decode-audio 加载音频为 48kHz f32 PCM
        //    优先使用当前可执行文件路径（兼容 cargo run 和 bundle 两种场景）
        let bin_path = std::env::var("VOXRIL_BIN")
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "voxril".into())
            });
        let output = std::process::Command::new(&bin_path)
            .args(&["decode-audio", "--input", &audio_path, "--sr", "48000"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("decode-audio 调用失败: {}", e))?;
        if !output.status.success() {
            return Err(format!("decode-audio 失败 (exit={})", output.status));
        }

        // f32 LE PCM
        let audio_48k: Vec<f32> = output.stdout.chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        let dur = audio_48k.len() as f64 / 48000.0;
        eprintln!("[F0] Rust 加载: {:.1}s, {} samps @ 48kHz", dur, audio_48k.len());

        // 4. 运行 RMVPE 推理
        use crate::audio_engine::rvc::extract_f0_from_file;
        let profile = extract_f0_from_file(&audio_48k, &mut rmvpe);

        // 5. 写 JSON
        let json = serde_json::to_string(&profile)
            .map_err(|e| format!("序列化 F0 Profile 失败: {}", e))?;
        std::fs::write(&json_path_clone, &json)
            .map_err(|e| format!("写 F0 Profile 文件失败: {}", e))?;

        let n_nonzero = profile.f0_curve.iter().filter(|&&v| v > 0.0).count();
        let mean_f0 = if n_nonzero > 0 {
            profile.f0_curve.iter().filter(|&&v| v > 0.0).sum::<f32>() / n_nonzero as f32
        } else { 0.0 };
        eprintln!("[F0] Rust RMVPE 完成: {} 帧, {} 有声帧, 均值={:.1}Hz",
            profile.f0_curve.len(), n_nonzero, mean_f0);

        Ok(profile)
    }).await.map_err(|e| format!("spawn_blocking 失败: {}", e))?
    .map_err(|e| format!("F0 提取失败: {}", e))?;

    let _ = app.emit("live-singing-log", "[F0] F0 Profile 提取完成 (Rust ONNX)");
    let _ = app.emit("live-singing-progress", serde_json::json!({"percent": 100, "message": "F0 Profile 提取完成"}));

    let frame_count = profile.f0_curve.len();
    let duration_secs = profile.duration_secs();
    Ok(F0ProfilePayload {
        f0_curve: profile.f0_curve,
        sample_rate: profile.sample_rate,
        hop_length: profile.hop_length,
        duration_secs,
        frame_count,
        json_path: json_str,
    })
}

/// 根据音色模型路径，在同目录下查找 *.index 文件。
/// 训练生成的索引与 ONNX 模型在同一目录，直接 glob_files 即可找到。
/// 返回 None 表示该模型目录下没有 index 文件。
#[tauri::command]
pub async fn get_model_index_path(model_path: String) -> Result<Option<String>, String> {
    let p = std::path::Path::new(&model_path);
    let dir = p.parent().filter(|d| d.is_dir());
    match dir {
        Some(d) => {
            let files = crate::utils::glob_files(d, "index").unwrap_or_default();
            Ok(files.into_iter().last().map(|p| p.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

/// 导出 STFT 相位模板：为已合成的人声音频生成 .stft.npy 文件。
/// 直播修音时 Phase Vocoder 用此模板抑制 phasiness 失真。
#[tauri::command]
pub async fn export_stft_template(
    audio_path: String,
    _state: State<'_, AppState>,
) -> Result<String, String> {
    let src = Path::new(&audio_path);
    if !src.exists() {
        return Err(format!("音频文件不存在: {}", audio_path));
    }

    let stft_path = src.with_extension("stft.npy");
    let stft_str = stft_path.to_string_lossy().to_string();
    let stft_path_clone = stft_path.clone();
    let audio_path_clone = audio_path.clone();

    // 如果已存在且比音频新，直接返回
    if stft_path.exists() {
        if let (Ok(src_m), Ok(stft_m)) = (src.metadata(), stft_path.metadata()) {
            if let (Ok(src_t), Ok(stft_t)) = (src_m.modified(), stft_m.modified()) {
                if stft_t >= src_t {
                    return Ok(stft_str);
                }
            }
        }
    }

    tokio::task::spawn_blocking(move || {
        crate::audio_engine::pitch_correction::export_stft_to_file(
            std::path::Path::new(&audio_path_clone),
            std::path::Path::new(&stft_path_clone),
        )?;
        Ok(stft_str)
    }).await.map_err(|e| e.to_string())?
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
