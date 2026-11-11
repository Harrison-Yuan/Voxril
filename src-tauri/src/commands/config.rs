use crate::database::TrainConfig;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigResponse {
    pub config: Option<TrainConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AllConfigsResponse {
    pub configs: Vec<TrainConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LastExperimentResponse {
    pub exp_dir: Option<String>,
}

#[tauri::command]
pub async fn get_config(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<ConfigResponse, String> {
    let config = state.db
        .get_config(&exp_dir)
        .await
        .map_err(|e| format!("Failed to get config: {}", e))?;
    Ok(ConfigResponse { config })
}

#[tauri::command]
pub async fn get_all_configs(
    state: State<'_, AppState>,
) -> Result<AllConfigsResponse, String> {
    let configs = state.db
        .get_all_configs()
        .await
        .map_err(|e| format!("Failed to get all configs: {}", e))?;
    Ok(AllConfigsResponse { configs })
}

#[tauri::command]
pub async fn save_config(
    config: TrainConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let exp_dir = config.exp_dir.clone();
    state.db
        .save_config(config)
        .await
        .map_err(|e| format!("Failed to save config: {}", e))?;
    
    state.db
        .set_setting("last_experiment".to_string(), exp_dir)
        .await
        .map_err(|e| format!("Failed to save last experiment: {}", e))?;
    
    Ok(())
}

#[tauri::command]
pub async fn delete_config(
    _exp_dir: String,
    _state: State<'_, AppState>,
) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn get_last_experiment(
    state: State<'_, AppState>,
) -> Result<LastExperimentResponse, String> {
    let exp_dir = state.db
        .get_setting("last_experiment")
        .await
        .map_err(|e| format!("Failed to get last experiment: {}", e))?;
    Ok(LastExperimentResponse { exp_dir })
}

#[tauri::command]
pub async fn set_last_experiment(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.db
        .set_setting("last_experiment".to_string(), exp_dir)
        .await
        .map_err(|e| format!("Failed to set last experiment: {}", e))
}
