//! 模型下载源配置：官方 / 国内镜像 / 自定义。
//!
//! 配置保存在 `app_settings` 表，启动自动下载与手动下载都会把它转成
//! `tools/check_download_models.py` 的命令行参数。

use crate::database::Database;
use serde::{Deserialize, Serialize};

const SETTING_KEY: &str = "model_download_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSettings {
    /// auto | official | cn | custom
    #[serde(default = "default_source")]
    pub source: String,
    /// 自定义 HuggingFace 端点，例如 https://hf-mirror.com
    #[serde(default)]
    pub hf_endpoint: String,
    /// 自定义 GitHub 加速前缀，例如 https://ghfast.top
    #[serde(default)]
    pub gh_proxy: String,
}

fn default_source() -> String {
    "auto".to_string()
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            source: default_source(),
            hf_endpoint: String::new(),
            gh_proxy: String::new(),
        }
    }
}

impl DownloadSettings {
    pub async fn load(db: &Database) -> Self {
        match db.get_setting(SETTING_KEY).await {
            Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
            _ => Self::default(),
        }
    }

    pub async fn save(&self, db: &Database) -> Result<(), String> {
        let normalized = self.normalized();
        let raw = serde_json::to_string(&normalized)
            .map_err(|e| format!("序列化下载设置失败: {}", e))?;
        db.set_setting(SETTING_KEY.to_string(), raw)
            .await
            .map_err(|e| format!("保存下载设置失败: {}", e))
    }

    pub fn normalized(&self) -> Self {
        let source = match self.source.as_str() {
            "official" | "cn" | "custom" => self.source.clone(),
            _ => default_source(),
        };
        Self {
            source,
            hf_endpoint: self.hf_endpoint.trim().trim_end_matches('/').to_string(),
            gh_proxy: self.gh_proxy.trim().trim_end_matches('/').to_string(),
        }
    }

    /// 传给 `tools/check_download_models.py` 的参数。
    pub fn cli_args(&self) -> Vec<String> {
        let normalized = self.normalized();
        let mut args = vec!["--source".to_string(), normalized.source];
        if !normalized.hf_endpoint.is_empty() {
            args.push("--hf-endpoint".to_string());
            args.push(normalized.hf_endpoint);
        }
        if !normalized.gh_proxy.is_empty() {
            args.push("--gh-proxy".to_string());
            args.push(normalized.gh_proxy);
        }
        args
    }
}
