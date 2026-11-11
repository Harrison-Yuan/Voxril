use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceResponse {
    pub workspace: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CleanupResponse {
    pub freed_gb: f64,
    pub message: String,
}

#[tauri::command]
pub async fn get_workspace(state: State<'_, AppState>) -> Result<WorkspaceResponse, String> {
    let workspace = state.workspace_root.read().await;
    Ok(WorkspaceResponse {
        workspace: workspace.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn set_workspace(
    workspace: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceResponse, String> {
    let workspace_path = std::path::PathBuf::from(&workspace);
    
    tokio::fs::create_dir_all(&workspace_path)
        .await
        .map_err(|e| format!("Failed to create workspace directory: {}", e))?;

    let mut workspace_root = state.workspace_root.write().await;
    *workspace_root = workspace_path.clone();

    let config_path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".rvc-workspace-config.json");
    let config = serde_json::json!({
        "workspaceRoot": workspace
    });
    
    tokio::fs::write(&config_path, serde_json::to_string_pretty(&config).unwrap())
        .await
        .map_err(|e| format!("Failed to save workspace config: {}", e))?;

    Ok(WorkspaceResponse { workspace })
}

#[tauri::command]
pub async fn select_workspace(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    
    let folder = app
        .dialog()
        .file()
        .set_title("选择工作目录")
        .blocking_pick_folder();

    Ok(folder.map(|p| p.to_string()))
}

/// 深度清理构建缓存（target/、web/dist/、node_modules/.cache 等）
/// 由用户在设置页手动触发，不清除模型/音频/配置等资产。
#[tauri::command]
pub async fn deep_clean_cache(state: State<'_, AppState>) -> Result<CleanupResponse, String> {
    let project = state.project_root.clone();
    let ws = state.workspace_root.read().await.clone();

    // 在异步上下文中执行同步清理
    let freed_bytes = tokio::task::spawn_blocking(move || {
        crate::cleanup::deep_clean(&project, &ws)
    })
    .await
    .map_err(|e| format!("清理任务执行失败: {}", e))?;

    let freed_gb = freed_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let message = if freed_gb > 0.1 {
        format!("释放了 {:.2} GB 磁盘空间", freed_gb)
    } else {
        "没有需要清理的构建缓存".to_string()
    };

    Ok(CleanupResponse { freed_gb, message })
}
