use crate::database::TrainHistory;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub history: Vec<TrainHistory>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateHistoryRequest {
    pub completed_epoch: Option<i32>,
    pub status: Option<String>,
    pub end_time: Option<i64>,
    pub index_path: Option<String>,
}

#[tauri::command]
pub async fn get_all_history(
    state: State<'_, AppState>,
) -> Result<HistoryResponse, String> {
    let history = state.db
        .get_all_history()
        .await
        .map_err(|e| format!("Failed to get history: {}", e))?;
    Ok(HistoryResponse { history })
}

#[tauri::command]
pub async fn add_history(
    record: TrainHistory,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let id = record.id.clone();
    state.db
        .add_history(record)
        .await
        .map_err(|e| format!("Failed to add history: {}", e))?;
    Ok(id)
}

#[tauri::command]
pub async fn update_history(
    id: String,
    updates: UpdateHistoryRequest,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.db
        .update_history(
            id,
            updates.completed_epoch,
            updates.status,
            updates.end_time,
            updates.index_path,
        )
        .await
        .map_err(|e| format!("Failed to update history: {}", e))
}

#[tauri::command]
pub async fn delete_history(id: String, state: State<'_, AppState>) -> Result<(), String> {
    state.db
        .delete_history(id)
        .await
        .map_err(|e| format!("Failed to delete history: {}", e))
}

#[tauri::command]
pub async fn delete_all_history(state: State<'_, AppState>) -> Result<(), String> {
    state.db
        .delete_all_history()
        .await
        .map_err(|e| format!("Failed to delete all history: {}", e))
}
