use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowStatusResponse {
    pub status: Option<String>,
}

#[tauri::command]
pub async fn get_workflow(
    _exp_dir: String,
    _state: State<'_, AppState>,
) -> Result<WorkflowStatusResponse, String> {
    Ok(WorkflowStatusResponse { status: None })
}

#[tauri::command]
pub async fn save_workflow(
    _status: String,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn delete_workflow(
    _exp_dir: String,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    Ok(())
}
