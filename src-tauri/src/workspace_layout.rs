//! 工作区目录布局：v1（现状）与 v2（目标）两套路径的唯一解析入口。
//!
//! 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §5。
//! 原则：`layout.json` 缺失即走 v1，保证不迁移也能用；所有路径拼接都必须经过本模块。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 布局标记文件，放在工作区根目录。
pub const LAYOUT_FILE: &str = "layout.json";
/// 目标布局版本。
pub const LAYOUT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutVersion {
    /// 现状：`experiments/`、`converted/`、`mixed/`、`separated/`、`uvr_models/` 平铺在根目录。
    V1,
    /// 目标：`inputs/`（进）、`work/`（算）、`outputs/`（出）、`models/`（留）。
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// 转换后的人声
    Vocal,
    /// 与人声混音后的成品
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationEntry {
    pub from: String,
    pub to: String,
}

/// `layout.json` 内容。迁移完成后写入，读侧据此决定路径语义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutFile {
    pub version: u32,
    pub migrated_at: i64,
    #[serde(default)]
    pub mapping: Vec<MigrationEntry>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceLayout {
    root: PathBuf,
    version: LayoutVersion,
}

impl WorkspaceLayout {
    /// 探测布局版本：`layout.json` 存在且 version >= 2 时为 v2，其余一律 v1。
    pub fn detect(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let version = match read_layout_file(&root) {
            Some(file) if file.version >= LAYOUT_SCHEMA_VERSION => LayoutVersion::V2,
            _ => LayoutVersion::V1,
        };
        Self { root, version }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn version(&self) -> LayoutVersion {
        self.version
    }

    pub fn is_v2(&self) -> bool {
        self.version == LayoutVersion::V2
    }

    /// 用户原始素材（不可再生）。
    pub fn inputs_raw_dir(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.clone(),
            LayoutVersion::V2 => self.root.join("inputs").join("raw"),
        }
    }

    /// 某个声音的训练源音频。
    pub fn inputs_training_dir(&self, voice: &str) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join(voice),
            LayoutVersion::V2 => self.root.join("inputs").join("training").join(voice),
        }
    }

    /// 分离后的人声/伴奏（v1：`separated/`，v2：`work/datasets/`）。
    pub fn datasets_root(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join("separated"),
            LayoutVersion::V2 => self.root.join("work").join("datasets"),
        }
    }

    /// 批次工作位根目录（v1：`experiments/`，v2：`work/runs/`）。
    pub fn runs_root(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join("experiments"),
            LayoutVersion::V2 => self.root.join("work").join("runs"),
        }
    }

    /// 批次工作位目录。`exp_dir` 形如 `<voice>/<run_id>`（v1 为 `<voice>/v2_40k`）。
    pub fn run_path(&self, exp_dir: &str) -> PathBuf {
        self.runs_root().join(exp_dir)
    }

    /// 派生缓存（v1：`uvr_models/`，v2：`work/cache/`）。
    pub fn cache_root(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join("uvr_models"),
            LayoutVersion::V2 => self.root.join("work").join("cache"),
        }
    }

    /// 一键翻唱任务中间产物根目录（v1：`cover_jobs/`，v2：`work/cover/`）。
    pub fn cover_jobs_root(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join("cover_jobs"),
            LayoutVersion::V2 => self.root.join("work").join("cover"),
        }
    }

    /// 一键翻唱任务的中间产物目录，每次生成独立一个，避免不同歌曲的人声/伴奏互相覆盖。
    pub fn cover_job_dir(&self, job_id: &str) -> PathBuf {
        self.cover_jobs_root().join(job_id)
    }

    /// 转换/混音产物的输出目录。v1 两个目录分离，v2 统一进 `outputs/draft`。
    pub fn output_dir(&self, kind: OutputKind) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => match kind {
                OutputKind::Vocal => self.root.join("converted"),
                OutputKind::Mixed => self.root.join("mixed"),
            },
            LayoutVersion::V2 => self.root.join("outputs").join("draft"),
        }
    }

    /// 用户定稿的成品（永不自动清理）。
    pub fn final_output_dir(&self) -> PathBuf {
        self.root.join("outputs").join("final")
    }

    /// 声音资产根目录（v1 与工作位共用 `experiments/`，v2 独立到 `models/`）。
    pub fn assets_root(&self) -> PathBuf {
        match self.version {
            LayoutVersion::V1 => self.root.join("experiments"),
            LayoutVersion::V2 => self.root.join("models"),
        }
    }

    /// 声音资产目录（v1：`experiments/<voice>/`，v2：`models/<voice>/`）。
    pub fn voice_assets_dir(&self, voice: &str) -> PathBuf {
        self.assets_root().join(voice)
    }

    /// 归档权重与索引目录。
    pub fn weights_dir(&self, voice: &str) -> PathBuf {
        self.voice_assets_dir(voice).join("weights")
    }

    /// 批次清单（run.json）归档目录。
    pub fn run_manifests_dir(&self, voice: &str) -> PathBuf {
        self.voice_assets_dir(voice).join("runs")
    }

    /// 删除中转站，所有回收动作先移到此处再过期清理。
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(".trash")
    }
}

/// `exp_dir` 的三种形态。前端只传声音名或 `<声音名>/<批次号>`，目录名的解析集中在此。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTarget {
    /// `<voice>`：尚未建批，由预处理创建。
    New { voice: String },
    /// `<voice>/<batch>`：已建批次。
    Batch { voice: String, run_id: String },
    /// `<voice>/<其它目录>`：旧布局工作位（如 `v2_40k`），不参与批次化。
    Legacy,
}

/// 单个路径段是否可作为工作区子目录名。
pub fn is_safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.chars().any(|c| c.is_control() || c == '/' || c == '\\')
}

/// 把 `exp_dir` 解析为三种形态之一。声音名来自用户输入，必须落在工作区子目录内。
/// 返回 `String` 错误，便于直接透传给前端。
pub fn parse_run_target(exp_dir: &str) -> std::result::Result<RunTarget, String> {
    let parts: Vec<&str> = exp_dir
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(voice) = parts.first() {
        if !is_safe_component(voice) {
            return Err(format!("声音名不合法：{}", voice));
        }
    }
    match parts.as_slice() {
        [voice] => Ok(RunTarget::New {
            voice: voice.to_string(),
        }),
        [voice, second] if crate::run_manifest::is_valid_run_id(second) => Ok(RunTarget::Batch {
            voice: voice.to_string(),
            run_id: second.to_string(),
        }),
        [_, _] => Ok(RunTarget::Legacy),
        _ => Err(format!(
            "实验目录名不合法：{}（应为 <声音名> 或 <声音名>/<批次号>）",
            exp_dir
        )),
    }
}

pub fn layout_file_path(root: &Path) -> PathBuf {
    root.join(LAYOUT_FILE)
}

pub fn read_layout_file(root: &Path) -> Option<LayoutFile> {
    let content = fs::read_to_string(layout_file_path(root)).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn write_layout_file(root: &Path, file: &LayoutFile) -> Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("创建工作区目录失败: {}", root.display()))?;
    let json = serde_json::to_string_pretty(file)?;
    let tmp = layout_file_path(root).with_extension("json.tmp");
    fs::write(&tmp, json).with_context(|| format!("写入 layout.json 失败: {}", tmp.display()))?;
    fs::rename(&tmp, layout_file_path(root))
        .with_context(|| "提交 layout.json 失败".to_string())?;
    Ok(())
}

/// 递归统计目录或文件的字节数；路径不存在时返回 0。
/// 用于体积提示与回收统计，硬链接会被重复计数（只作估算，不用于配额判断）。
pub fn bytes_of(path: &Path) -> u64 {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    if meta.is_file() {
        return meta.len();
    }
    let walker = walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != "__pycache__");
    let mut total = 0u64;
    for item in walker.flatten() {
        if item.file_type().is_file() {
            total += item.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 硬链接归档结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkOutcome {
    /// 同卷硬链接，零额外空间。
    Hardlink,
    /// 跨卷或链接失败，回落为复制。
    Copied,
}

/// 归档：优先硬链接，失败则复制。目标已存在时报错，避免静默覆盖历史批次。
pub fn hardlink_or_copy(src: &Path, dst: &Path) -> Result<LinkOutcome> {
    if !src.exists() {
        anyhow::bail!("归档源不存在: {}", src.display());
    }
    if dst.exists() {
        anyhow::bail!("归档目标已存在，拒绝覆盖: {}", dst.display());
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建归档目录失败: {}", parent.display()))?;
    }
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(LinkOutcome::Hardlink),
        Err(_) => {
            fs::copy(src, dst).with_context(|| {
                format!("归档复制失败: {} -> {}", src.display(), dst.display())
            })?;
            Ok(LinkOutcome::Copied)
        }
    }
}

/// 判断两个路径是否在同一卷（硬链接前提）。目标不存在时向上找最近的已存在祖先。
#[cfg(unix)]
pub fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    fn dev_of(path: &Path) -> Option<u64> {
        let mut cur = Some(path);
        while let Some(p) = cur {
            if let Ok(meta) = fs::metadata(p) {
                return Some(meta.dev());
            }
            cur = p.parent();
        }
        None
    }

    match (dev_of(a), dev_of(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(not(unix))]
pub fn same_volume(_a: &Path, _b: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "voxril-layout-{}-{}",
            tag,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_defaults_to_v1_without_layout_file() {
        let root = temp_root("v1");
        let layout = WorkspaceLayout::detect(&root);
        assert_eq!(layout.version(), LayoutVersion::V1);
        assert_eq!(layout.runs_root(), root.join("experiments"));
        assert_eq!(layout.output_dir(OutputKind::Vocal), root.join("converted"));
        assert_eq!(layout.output_dir(OutputKind::Mixed), root.join("mixed"));
        assert_eq!(layout.datasets_root(), root.join("separated"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn detect_reads_v2_layout_file() {
        let root = temp_root("v2");
        write_layout_file(
            &root,
            &LayoutFile {
                version: LAYOUT_SCHEMA_VERSION,
                migrated_at: 1,
                mapping: vec![MigrationEntry {
                    from: "experiments".to_string(),
                    to: "work/runs".to_string(),
                }],
            },
        )
        .unwrap();

        let layout = WorkspaceLayout::detect(&root);
        assert_eq!(layout.version(), LayoutVersion::V2);
        assert!(layout.is_v2());
        assert_eq!(layout.runs_root(), root.join("work").join("runs"));
        assert_eq!(layout.run_path("harrison/261122-0156"), root.join("work").join("runs").join("harrison").join("261122-0156"));
        assert_eq!(layout.datasets_root(), root.join("work").join("datasets"));
        assert_eq!(layout.cache_root(), root.join("work").join("cache"));
        assert_eq!(layout.output_dir(OutputKind::Vocal), root.join("outputs").join("draft"));
        assert_eq!(layout.output_dir(OutputKind::Mixed), root.join("outputs").join("draft"));
        assert_eq!(layout.final_output_dir(), root.join("outputs").join("final"));
        assert_eq!(layout.weights_dir("harrison"), root.join("models").join("harrison").join("weights"));
        assert_eq!(layout.run_manifests_dir("harrison"), root.join("models").join("harrison").join("runs"));
        assert_eq!(layout.inputs_training_dir("harrison"), root.join("inputs").join("training").join("harrison"));
        assert_eq!(layout.trash_dir(), root.join(".trash"));

        let file = read_layout_file(&root).unwrap();
        assert_eq!(file.version, LAYOUT_SCHEMA_VERSION);
        assert_eq!(file.mapping.len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn v1_paths_stay_unchanged() {
        let root = temp_root("v1paths");
        let layout = WorkspaceLayout::detect(&root);
        assert_eq!(layout.run_path("harrison/v2_40k"), root.join("experiments").join("harrison").join("v2_40k"));
        assert_eq!(layout.cache_root(), root.join("uvr_models"));
        assert_eq!(layout.weights_dir("harrison"), root.join("experiments").join("harrison").join("weights"));
        assert_eq!(layout.inputs_raw_dir(), root);
        assert_eq!(layout.inputs_training_dir("harrison"), root.join("harrison"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn hardlink_or_copy_creates_link_then_refuses_overwrite() {
        let root = temp_root("link");
        let src = root.join("model.pth");
        let dst = root.join("weights").join("harrison-261122-0156-e150.pth");
        fs::write(&src, b"weights").unwrap();

        let outcome = hardlink_or_copy(&src, &dst).unwrap();
        assert_eq!(outcome, LinkOutcome::Hardlink);
        assert_eq!(fs::read(&dst).unwrap(), b"weights");

        // 已存在目标必须报错，避免覆盖历史批次
        assert!(hardlink_or_copy(&src, &dst).is_err());

        // 源缺失时报错
        assert!(hardlink_or_copy(&root.join("missing.pth"), &root.join("x.pth")).is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn same_volume_is_true_within_one_temp_dir() {
        let root = temp_root("vol");
        fs::create_dir_all(root.join("a")).unwrap();
        assert!(same_volume(&root.join("a"), &root.join("b")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parse_run_target_distinguishes_three_forms() {
        assert_eq!(
            parse_run_target("harrison").unwrap(),
            RunTarget::New {
                voice: "harrison".to_string()
            }
        );
        assert_eq!(
            parse_run_target("harrison/261122-0156").unwrap(),
            RunTarget::Batch {
                voice: "harrison".to_string(),
                run_id: "261122-0156".to_string()
            }
        );
        assert_eq!(parse_run_target("harrison/v2_40k").unwrap(), RunTarget::Legacy);
        assert!(parse_run_target("..").is_err());
        assert!(parse_run_target("harrison/v2_40k/extra").is_err());
    }

    #[test]
    fn bytes_of_sums_files_recursively() {
        let root = temp_root("bytes");
        fs::write(root.join("a.bin"), b"1234").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("b.bin"), b"123456").unwrap();
        fs::create_dir_all(root.join("sub").join("__pycache__")).unwrap();
        fs::write(root.join("sub").join("__pycache__").join("x.pyc"), b"ignored").unwrap();

        assert_eq!(bytes_of(&root), 10);
        assert_eq!(bytes_of(&root.join("a.bin")), 4);
        assert_eq!(bytes_of(&root.join("missing")), 0);
        fs::remove_dir_all(&root).ok();
    }
}
