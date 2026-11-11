use crate::state::AppState;
use crate::utils;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

const DATA_DIR: &str = "data";
const MODEL_DIR: &str = "models";

#[derive(Debug, Serialize, Deserialize)]
pub struct FileExistsResponse {
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobFilesResponse {
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioFile {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioFilesResponse {
    pub files: Vec<AudioFile>,
}

#[tauri::command]
pub async fn file_exists(path: String) -> Result<FileExistsResponse, String> {
    let path = PathBuf::from(path);
    Ok(FileExistsResponse {
        exists: path.exists(),
    })
}

#[tauri::command]
pub async fn list_artifacts(
    dir: String,
    kind: String,
    _state: State<'_, AppState>,
) -> Result<GlobFilesResponse, String> {
    let dir_path = PathBuf::from(dir);
    let files = utils::glob_files(&dir_path, &kind)
        .map_err(|e| format!("Failed to glob files: {}", e))?;

    Ok(GlobFilesResponse {
        files: files
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointIterationResponse {
    pub iteration: i32,
}

#[tauri::command]
pub async fn get_checkpoint_iteration(path: String) -> Result<CheckpointIterationResponse, String> {
    let re = regex::Regex::new(r"G_(\d+)\.pth$").map_err(|e| e.to_string())?;
    
    if let Some(caps) = re.captures(&path) {
        if let Some(iter_str) = caps.get(1) {
            let iteration = iter_str.as_str().parse::<i32>().unwrap_or(0);
            return Ok(CheckpointIterationResponse { iteration });
        }
    }
    
    Ok(CheckpointIterationResponse { iteration: 0 })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadTextResponse {
    pub content: String,
}

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<ReadTextResponse, String> {
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    Ok(ReadTextResponse { content })
}

#[allow(dead_code)]
#[tauri::command]
pub async fn read_text(path: String) -> Result<String, String> {
    tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
pub async fn list_audio_files(dir: String) -> Result<AudioFilesResponse, String> {
    let dir_path = PathBuf::from(dir);
    let extensions = ["wav", "mp3", "flac", "m4a", "ogg", "aac"];

    let files = utils::collect_files_with_extension(&dir_path, &extensions)
        .map_err(|e| format!("Failed to list audio files: {}", e))?;

    Ok(AudioFilesResponse {
        files: files
            .into_iter()
            .map(|p| AudioFile {
                name: p.file_name().unwrap().to_string_lossy().to_string(),
                path: p.to_string_lossy().to_string(),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn open_in_finder(path: String) -> Result<(), String> {
    use std::process::Command;

    Command::new("open")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open in Finder: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn open_in_vscode(path: String) -> Result<(), String> {
    use std::process::Command;

    Command::new("code")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("Failed to open in VS Code: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn delete_file(path: String) -> Result<(), String> {
    tokio::fs::remove_file(&path)
        .await
        .map_err(|e| format!("Failed to delete file: {}", e))
}

#[tauri::command]
pub async fn delete_training_result(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let logs_path = state.project_root.join("logs").join(&exp_dir);
    let workspace_root = state.workspace_root.read().await;
    let model_path = workspace_root.join(MODEL_DIR).join(&exp_dir);
    let data_path = workspace_root.join(DATA_DIR).join(&exp_dir);

    let mut deleted = Vec::new();
    let mut kept = Vec::new();

    if logs_path.exists() {
        tokio::fs::remove_dir_all(&logs_path)
            .await
            .map_err(|e| format!("Failed to delete logs directory: {}", e))?;
        deleted.push("训练日志(旧)".to_string());
    }

    if model_path.exists() {
        if let Ok(mut entries) = tokio::fs::read_dir(&model_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();

                let should_delete = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) && (
                    name_str == "eval"
                    || name_str.starts_with("events.out.tfevents")
                )
                || name_str.ends_with(".pth")
                || name_str.ends_with(".index")
                || name_str.ends_with(".npy") && name_str.starts_with("total_fea")
                || name_str.ends_with(".log")
                || name_str.starts_with("events.out.tfevents");

                if should_delete {
                    if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                        tokio::fs::remove_dir_all(entry.path()).await.ok();
                    } else {
                        tokio::fs::remove_file(entry.path()).await.ok();
                    }
                    deleted.push(name_str);
                } else {
                    kept.push(name_str);
                }
            }
        }
    }

    if data_path.exists() {
        kept.push(format!("预处理数据({})", data_path.display()));
    }

    let message = if deleted.is_empty() {
        "没有需要删除的训练产物".to_string()
    } else {
        format!(
            "已删除: {}\n预处理数据保存在 {}，可直接重新训练",
            deleted.join(", "),
            data_path.display()
        )
    };

    Ok(message)
}

#[tauri::command]
pub async fn clear_training_artifacts(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    const EXPERIMENTS_DIR: &str = "experiments";

    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);

    if !exp_path.exists() {
        return Err(format!("实验目录不存在: {}", exp_path.display()));
    }

    let mut deleted = Vec::new();

    // Delete checkpoint files (G_*.pth, D_*.pth)
    if let Ok(mut entries) = tokio::fs::read_dir(&exp_path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name.starts_with("G_") || name.starts_with("D_")) && name.ends_with(".pth") {
                tokio::fs::remove_file(entry.path()).await.ok();
                deleted.push(name);
            }
        }
    }

    // Delete checkpoints subdirectory
    let ckpt_dir = exp_path.join("checkpoints");
    if ckpt_dir.exists() {
        tokio::fs::remove_dir_all(&ckpt_dir).await.ok();
        deleted.push("checkpoints/".to_string());
    }

    // Delete model.pth
    let model_pth = exp_path.join("model.pth");
    if model_pth.exists() {
        tokio::fs::remove_file(&model_pth).await.ok();
        deleted.push("model.pth".to_string());
    }

    // Delete index files
    let index_files = utils::glob_files(&exp_path, "index").unwrap_or_default();
    for p in &index_files {
        tokio::fs::remove_file(p).await.ok();
        deleted.push(p.file_name().unwrap().to_string_lossy().to_string());
    }

    // Delete total_fea.npy (legacy — no longer used but may exist from old builds)
    let total_fea = exp_path.join("total_fea.npy");
    if total_fea.exists() {
        tokio::fs::remove_file(&total_fea).await.ok();
        deleted.push("total_fea.npy".to_string());
    }

    // Delete logs and config
    for name in &["train.log", "config.json", "filelist.txt"] {
        let p = exp_path.join(name);
        if p.exists() {
            tokio::fs::remove_file(&p).await.ok();
            deleted.push(name.to_string());
        }
    }

    // Delete eval directory
    let eval_dir = exp_path.join("eval");
    if eval_dir.exists() {
        tokio::fs::remove_dir_all(&eval_dir).await.ok();
        deleted.push("eval/".to_string());
    }

    // Delete preprocessing/feature extraction outputs (audio, f0, features)
    // These must be regenerated if the preprocessing code has changed
    for name in &["0_gt_wavs", "1_16k_wavs", "2a_f0", "2b-f0nsf", "3_feature256", "3_feature768"] {
        let p = exp_path.join(name);
        if p.exists() {
            tokio::fs::remove_dir_all(&p).await.ok();
            deleted.push(format!("{}/", name));
        }
    }

    // Delete preprocessing/feature extraction logs
    for name in &["preprocess.log", "extract_f0_feature.log", "extract_audio_feature.log"] {
        let p = exp_path.join(name);
        if p.exists() {
            tokio::fs::remove_file(&p).await.ok();
            deleted.push(name.to_string());
        }
    }

    // Delete tensorboard events
    if let Ok(mut entries) = tokio::fs::read_dir(&exp_path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("events.out.tfevents") {
                tokio::fs::remove_file(entry.path()).await.ok();
                deleted.push(name);
            }
        }
    }

    let msg = if deleted.is_empty() {
        "没有可清理的训练产物".to_string()
    } else {
        format!("已清理 {} 个训练产物及预处理数据，可重新开始完整训练流程（预处理→特征提取→训练）", deleted.len())
    };
    Ok(msg)
}
