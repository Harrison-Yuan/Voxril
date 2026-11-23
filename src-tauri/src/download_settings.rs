//! 模型下载源配置：官方 / 国内镜像 / 自定义。
//!
//! 配置保存在 `app_settings` 表，启动自动下载与手动下载都会把它转成
//! `tools/check_download_models.py` 的命令行参数。

use crate::database::Database;
use serde::{Deserialize, Serialize};

const SETTING_KEY: &str = "model_download_settings";

const TUNA_PYPI: &str = "https://pypi.tuna.tsinghua.edu.cn/simple";
const ALIYUN_PYPI: &str = "https://mirrors.aliyun.com/pypi/simple/";

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
    /// pip 源：auto | official | tuna | aliyun | custom
    #[serde(default = "default_source")]
    pub pip_source: String,
    /// 自定义 PyPI 索引地址
    #[serde(default)]
    pub pip_index: String,
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
            pip_source: default_source(),
            pip_index: String::new(),
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
        let pip_source = match self.pip_source.as_str() {
            "official" | "tuna" | "aliyun" | "custom" => self.pip_source.clone(),
            _ => default_source(),
        };
        Self {
            source,
            hf_endpoint: self.hf_endpoint.trim().trim_end_matches('/').to_string(),
            gh_proxy: self.gh_proxy.trim().trim_end_matches('/').to_string(),
            pip_source,
            pip_index: self.pip_index.trim().trim_end_matches('/').to_string(),
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

    /// 传给 `tools/auto_setup_venv.py` 的参数。
    pub fn pip_cli_args(&self) -> Vec<String> {
        let normalized = self.normalized();
        let mut args = vec!["--pip-source".to_string(), normalized.pip_source];
        if !normalized.pip_index.is_empty() {
            args.push("--pip-index".to_string());
            args.push(normalized.pip_index);
        }
        args
    }

    /// 运行时安装单个包时使用的索引地址；None 表示官方 PyPI。
    pub fn pip_index_url(&self) -> Option<String> {
        let normalized = self.normalized();
        match normalized.pip_source.as_str() {
            "tuna" => Some(TUNA_PYPI.to_string()),
            "aliyun" => Some(ALIYUN_PYPI.to_string()),
            "custom" if !normalized.pip_index.is_empty() => Some(normalized.pip_index),
            _ => None,
        }
    }
}
