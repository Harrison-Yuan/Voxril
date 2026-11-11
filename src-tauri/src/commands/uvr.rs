use crate::python::PythonExecutor;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use std::process::Stdio;

#[derive(Debug, Serialize, Deserialize)]
pub struct SeparateRequest {
    pub model_name: String,
    pub input_path: String,
    pub vocal_dir: String,
    pub instrumental_dir: String,
    pub agg: i32,
    pub format: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SeparateResponse {
    pub ok: bool,
    pub message: String,
    pub vocal_dir: String,
    pub instrumental_dir: String,
    pub vocal_files: Vec<String>,
    pub instrumental_files: Vec<String>,
}

#[tauri::command]
pub async fn separate_vocals(
    request: SeparateRequest,
    state: State<'_, AppState>,
) -> Result<SeparateResponse, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let python_code = format!(
        r#"
import json, os, sys
sys.path.insert(0, r'{}')
sys.stderr.write("UVR_DEBUG_PYTHON=" + sys.executable + "\n")
sys.stderr.flush()

from infer.modules.vocal_sep_mps import separate, separate_batch

input_path = r'{}'
vocal_dir = r'{}'
ins_dir = r'{}'
format = '{}'
model_name = '{}'

os.makedirs(vocal_dir, exist_ok=True)
os.makedirs(ins_dir, exist_ok=True)

if os.path.isdir(input_path):
    results = separate_batch(input_path, vocal_dir, ins_dir, format, model_name=model_name)
    all_ok = all(r.get('ok', False) for r in results)
    vocal_files = [r.get('vocal_path', '') for r in results if r.get('ok')]
    ins_files = [r.get('instrumental_path', '') for r in results if r.get('ok')]
    print(json.dumps({{
        'ok': all_ok,
        'message': '\n'.join(r.get('message', '') for r in results),
        'vocal_dir': vocal_dir,
        'instrumental_dir': ins_dir,
        'vocal_files': vocal_files,
        'instrumental_files': ins_files
    }}))
else:
    result = separate(input_path, vocal_dir, ins_dir, format, model_name=model_name)
    print(json.dumps({{
        'ok': result.get('ok'),
        'message': result.get('message', ''),
        'vocal_dir': vocal_dir,
        'instrumental_dir': ins_dir,
        'vocal_files': [result.get('vocal_path', '')] if result.get('ok') else [],
        'instrumental_files': [result.get('instrumental_path', '')] if result.get('ok') else []
    }}))
"#,
        state.project_root.display(),
        request.input_path,
        request.vocal_dir,
        request.instrumental_dir,
        request.format,
        request.model_name,
    );

    let result = executor
        .run_script("-c", vec![python_code])
        .await
        .map_err(|e| format!("Vocal separation failed: {}", e))?;

    let response: SeparateResponse = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(response)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SeparateProgressPayload {
    pub event_id: String,
    pub kind: String,       // "progress" | "log" | "result"
    pub step: String,       // "file" | "segment"
    pub current: i32,
    pub total: i32,
    pub percent: i32,
    pub message: String,
    pub result: Option<SeparateResponse>,
}

#[tauri::command]
pub async fn separate_vocals_streaming(
    request: SeparateRequest,
    event_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<SeparateResponse, String> {
    // 互斥检查：禁止与训练/推理同时运行
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
                    eprintln!("[uvr] 检测到训练进程已退出，自动清除任务锁");
                } else {
                    return Err(format!(
                        "系统当前有任务正在运行: [{}]，请等待其完成后再进行歌声分离。",
                        running
                    ));
                }
            } else {
                return Err(format!(
                    "系统当前有任务正在运行: [{}]，请等待其完成后再进行歌声分离。",
                    running
                ));
            }
        }
    }
    // 设置任务令牌
    *state.active_heavy_task.lock().await = Some("uvr".to_string());

    let result = _do_separate_vocals_streaming(request, event_id, &state, app).await;

    // 无论成功失败，都清除令牌
    {
        let pid = state.uvr_pid.lock().await.take();
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
        let mut lock = state.active_heavy_task.lock().await;
        if lock.as_deref() == Some("uvr") {
            *lock = None;
        }
    }

    result
}

#[tauri::command]
pub async fn cancel_uvr(state: State<'_, AppState>) -> Result<(), String> {
    let pid = state.uvr_pid.lock().await.take();
    if let Some(pid) = pid {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
    let mut lock = state.active_heavy_task.lock().await;
    if lock.as_deref() == Some("uvr") {
        *lock = None;
    }
    Ok(())
}

async fn _do_separate_vocals_streaming(
    request: SeparateRequest,
    event_id: String,
    state: &State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<SeparateResponse, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    let project_root = state.project_root.clone();

    let python_code = format!(
        r#"
import json, os, sys, subprocess
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"
sys.path.insert(0, r'{project_root}')
sys.stderr.write("UVR_DEBUG_PYTHON=" + sys.executable + "\n")
sys.stderr.flush()

# ── 自愈：检测 audio_separator 是否可用，打包 App 首次运行时可能缺失 ──
try:
    import audio_separator
except ImportError:
    sys.stdout.write("[UVR_PROGRESS] 正在安装 audio-separator（首次运行），请稍候...\n")
    sys.stdout.flush()
    result = subprocess.run(
        [sys.executable, "-m", "pip", "install", "audio-separator>=0.44.0", "--quiet"],
        capture_output=True, text=True, timeout=300,
    )
    if result.returncode != 0:
        print(json.dumps({{
            'ok': False,
            'message': 'audio-separator 安装失败，请检查网络后重试。\n' + result.stderr[-500:],
            'vocal_dir': r'{vocal_dir}',
            'instrumental_dir': r'{ins_dir}',
            'vocal_files': [],
            'instrumental_files': [],
        }}))
        sys.exit(1)
    sys.stdout.write("[UVR_PROGRESS] audio-separator 安装完成\n")
    sys.stdout.flush()

import torch
torch.set_num_threads(2)

from infer.modules.vocal_sep_mps import separate, separate_batch

input_path = r'{input_path}'
vocal_dir = r'{vocal_dir}'
ins_dir = r'{ins_dir}'
fmt = '{format}'
model_name = '{model_name}'

os.makedirs(vocal_dir, exist_ok=True)
os.makedirs(ins_dir, exist_ok=True)

if os.path.isdir(input_path):
    results = separate_batch(input_path, vocal_dir, ins_dir, fmt, model_name=model_name)
    all_ok = all(r.get('ok', False) for r in results)
    vocal_files = [r.get('vocal_path', '') for r in results if r.get('ok')]
    ins_files = [r.get('instrumental_path', '') for r in results if r.get('ok')]
    print(json.dumps({{
        'ok': all_ok,
        'message': '\n'.join(r.get('message', '') for r in results),
        'vocal_dir': vocal_dir,
        'instrumental_dir': ins_dir,
        'vocal_files': vocal_files,
        'instrumental_files': ins_files
    }}))
else:
    result = separate(input_path, vocal_dir, ins_dir, fmt, model_name=model_name)
    print(json.dumps({{
        'ok': result.get('ok'),
        'message': result.get('message', ''),
        'vocal_dir': vocal_dir,
        'instrumental_dir': ins_dir,
        'vocal_files': [result.get('vocal_path', '')] if result.get('ok') else [],
        'instrumental_files': [result.get('instrumental_path', '')] if result.get('ok') else []
    }}))
"#,
        project_root = project_root.display(),
        input_path = request.input_path,
        vocal_dir = request.vocal_dir,
        ins_dir = request.instrumental_dir,
        format = request.format,
        model_name = request.model_name,
    );

    let mut cmd = executor.build_command("-c", vec![python_code], 0);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("启动分离失败: {}", e))?;

    // Track PID for cancellation
    if let Some(pid) = child.id() {
        state.uvr_pid.lock().await.replace(pid);
    }

    let stdout = child.stdout.take()
        .ok_or_else(|| "无法获取 Python stdout".to_string())?;
    let stderr = child.stderr.take()
        .ok_or_else(|| "无法获取 Python stderr".to_string())?;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut last_json_line = String::new();
    let mut stderr_reader = BufReader::new(stderr);

    let app2 = app.clone();
    let event_id2 = event_id.clone();
    let stderr_handle = tokio::spawn(async move {
        let mut lines = Vec::new();
        let mut buf = String::new();
        loop {
            match stderr_reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let trimmed = buf.trim().to_string();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.clone());
                        // Also forward stderr as log progress to frontend
                        let _ = app2.emit("uvr-separate-progress", &SeparateProgressPayload {
                            event_id: event_id2.clone(),
                            kind: "log".to_string(),
                            step: String::new(),
                            current: 0,
                            total: 0,
                            percent: 0,
                            message: trimmed,
                            result: None,
                        });
                    }
                    buf.clear();
                }
            }
        }
        lines
    });

    loop {
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim().to_string();
                if trimmed.starts_with("[UVR_PROGRESS]") {
                    let payload = parse_uvr_progress(&trimmed, &event_id);
                    let _ = app.emit("uvr-separate-progress", &payload);
                } else if !trimmed.is_empty() {
                    let _ = app.emit("uvr-separate-progress", &SeparateProgressPayload {
                        event_id: event_id.clone(),
                        kind: "log".to_string(),
                        step: String::new(),
                        current: 0,
                        total: 0,
                        percent: 0,
                        message: trimmed.clone(),
                        result: None,
                    });
                }
                if trimmed.starts_with('{') {
                    last_json_line = trimmed;
                }
                line.clear();
            }
            Err(e) => {
                let _ = app.emit("uvr-separate-progress", &SeparateProgressPayload {
                    event_id: event_id.clone(),
                    kind: "log".to_string(),
                    step: String::new(),
                    current: 0,
                    total: 0,
                    percent: 0,
                    message: format!("stdout read error: {}", e),
                    result: None,
                });
                break;
            }
        }
    }

    let stderr_captured = stderr_handle.await.unwrap_or_default();
    let status = child.wait().await.map_err(|e| format!("分离进程错误: {}", e))?;

    if !status.success() {
        let stderr_msg = stderr_captured.join("\n");
        return Err(format!("分离失败: {}", if stderr_msg.is_empty() { "未知错误" } else { &stderr_msg }));
    }

    if last_json_line.is_empty() {
        return Err("分离完成但未获取到结果".to_string());
    }

    let response: SeparateResponse = serde_json::from_str(&last_json_line)
        .map_err(|e| format!("解析结果失败: {}", e))?;

    Ok(response)
}

fn parse_uvr_progress(line: &str, event_id: &str) -> SeparateProgressPayload {
    let after_tag = line.strip_prefix("[UVR_PROGRESS]").unwrap_or(line);

    let file_tag_end = after_tag.find(']').unwrap_or(0);
    let step_start = if file_tag_end > 0 && after_tag.as_bytes().get(0) == Some(&b'[') {
        after_tag[file_tag_end + 1..].trim_start()
    } else {
        after_tag.trim_start()
    };

    let (step, rest) = if step_start.starts_with("[file]") {
        ("file", step_start.strip_prefix("[file] ").unwrap_or(step_start))
    } else if step_start.starts_with("[segment]") {
        ("segment", step_start.strip_prefix("[segment] ").unwrap_or(step_start))
    } else {
        ("", step_start)
    };

    let parts: Vec<&str> = rest.splitn(3, ' ').collect();
    let (current, total, percent, message) = if parts.len() >= 3 {
        let cur = parts[0].split('/').next().unwrap_or("0").parse().unwrap_or(0);
        let tot = parts[0].split('/').nth(1).unwrap_or("1").parse().unwrap_or(1);
        let pct = parts[1].trim_end_matches('%').parse().unwrap_or(0);
        (cur, tot, pct, parts.get(2).unwrap_or(&"").to_string())
    } else {
        (0, 0, 0, rest.to_string())
    };

    SeparateProgressPayload {
        event_id: event_id.to_string(),
        kind: "progress".to_string(),
        step: step.to_string(),
        current,
        total,
        percent,
        message,
        result: None,
    }
}
