use crate::cleanup;
use crate::python::PythonExecutor;
use crate::run_manifest;
use crate::state::AppState;
use crate::utils;
use crate::utils::kill_orphan_multiprocessing;
use crate::workspace_layout::{OutputKind, WorkspaceLayout};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use std::process::Stdio;

#[derive(Debug, Serialize, Deserialize)]
pub struct ConvertRequest {
    pub model_name: String,
    pub source_path: String,
    pub transpose: i32,
    pub f0_method: String,
    pub index_rate: f64,
    pub filter_radius: i32,
    pub resample_sr: i32,
    pub rms_mix_rate: f64,
    pub protect: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConvertResponse {
    pub ok: bool,
    pub message: String,
    pub output_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MergeResponse {
    pub ok: bool,
    pub message: String,
    pub output_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConvertProgressPayload {
    pub event_id: String,
    /// 所属流程阶段：独立转换为 `convert`，一键翻唱里同样为 `convert`。
    pub stage: String,
    pub kind: String,
    pub current: i32,
    pub total: i32,
    pub percent: i32,
    pub message: String,
}

#[tauri::command]
pub async fn convert_voice_streaming(
    request: ConvertRequest,
    event_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ConvertResponse, String> {
    // 互斥检查：禁止与训练/UVR 同时运行
    {
        let task = state.active_heavy_task.lock().await;
        if let Some(ref running) = *task {
            // 如果锁标记为 "train"，但实际训练进程已不存在，则自动清除僵尸锁
            if running == "train" {
                let alive = std::process::Command::new("pgrep")
                    .args(["-f", "train\\.py"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false);
                if !alive {
                    drop(task);
                    let mut lock = state.active_heavy_task.lock().await;
                    if lock.as_deref() == Some("train") {
                        *lock = None;
                    }
                    eprintln!("[infer] 检测到训练进程已退出，自动清除任务锁");
                } else {
                    return Err(format!(
                        "系统当前有任务正在运行: [{}]，请等待其完成后再进行声音转换。",
                        running
                    ));
                }
            } else {
                return Err(format!(
                    "系统当前有任务正在运行: [{}]，请等待其完成后再进行声音转换。",
                    running
                ));
            }
        }
    }
    // 设置任务令牌
    *state.active_heavy_task.lock().await = Some("infer".to_string());

    let result = do_convert_voice_streaming(
        request,
        event_id,
        "infer-convert-progress",
        "convert",
        &state,
        app,
    )
    .await;

    // 无论成功失败，都清除令牌
    {
        let pid = state.infer_pid.lock().await.take();
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        let mut lock = state.active_heavy_task.lock().await;
        if lock.as_deref() == Some("infer") {
            *lock = None;
        }
    }

    result
}

#[tauri::command]
pub async fn cancel_infer(state: State<'_, AppState>) -> Result<(), String> {
    let pid = state.infer_pid.lock().await.take();
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
    let mut lock = state.active_heavy_task.lock().await;
    if lock.as_deref() == Some("infer") {
        *lock = None;
    }
    Ok(())
}

/// 执行声音转换，并通过 `event_name` 事件通道推送进度。
///
/// 独立转换页与一键翻唱共用本函数：前者监听 `infer-convert-progress`，
/// 后者监听 `cover-progress`，靠 `stage` 字段区分当前阶段。调用方负责加锁。
pub(crate) async fn do_convert_voice_streaming(
    request: ConvertRequest,
    event_id: String,
    event_name: &str,
    stage: &str,
    state: &State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ConvertResponse, String> {
    use std::path::Path;
    use chrono::Local;

    kill_orphan_multiprocessing();

    // Kill any zombie from a previously crashed / timed-out inference
    {
        let mut old_pid = state.infer_pid.lock().await;
        if let Some(pid) = old_pid.take() {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    let data_dir = state.data_dir.clone();

    let workspace_root = state.workspace_root.read().await;
    let experiments_root = WorkspaceLayout::detect(&*workspace_root).assets_root();
    

    // 歌名取输入文件名，但要先剥掉分离产物自带的角色前缀（`vocal_枪火` → `枪火`）：
    // 否则成品名会以 `vocal_` 开头，看着像中间产物而不是作品。
    let source_stem = song_stem(
        Path::new(&request.source_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("audio"),
    );

    let model_name2 = request.model_name.trim_matches('/');
    let (qualified_dir2, model_file2) = if model_name2.ends_with(".pth") {
        let p = Path::new(&model_name2);
        let parent = p.parent().and_then(|x| x.to_str()).unwrap_or("");
        let fname = p.file_name().and_then(|x| x.to_str()).unwrap_or("model.pth");
        if parent.is_empty() { (model_name2, fname) } else { (parent, fname) }
    } else {
        (model_name2, "model.pth")
    };
    let experiment_root2 = experiments_root.join(qualified_dir2);
    let experiment_root2_str = experiment_root2.to_string_lossy().to_string();

    // For index search: if model is in checkpoints/, use parent dir (version root)
    let index_root = if experiment_root2.ends_with("checkpoints") {
        experiment_root2.parent().unwrap_or(&experiment_root2).to_path_buf()
    } else {
        experiment_root2.clone()
    };
    let index_root_str = index_root.to_string_lossy().to_string();

    // weight_root: experiment_root2 already points to the directory containing the model file
    // (qualified_dir2 is set from parent path, which already includes "checkpoints/" if applicable)
    let weight_root = experiment_root2_str.clone();

    // Find the index file that belongs to this specific experiment
    let index_file = find_experiment_index(&experiment_root2.join(model_file2));

    // 成品文件名自带 voice 与批次号，便于日后回答"这是哪一版模型产出的"
    let (voice_tag, run_tag) =
        model_identity(&request.model_name, &experiment_root2.join(model_file2));
    let hhmm = Local::now().format("%H%M").to_string();

    let output_dir = WorkspaceLayout::detect(&*workspace_root).output_dir(OutputKind::Vocal);
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("创建输出目录失败: {}", e))?;
    let output_path = unique_output_path(
        &output_dir,
        &format!(
            "{}_{}_{}_key{}_{}",
            source_stem, voice_tag, run_tag, request.transpose, hhmm
        ),
        "wav",
    );

    let python_code = format!(
        r#"
import json, os, sys, glob, signal, atexit, time

# ⚠️ DMG 兼容：soundfile 可能因系统缺失 libsndfile 不可用
try:
    import soundfile as sf
except ImportError:
    import scipy.io.wavfile as sf_wav
    sf = None

# Fix OpenMP threading conflicts on macOS - MUST be set before any library imports
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"
os.environ["OMP_NUM_THREADS"] = "1"
os.environ["MKL_NUM_THREADS"] = "1"
os.environ["OPENBLAS_NUM_THREADS"] = "1"
os.environ["VECLIB_MAXIMUM_THREADS"] = "1"
os.environ["NUMEXPR_NUM_THREADS"] = "1"

import warnings
warnings.filterwarnings('ignore')

def _cleanup_subprocess():
    try:
        import subprocess
        subprocess.run(['pkill', '-9', '-P', str(os.getpid())], capture_output=True, timeout=3)
    except Exception:
        pass

atexit.register(_cleanup_subprocess)
signal.signal(signal.SIGTERM, lambda *_: (_cleanup_subprocess(), os._exit(0)))
signal.signal(signal.SIGINT, lambda *_: (_cleanup_subprocess(), os._exit(0)))

def emit_progress(msg, current=0, total=0):
    tag = "[INFER_PROGRESS]"
    if current and total:
        pct = int(current / max(total, 1) * 100)
        sys.stdout.write(f"{{tag}} {{current}}/{{total}} {{pct}}% {{msg}}\n")
    else:
        sys.stdout.write(f"{{tag}} {{msg}}\n")
    sys.stdout.flush()

emit_progress("检查系统资源...")

# --- 内存安全检查 (无 psutil 依赖，直接读 vm_stat) ---
import subprocess as _sp, re as _re
try:
    _vm_out = _sp.run(['/usr/bin/vm_stat'], capture_output=True, text=True, timeout=3).stdout
    _free    = int(_re.search(r'Pages free:\s+(\d+)',     _vm_out).group(1))
    _inact   = int(_re.search(r'Pages inactive:\s+(\d+)', _vm_out).group(1))
    _purg    = int(_re.search(r'Pages purgeable:\s+(\d+)', _vm_out).group(1))
    _avail_mb = (_free + _inact + _purg) * 16384 // 1024 // 1024
except Exception:
    _avail_mb = 9999  # vm_stat 失败时跳过检查

if _avail_mb < 800:
    sys.stdout.write(json.dumps({{
        'ok': False,
        'message': f'内存不足：当前可用约 {{_avail_mb}} MB，需要至少 800 MB。\n请关闭其他应用（如浏览器）后重试。',
        'output_path': ''
    }}) + '\n')
    sys.stdout.flush()
    sys.exit(0)

emit_progress(f"内存检查通过 (可用 {{_avail_mb}} MB)")

os.environ["CUDA_VISIBLE_DEVICES"] = ""

import logging
logging.getLogger('numba').setLevel(logging.WARNING)

try:
    import faiss
    faiss.omp_set_num_threads(2)
except Exception:
    pass

os.environ['weight_root'] = r'{}'
os.environ['index_root'] = r'{}'
os.environ['rmvpe_root'] = r'{}/assets/rmvpe'
os.environ['hubert_path'] = r'{}/assets/hubert/vec-768-layer-12.onnx'

try:
    emit_progress("加载声音转换模型(PyTorch MPS)...")

    model_path = os.path.join(r'{}', '{}')
    from infer.modules.vc.vc_pytorch import get_vc_pytorch
    vc = get_vc_pytorch(model_path)

    _index_path = r'{}'
    sys.stderr.write("[INFER_INDEX] index_path=%s exists=%s\n" % (
        _index_path, os.path.exists(_index_path) if _index_path else False))
    if _index_path and os.path.exists(_index_path):
        emit_progress(f"已加载特征索引: {{os.path.basename(_index_path)}}")
    else:
        emit_progress("未找到特征索引，音色效果可能不佳")

    t_start = time.time()
    emit_progress("声音转换中...")
    info, opt = vc.convert(
        r'{}', {}, '{}', _index_path, {},
        {}, {}, {}, {}, 0
    )
    t_elapsed = time.time() - t_start

    emit_progress("转换完成，保存音频...")

    if opt is not None and opt[1] is not None:
        output_path = r'{}'
        os.makedirs(os.path.dirname(output_path), exist_ok=True)
        sf.write(output_path, opt[1], opt[0]) if sf is not None else sf_wav.write(output_path, opt[0], opt[1])
        result = {{'ok': True, 'message': f"转换完成({{t_elapsed:.1f}}s)\\n{{info}}", 'output_path': output_path}}
    else:
        result = {{'ok': False, 'message': str(info) if info else 'Conversion failed', 'output_path': ''}}
except Exception as e:
    import traceback
    error_msg = f"{{type(e).__name__}}: {{str(e)}}\\n{{traceback.format_exc(limit=3)}}"
    result = {{'ok': False, 'message': error_msg, 'output_path': ''}}

emit_progress("完成")
sys.stdout.write(json.dumps(result) + "\n")
sys.stdout.flush()
"#,
        weight_root,                 // weight_root = os.environ['weight_root']
        index_root_str,              // index_root env
        data_dir.display(),
        data_dir.display(),
        weight_root,                 // model_path dir (weight_root to match how get_vc resolves)
        model_file2,
        index_file.as_ref().map(|p| p.to_string_lossy()).unwrap_or(std::borrow::Cow::Borrowed("")),
        request.source_path,
        request.transpose,
        request.f0_method,
        request.index_rate,
        request.filter_radius,
        request.resample_sr,
        request.rms_mix_rate,
        request.protect,
        output_path.display()
    );

    let mut cmd = executor.build_command("-c", vec![python_code], 0); // foreground

    let stderr_path = std::env::temp_dir().join(format!("rvc_stderr_{}.txt", std::process::id()));
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("无法创建stderr文件: {}", e))?;

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|e| format!("启动转换失败: {}", e))?;

    // Track PID so a future crash/restart can kill this process
    *state.infer_pid.lock().await = child.id();

    let stdout = child.stdout.take()
        .ok_or_else(|| "无法获取 Python stdout".to_string())?;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut last_json_line = String::new();

    loop {
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.starts_with("[INFER_PROGRESS]") {
                    let payload = parse_infer_progress(&trimmed, &event_id, stage);
                    let _ = app.emit(event_name, &payload);
                } else if trimmed.starts_with('{') {
                    last_json_line = trimmed;
                }
                line.clear();
            }
            Err(e) => {
                let _ = app.emit(event_name, &ConvertProgressPayload {
                    event_id: event_id.clone(),
                    stage: stage.to_string(),
                    kind: "log".to_string(),
                    current: 0,
                    total: 0,
                    percent: 0,
                    message: format!("stdout read error: {}", e),
                });
                break;
            }
        }
    }

    // Wait for the process with a hard timeout — on macOS the MPS/swap combination
    // can cause the process to hang indefinitely when memory is critically low.
    const INFER_TIMEOUT_SECS: u64 = 180;
    let child_pid = child.id();
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(INFER_TIMEOUT_SECS),
        child.wait(),
    )
    .await
    .map_err(|_| {
        // Timed out — force-kill the stale process
        if let Some(pid) = child_pid {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
        format!("推理超时（{}秒），进程已强制终止。请关闭其他占用内存的应用后重试。", INFER_TIMEOUT_SECS)
    })?;
    // Clear PID — process finished or was killed
    *state.infer_pid.lock().await = None;
    let status = status.map_err(|e| format!("转换进程错误: {}", e))?
;

    if !status.success() {
        let stderr_lines = tokio::fs::read_to_string(&stderr_path).await.unwrap_or_default();
        let stderr_filtered: String = stderr_lines.lines()
            .filter(|l| !l.contains("DEBUG:numba"))
            .collect::<Vec<_>>()
            .join("\n");
        let hint = if stderr_filtered.trim().is_empty() { String::new() } else { format!("\nstderr: {}", stderr_filtered.trim()) };
        let _ = app.emit(event_name, &ConvertProgressPayload {
            event_id: event_id.clone(),
            stage: stage.to_string(),
            kind: "log".to_string(),
            current: 0, total: 0, percent: 0,
            message: format!("转换进程异常退出: exit code {}\n{}", status.code().unwrap_or(-1), stderr_filtered.trim()),
        });
        let _ = tokio::fs::remove_file(&stderr_path).await;
        let _ = tokio::task::spawn_blocking(kill_orphan_multiprocessing).await;
        return Err(format!("转换进程异常退出 (exit code: {}){}", status.code().unwrap_or(-1), hint));
    }
    let _ = tokio::fs::remove_file(&stderr_path).await;
    let _ = tokio::task::spawn_blocking(kill_orphan_multiprocessing).await;

    if last_json_line.is_empty() {
        return Err("转换完成但未获取到结果".to_string());
    }

    let response: ConvertResponse = serde_json::from_str(&last_json_line)
        .map_err(|e| format!("解析结果失败: {}", e))?;

    // 成品登记：失败不影响转换结果，但要在日志里留痕
    if response.ok {
        let bytes = std::fs::metadata(&output_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        if let Err(e) = state
            .db
            .upsert_output(crate::database::OutputRecord {
                id: 0,
                kind: "converted".to_string(),
                path: output_path.to_string_lossy().to_string(),
                song: source_stem.to_string(),
                voice: voice_tag.clone(),
                run_id: Some(run_tag.clone()),
                transpose: request.transpose,
                created_at: Local::now().timestamp(),
                bytes,
                starred: false,
                finalized: false,
            })
            .await
        {
            eprintln!("[infer] 登记转换成品失败: {}", e);
        }
    }

    Ok(response)
}

/// 从模型标识解析出（声音名, 批次号），用于给成品命名。
///
/// 解析优先级：
///   1. 路径段本身就是合法批次号（`<voice>/<run_id>`）；
///   2. 归档权重文件名 `<voice>-<run_id>-e<epoch>.pth`；
///   3. 旧布局未归档批次：回落到 `model.pth` 的修改时间。
fn model_identity(model_name: &str, model_path: &std::path::Path) -> (String, String) {
    let segments: Vec<&str> = model_name
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let voice = segments.first().copied().unwrap_or("model").to_string();

    if let Some(run) = segments.iter().find(|s| run_manifest::is_valid_run_id(s)) {
        return (voice, (*run).to_string());
    }

    let archive_re = regex::Regex::new(r"-(\d{6}-\d{4}(?:-\d+)?)-e\d+\.pth$").ok();
    if let (Some(re), Some(name)) = (archive_re, model_path.file_name().and_then(|n| n.to_str())) {
        if let Some(run) = re.captures(name).and_then(|c| c.get(1)) {
            return (voice, run.as_str().to_string());
        }
    }

    let fallback = std::fs::metadata(model_path)
        .and_then(|m| m.modified())
        .ok()
        .map(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            dt.format("%y%m%d-%H%M").to_string()
        })
        .unwrap_or_else(|| "000000-0000".to_string());
    (voice, fallback)
}

/// 在目录内生成不冲突的文件名：已存在时追加 `-2`、`-3`……
pub(crate) fn unique_output_path(dir: &std::path::Path, stem: &str, ext: &str) -> std::path::PathBuf {
    let mut candidate = dir.join(format!("{}.{}", stem, ext));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{}-{}.{}", stem, n, ext));
        n += 1;
    }
    candidate
}

fn parse_infer_progress(line: &str, event_id: &str, stage: &str) -> ConvertProgressPayload {
    let after_tag = line.strip_prefix("[INFER_PROGRESS]").unwrap_or(line).trim_start();

    if after_tag.starts_with("[segment]") {
        let rest = after_tag.strip_prefix("[segment] ").unwrap_or(after_tag);
        let parts: Vec<&str> = rest.splitn(3, ' ').collect();
        let (current, total, percent) = if parts.len() >= 3 {
            let cur = parts[0].split('/').next().unwrap_or("0").parse().unwrap_or(0);
            let tot = parts[0].split('/').nth(1).unwrap_or("1").parse().unwrap_or(1);
            let pct = parts[1].trim_end_matches('%').parse().unwrap_or(0);
            (cur, tot, pct)
        } else {
            (0, 0, 0)
        };
        let msg = parts.get(2).unwrap_or(&"").to_string();
        ConvertProgressPayload {
            event_id: event_id.to_string(),
            stage: stage.to_string(),
            kind: "progress".to_string(),
            current,
            total,
            percent,
            message: msg,
        }
    } else {
        ConvertProgressPayload {
            event_id: event_id.to_string(),
            stage: stage.to_string(),
            kind: "log".to_string(),
            current: 0,
            total: 0,
            percent: 0,
            message: after_tag.to_string(),
        }
    }
}

#[allow(dead_code)]
fn is_epoch_checkpoint(name: &str) -> bool {
    if let Some(e_pos) = name.find("_e") {
        let after_e = &name[e_pos+2..];
        if let Some(s_pos) = after_e.find("_s") {
            let num1: &str = &after_e[..s_pos];
            let num2: &str = &after_e[s_pos+2..].split('.').next().unwrap_or("");
            return num1.chars().all(|c| c.is_ascii_digit()) && !num1.is_empty()
                && num2.chars().all(|c| c.is_ascii_digit()) && !num2.is_empty();
        }
    }
    false
}

#[tauri::command]
pub async fn merge_audio(
    vocal_path: String,
    instrumental_path: String,
    vocal_gain: f32,
    state: State<'_, AppState>,
) -> Result<MergeResponse, String> {
    do_merge_audio(vocal_path, instrumental_path, vocal_gain, &state).await
}

/// 混音执行体：独立合成与一键翻唱共用，产物同样登记进成品库。
pub(crate) async fn do_merge_audio(
    vocal_path: String,
    instrumental_path: String,
    vocal_gain: f32,
    state: &State<'_, AppState>,
) -> Result<MergeResponse, String> {
    use std::path::Path;
    use std::process::{Command, Stdio};
    use chrono::Local;

    // 定位伴生 ffmpeg
    let ffmpeg = find_sidecar_ffmpeg(&state.project_root);

    let vocal_stem = Path::new(&vocal_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");

    // 合成作品名沿用转换成品的 `<song>_<voice>_<run_id>_key<±N>` 批次串，并以 `merged_` 开头；
    // 认不出命名时退化为 `merged_<歌名>_<HHmm>`，不再叠加双时间戳。
    let hhmm = Local::now().format("%H%M").to_string();
    let stem = mix_stem(vocal_stem, &hhmm);

    let workspace = state.workspace_root.read().await;
    let mixed_dir = WorkspaceLayout::detect(&*workspace).output_dir(OutputKind::Mixed);
    std::fs::create_dir_all(&mixed_dir).map_err(|e| format!("创建混音输出目录失败: {}", e))?;
    let output_path = unique_output_path(&mixed_dir, &stem, "wav");

    // ffmpeg 混音命令
    let volume_filter = format!("[1:a]volume={}dB[vocal];[0:a][vocal]amix=inputs=2:normalize=0", vocal_gain);

    let output = Command::new(&ffmpeg)
        .args(&[
            "-y",
            "-i", &instrumental_path,
            "-i", &vocal_path,
            "-filter_complex", &volume_filter,
            "-ac", "1",
            output_path.to_str().unwrap(),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("ffmpeg 调用失败: {}", e))?;

    if output.status.success() {
        // 成品登记：混音同样进入成品库，元数据从转换成品名继承
        let (song, voice, run_id, transpose) = parse_vocal_identity(vocal_stem);
        let bytes = std::fs::metadata(&output_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        if let Err(e) = state
            .db
            .upsert_output(crate::database::OutputRecord {
                id: 0,
                kind: "mixed".to_string(),
                path: output_path.to_string_lossy().to_string(),
                song,
                voice,
                run_id,
                transpose,
                created_at: Local::now().timestamp(),
                bytes,
                starred: false,
                finalized: false,
            })
            .await
        {
            eprintln!("[infer] 登记混音成品失败: {}", e);
        }
        Ok(MergeResponse {
            ok: true,
            message: "合成完成".to_string(),
            output_path: output_path.to_string_lossy().to_string(),
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok(MergeResponse {
            ok: false,
            message: format!("合成失败: {}", stderr),
            output_path: String::new(),
        })
    }
}

/// 人声/伴奏分离产物自带的角色前缀。它们描述的是「这条轨是什么」，不属于歌名。
const ROLE_PREFIXES: [&str; 5] = ["vocal_", "vocals_", "instrument_", "instrumental_", "inst_"];

/// 从音频文件名推导歌名：反复剥掉角色前缀（`vocal_枪火` → `枪火`），全被剥光时回退 `audio`。
fn song_stem(raw: &str) -> String {
    let mut s = raw.trim();
    loop {
        let lower = s.to_lowercase();
        match ROLE_PREFIXES.iter().find(|p| lower.starts_with(**p)) {
            Some(p) => s = s[p.len()..].trim_start_matches(|c| c == '_' || c == '-' || c == ' '),
            None => break,
        }
    }
    if s.is_empty() {
        "audio".to_string()
    } else {
        s.to_string()
    }
}

/// 由转换成品名推导合成作品名：`merged_<song>_<voice>_<run_id>_key<±N>_<HHmm>`。
/// 名字里必须留 `merged_` 标记，成品才不会被误认成人声中间产物。
fn mix_stem(vocal_stem: &str, hhmm: &str) -> String {
    let re = regex::Regex::new(
        r"^(?P<song>.+)_(?P<voice>[^_]+)_(?P<run>\d{6}-\d{4}(?:-\d+)?)_key(?P<key>[+-]?\d+)_\d{4}$",
    );
    match re.ok().and_then(|re| re.captures(vocal_stem)) {
        Some(c) => format!(
            "merged_{}_{}_{}_key{}_{}",
            song_stem(&c["song"]), &c["voice"], &c["run"], &c["key"], hhmm
        ),
        None => format!("merged_{}_{}", song_stem(vocal_stem), hhmm),
    }
}

/// 从转换成品名解析（歌名, 声音, 批次号, 升降调），供混音登记沿用同一套元数据。
fn parse_vocal_identity(vocal_stem: &str) -> (String, String, Option<String>, i32) {
    let re = regex::Regex::new(
        r"^(?P<song>.+)_(?P<voice>[^_]+)_(?P<run>\d{6}-\d{4}(?:-\d+)?)_key(?P<key>[+-]?\d+)_\d{4}$",
    );
    match re.ok().and_then(|re| re.captures(vocal_stem)) {
        Some(c) => (
            song_stem(&c["song"]),
            c["voice"].to_string(),
            Some(c["run"].to_string()),
            c["key"].parse().unwrap_or(0),
        ),
        None => (song_stem(vocal_stem), String::new(), None, 0),
    }
}

/// 在 bundle 或开发模式中查找伴生 ffmpeg
fn find_sidecar_ffmpeg(project_root: &std::path::Path) -> std::path::PathBuf {
    // DMG 模式
    if let Ok(exe) = std::env::current_exe() {
        let resources = exe.parent().and_then(|p| p.parent()).map(|p| p.join("Resources"));
        if let Some(ref r) = resources {
            // Tauri v2: Resources/bin/ffmpeg（直接资源）
            let r1 = r.join("bin").join("ffmpeg");
            if r1.exists() { return r1; }
            // Tauri v2: Resources/_up_/bin/ffmpeg（上级资源）
            let r2 = r.join("_up_").join("bin").join("ffmpeg");
            if r2.exists() { return r2; }
        }
    }
    // 开发模式：<project_root>/src-tauri/bin/ffmpeg
    let dev = project_root.join("src-tauri").join("bin").join("ffmpeg");
    if dev.exists() { return dev; }
    // 开发模式备用：<project_root>/bin/ffmpeg
    let dev2 = project_root.join("bin").join("ffmpeg");
    if dev2.exists() { return dev2; }
    // 回退：系统 PATH
    std::path::PathBuf::from("ffmpeg")
}

#[tauri::command]
pub async fn delete_model(model_name: String, state: State<'_, AppState>) -> Result<bool, String> {
    // 训练/UVR/推理期间禁止删除整个声音
    if let Some(task) = state.active_heavy_task.lock().await.clone() {
        return Err(format!(
            "系统当前有任务正在运行：[{}]，请等待完成后再删除。",
            task
        ));
    }

    // model_name 形如 "harrison" 或 "harrison/v2_40k"，只取声音名这一层
    let voice = model_name
        .trim_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();
    if !crate::workspace_layout::is_safe_component(&voice) {
        return Err(format!("声音名不合法：{}", model_name));
    }

    let layout = {
        let workspace_root = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace_root)
    };

    // 资产目录（权重/批次清单）与工作位都要清；v1 下两者同路径，去重后只动一次
    let mut targets = vec![
        layout.voice_assets_dir(&voice),
        layout.runs_root().join(&voice),
    ];
    targets.sort();
    targets.dedup();
    let mut moved = false;
    for target in targets {
        if target.exists() {
            cleanup::move_to_trash(&layout, &voice, &target);
            moved = true;
        }
    }
    if !moved {
        return Err(format!("声音模型不存在：{}", voice));
    }

    // 清掉该声音在数据库中的批次索引
    if let Ok(runs) = state.db.list_runs(Some(voice.clone())).await {
        for run in runs {
            if let Err(e) = state.db.delete_run(run.run_id).await {
                eprintln!("[infer] 删除批次索引失败: {}", e);
            }
        }
    }

    Ok(true)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteOutputResponse {
    pub freed_bytes: u64,
}

/// 删除一个转换/混音成品，连同 `.stft.npy` 旁路缓存一起移入回收站。
/// 只允许删除 `outputs/` 下的文件，避免误删任意路径。
#[tauri::command]
pub async fn delete_output(
    path: String,
    state: State<'_, AppState>,
) -> Result<DeleteOutputResponse, String> {
    let src = std::path::PathBuf::from(&path);
    if src
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("成品路径不合法：{}", path));
    }
    if !src.is_file() {
        return Err(format!("成品不存在：{}", path));
    }

    let layout = {
        let workspace_root = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace_root)
    };
    let mut allowed = vec![
        layout.output_dir(OutputKind::Vocal),
        layout.output_dir(OutputKind::Mixed),
        layout.final_output_dir(),
    ];
    allowed.sort();
    allowed.dedup();
    if !allowed.iter().any(|dir| src.starts_with(dir)) {
        return Err(format!("只能删除输出目录下的成品：{}", path));
    }

    let mut freed = cleanup::move_to_trash(&layout, "outputs", &src);
    // 直播相位模板缓存与成品同名同目录，删除成品时一并带走
    freed += cleanup::move_to_trash(&layout, "outputs", &src.with_extension("stft.npy"));

    if let Err(e) = state.db.delete_output_by_path(path).await {
        eprintln!("[infer] 删除成品索引失败: {}", e);
    }

    Ok(DeleteOutputResponse { freed_bytes: freed })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImportModelRequest {
    pub source: String,
    pub model_name: String,
}

#[tauri::command]
pub async fn import_model(
    request: ImportModelRequest,
    event_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ConvertResponse, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;

    let mut cmd = executor.build_command(
        "tools/import_model.py",
        vec![
            request.source,
            request.model_name,
            workspace_root.to_string_lossy().to_string(),
        ],
        10, // background
    );

    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动导入进程失败: {}", e))?;

    let stdout = child.stdout.take().ok_or("无法获取 stdout")?;
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = String::new();
    let mut last_json_line = String::new();

    loop {
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.starts_with("[IMPORT]") {
                    let msg = trimmed.strip_prefix("[IMPORT]").unwrap_or(&trimmed).trim();
                    let _ = app.emit("model-import-progress", &ConvertProgressPayload {
                        event_id: event_id.clone(),
                        stage: "import".to_string(),
                        kind: "progress".to_string(),
                        current: 0,
                        total: 100,
                        percent: 0, // Could parse percent if needed
                        message: msg.to_string(),
                    });
                } else if trimmed.starts_with('{') {
                    last_json_line = trimmed;
                }
                line.clear();
            }
            Err(_) => break,
        }
    }

    let status = child.wait().await.map_err(|e| format!("进程错误: {}", e))?;

    if !status.success() || last_json_line.is_empty() {
        return Err("导入失败或未获取到结果".to_string());
    }

    let response: ConvertResponse = serde_json::from_str(&last_json_line)
        .map_err(|e| format!("解析结果失败: {}", e))?;

    Ok(response)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BaseModelStatus {
    pub id: String,
    pub name: String,
    pub exists: bool,
    pub size_mb: f64,
}

#[tauri::command]
pub async fn get_base_models_status(state: State<'_, AppState>) -> Result<Vec<BaseModelStatus>, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    let mut cmd = executor.build_command("tools/check_download_models.py", vec!["status".to_string()], 0); // quick status check
    
    let output = cmd.output().await.map_err(|e| format!("Failed to get model status: {}", e))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Find the last line that looks like a JSON array
    let json_str = stdout.lines().rev().find(|l| l.starts_with('[')).unwrap_or("[]");
    
    let status: Vec<BaseModelStatus> = serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse status JSON: {}", e))?;
        
    Ok(status)
}

#[tauri::command]
pub async fn download_base_model(
    model_id: String,
    event_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    let settings = crate::download_settings::DownloadSettings::load(&state.db).await;
    let mut args = vec!["download".to_string(), model_id.clone()];
    args.extend(settings.cli_args());
    let mut cmd = executor.build_command(
        "tools/check_download_models.py",
        args,
        10, // background download
    );
    
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start download process: {}", e))?;

    // Track download PID for cleanup on exit
    let pid = child.id();
    if let Some(p) = pid {
        state.download_pids.lock().await.push(p);
    }
        
    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;
    let mut stderr = child.stderr.take().ok_or("Failed to get stderr")?;
    
    let mut reader = tokio::io::BufReader::new(stdout);
    let mut line = String::new();
    let mut last_error_msg = String::new();
    
    loop {
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.starts_with("[MODEL]") {
                    let msg = trimmed.strip_prefix("[MODEL]").unwrap_or(&trimmed).trim();
                    // 记录最后一条错误消息（解压失败等）
                    if msg.contains("失败") || msg.contains("ERROR") || msg.contains("error") || msg.contains("Error") || msg.contains("未找到") {
                        last_error_msg = msg.to_string();
                    }
                    let _ = app.emit("base-model-download-progress", &ConvertProgressPayload {
                        event_id: event_id.clone(),
                        stage: "download".to_string(),
                        kind: "progress".to_string(),
                        current: 0,
                        total: 100,
                        percent: 0,
                        message: msg.to_string(),
                    });
                }
                line.clear();
            }
            Err(_) => break,
        }
    }
    
    let status = child.wait().await.map_err(|e| format!("Process error: {}", e))?;

    // Remove PID from tracking
    if let Some(p) = pid {
        state.download_pids.lock().await.retain(|x| *x != p);
    }

    if !status.success() {
        let mut err_output = String::new();
        stderr.read_to_string(&mut err_output).await.map_err(|e| format!("Read error: {}", e))?;
        let err_msg = err_output.trim();
        if !err_msg.is_empty() {
            return Err(err_msg.to_string());
        }
        if !last_error_msg.is_empty() {
            return Err(last_error_msg);
        }
        return Err("下载失败，请检查网络连接后重试".to_string());
    }
    Ok(true) // 明确返回 true
}

#[tauri::command]
pub async fn delete_base_model(model_id: String, state: State<'_, AppState>) -> Result<bool, String> {
    let data_dir = state.data_dir.clone();
    let paths: Vec<std::path::PathBuf> = match model_id.as_str() {
        "hubert" => vec![data_dir.join("assets/hubert/vec-768-layer-12.onnx")],
        "rmvpe" => vec![data_dir.join("assets/rmvpe/rmvpe.onnx")],
        "bs_roformer" | "inst_main" | "karaoke" | "voc_ft" | "kim_vocal" => {
            let dir = data_dir.join("assets/uvr_models");
            let filename = match model_id.as_str() {
                "inst_main" => "UVR_MDXNET_Main.onnx",
                "bs_roformer" => "model_bs_roformer_ep_317_sdr_12.9755.ckpt",
                "karaoke" => "UVR-MDX-NET-KARA_2.onnx",
                "voc_ft" => "UVR-MDX-NET-Voc_FT.onnx",
                "kim_vocal" => "Kim_Vocal_2.onnx",
                _ => "",
            };
            let file_path = dir.join(filename);
            if file_path.exists() { vec![file_path] } else { vec![] }
        }
        _ => return Err(format!("Unknown model: {}", model_id)),
    };
    for path in paths {
        if path.exists() {
            tokio::fs::remove_file(&path).await.map_err(|e| format!("Failed to delete: {}", e))?;
        }
    }
    Ok(true)
}

#[tauri::command]
pub async fn get_model_storage_paths(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let data_dir = state.data_dir.to_string_lossy().to_string();
    let assets_root = {
        let workspace_root = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace_root).assets_root()
    };
    Ok(vec![
        format!("项目依赖模型: {}/assets/", data_dir),
        format!("用户声音模型: {}/", assets_root.display()),
    ])
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    collect_models(&state).await
}

/// 扫描工作区模型资产，返回可推理的模型标识（`<voice>/weights/x.pth` 等）。
pub(crate) async fn collect_models(state: &AppState) -> Result<Vec<String>, String> {
    let workspace_root = state.workspace_root.read().await;
    let layout = WorkspaceLayout::detect(&*workspace_root);
    let exp_root = layout.assets_root();
    // 旧布局只把 v*_(32|40|48)k 当版本目录，避免把 _backup_* 之类误判为模型
    let version_re = regex::Regex::new(r"^v\d+_(32|40|48)k$").map_err(|e| e.to_string())?;
    let mut models = Vec::new();

    let mut name_entries = match tokio::fs::read_dir(&exp_root).await {
        Ok(entries) => entries,
        Err(_) => return Ok(models),
    };
    while let Ok(Some(name_entry)) = name_entries.next_entry().await {
        if !name_entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = name_entry.file_name().to_string_lossy().to_string();
        let name_dir = name_entry.path();

        // 归档权重：models/<voice>/weights/*.pth
        if let Ok(mut weight_entries) = tokio::fs::read_dir(name_dir.join("weights")).await {
            while let Ok(Some(weight_entry)) = weight_entries.next_entry().await {
                let fname = weight_entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".pth") && !fname.starts_with("D_") {
                    models.push(format!("{}/weights/{}", name, fname));
                }
            }
        }

        // 旧布局工作位：<voice>/<version>/model.pth 与 checkpoints/*.pth
        if let Ok(mut ver_entries) = tokio::fs::read_dir(&name_dir).await {
            while let Ok(Some(ver_entry)) = ver_entries.next_entry().await {
                if !ver_entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let ver_sr = ver_entry.file_name().to_string_lossy().to_string();
                if !version_re.is_match(&ver_sr) {
                    continue;
                }
                let ver_dir = ver_entry.path();

                if ver_dir.join("model.pth").exists() {
                    models.push(format!("{}/{}", name, ver_sr));
                }

                let checkpoints_dir = ver_dir.join("checkpoints");
                if checkpoints_dir.is_dir() {
                    if let Ok(mut ckpt_entries) = tokio::fs::read_dir(&checkpoints_dir).await {
                        while let Ok(Some(ckpt_entry)) = ckpt_entries.next_entry().await {
                            let fname = ckpt_entry.file_name().to_string_lossy().to_string();
                            if fname.ends_with(".pth") && !fname.starts_with("D_") {
                                models.push(format!("{}/{}/checkpoints/{}", name, ver_sr, fname));
                            }
                        }
                    }
                }
            }
        }
    }

    models.sort();
    models.dedup();
    Ok(models)
}

/// 找到与模型配套的索引文件。
/// 归档权重与其索引同前缀（`<voice>-<run_id>-e<epoch>.{pth,index}`），优先直接命中；
/// 旧布局回落到 `checkpoints/` 或模型所在目录下的 `added_*.index`。
fn find_experiment_index(model_path: &std::path::Path) -> Option<std::path::PathBuf> {
    if let Some(stem) = model_path.file_stem() {
        let sibling = model_path.with_file_name(format!("{}.index", stem.to_string_lossy()));
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let dir = model_path.parent()?;
    let ckpt_dir = dir.join("checkpoints");
    let search_dir = if ckpt_dir.is_dir() { ckpt_dir } else { dir.to_path_buf() };
    let files = utils::glob_files(&search_dir, "index").ok()?;
    files.into_iter().last()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_identity_reads_batch_from_path_and_archive_name() {
        let dummy = std::path::Path::new("/tmp/x.pth");
        assert_eq!(
            model_identity("harrison/261122-0156", dummy),
            ("harrison".to_string(), "261122-0156".to_string())
        );
        assert_eq!(
            model_identity(
                "harrison/weights/harrison-261122-0156-e150.pth",
                std::path::Path::new("/tmp/harrison-261122-0156-e150.pth"),
            ),
            ("harrison".to_string(), "261122-0156".to_string())
        );
    }

    #[test]
    fn mix_stem_always_marks_merged() {
        assert_eq!(
            mix_stem("枪火_harrison_261122-0156_key-3_0930", "0955"),
            "merged_枪火_harrison_261122-0156_key-3_0955"
        );
        assert_eq!(mix_stem("legacy_mix", "0955"), "merged_legacy_mix_0955");
    }

    #[test]
    fn song_stem_strips_role_prefixes() {
        assert_eq!(song_stem("vocal_枪火"), "枪火");
        assert_eq!(song_stem("instrument_枪火"), "枪火");
        assert_eq!(song_stem("枪火"), "枪火");
        assert_eq!(song_stem("vocal_"), "audio");
    }
}
