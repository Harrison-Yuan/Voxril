use crate::download_settings::DownloadSettings;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub async fn get_download_settings(
    state: State<'_, AppState>,
) -> Result<DownloadSettings, String> {
    Ok(DownloadSettings::load(&state.db).await.normalized())
}

#[tauri::command]
pub async fn set_download_settings(
    settings: DownloadSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    settings.save(&state.db).await
}
