//! 版本检查：读取 GitHub 最新 Release，与当前应用版本比较。

use serde::Serialize;
use std::time::Duration;
use tauri::Manager;

const RELEASES_API: &str = "https://api.github.com/repos/Harrison-Yuan/Voxril/releases/latest";
const RELEASES_PAGE: &str = "https://github.com/Harrison-Yuan/Voxril/releases";

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub url: String,
    pub notes: String,
    pub has_update: bool,
}

fn parse_version(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);
    for index in 0..latest_parts.len().max(current_parts.len()) {
        let next = *latest_parts.get(index).unwrap_or(&0);
        let now = *current_parts.get(index).unwrap_or(&0);
        if next != now {
            return next > now;
        }
    }
    false
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let client = reqwest::Client::builder()
        .user_agent("Voxril-Updater")
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| format!("初始化网络请求失败: {}", e))?;

    let response = client
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("检查更新失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("检查更新失败: HTTP {}", response.status()));
    }

    let data: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析版本信息失败: {}", e))?;

    let tag = data["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let latest = if tag.is_empty() { current.clone() } else { tag };
    let url = data["html_url"]
        .as_str()
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    let notes = data["body"].as_str().unwrap_or("").to_string();

    Ok(UpdateInfo {
        has_update: is_newer(&latest, &current),
        current,
        latest,
        url,
        notes,
    })
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("只允许打开 https 链接".to_string());
    }
    crate::platform::open_path(&url).map_err(|e| format!("打开链接失败: {}", e))
}
