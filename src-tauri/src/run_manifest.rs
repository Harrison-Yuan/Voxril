//! 批次（run）清单：id 生成、目录指纹、pipeline 版本、run.json 读写。
//!
//! 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §3。
//! 该模块只负责"批次自身"的元数据，不涉及目录布局（见 workspace_layout）。

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// 批次清单文件名，固定放在批次目录根部。
pub const RUN_MANIFEST_FILE: &str = "run.json";

/// 指纹算法版本。改变被哈希的内容或顺序时递增，旧批次会自动降级为"需重跑"。
const FINGERPRINT_SCHEMA: &str = "v1";

/// 参与 pipeline_version 的脚本。任一文件内容变化都会使旧批次的 stage 失效。
const PIPELINE_FILES: &[&str] = &[
    "infer/modules/train/preprocess_optimized.py",
    "infer/modules/train/extract/extract_f0_print.py",
    "infer/modules/train/extract_feature_print.py",
    "infer/modules/train/train.py",
    "infer/lib/train/process_ckpt.py",
    "infer/lib/train/utils.py",
];

/// 参与指纹计算的目录名，内容全为可再生缓存，不应影响批次一致性。
const IGNORED_DIRS: &[&str] = &["__pycache__"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Running,
    Done,
    /// 产物仍在，但 dataset 指纹或 pipeline_version 变了 —— UI 应提示重跑。
    Stale,
    /// 产物已被主动回收（可再生，当前不可用）。与"需重跑"不同，不是异常。
    Reclaimed,
    Failed,
}

impl Default for StageStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageInfo {
    #[serde(default)]
    pub status: StageStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ntotal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_ivf: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunStages {
    #[serde(default)]
    pub preprocess: StageInfo,
    #[serde(default)]
    pub f0: StageInfo,
    #[serde(default)]
    pub feature: StageInfo,
    #[serde(default)]
    pub train: StageInfo,
    #[serde(default)]
    pub index: StageInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetInfo {
    /// 数据集来源目录（相对 workspace 根）。
    pub source: String,
    /// 排序后的 (相对路径, size) 列表哈希。
    pub fingerprint: String,
    #[serde(default)]
    pub splits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrainParams {
    pub epochs: u32,
    pub batch_size: u32,
    pub save_epoch: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f0_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub np: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    /// weights | index
    pub kind: String,
    /// 相对 `models/<voice>/` 的路径。
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reclaimable {
    #[serde(default)]
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reclaimed_at: Option<i64>,
}

/// run.json 的完整结构。字段顺序与服务端序列化输出一致，便于人工比对。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    /// 建立批次的时间戳（秒）。run_id 只精确到分钟，秒级时间单独记录。
    #[serde(default)]
    pub created_at: i64,
    pub voice: String,
    pub sr: String,
    pub version: String,
    pub if_f0: bool,
    pub pipeline_version: String,
    pub dataset: DatasetInfo,
    /// 训练参数；批次刚建立、尚未开始训练时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<TrainParams>,
    #[serde(default)]
    pub stages: RunStages,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    #[serde(default)]
    pub reclaimable: Reclaimable,
}

impl RunManifest {
    /// 新建批次时只用得到骨架，后续 stage 由各阶段回填。
    pub fn new(
        run_id: impl Into<String>,
        voice: impl Into<String>,
        sr: impl Into<String>,
        version: impl Into<String>,
        if_f0: bool,
        pipeline_version: impl Into<String>,
        dataset: DatasetInfo,
        params: Option<TrainParams>,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            created_at: now_ts(),
            voice: voice.into(),
            sr: sr.into(),
            version: version.into(),
            if_f0,
            pipeline_version: pipeline_version.into(),
            dataset,
            params,
            stages: RunStages::default(),
            assets: Vec::new(),
            reclaimable: Reclaimable::default(),
        }
    }

    /// 当 dataset 指纹或 pipeline 版本变化时，把已完成阶段降级为 stale。
    /// 返回是否有阶段被降级，供调用方决定是否提示用户。
    pub fn invalidate_if_changed(&mut self, dataset_fingerprint: &str, pipeline_version: &str) -> bool {
        let dataset_changed = self.dataset.fingerprint != dataset_fingerprint;
        let pipeline_changed = self.pipeline_version != pipeline_version;
        if !dataset_changed && !pipeline_changed {
            return false;
        }
        let mut changed = false;
        for stage in self.stages_mut() {
            if stage.status == StageStatus::Done {
                stage.status = StageStatus::Stale;
                changed = true;
            }
        }
        changed
    }

    fn stages_mut(&mut self) -> [&mut StageInfo; 5] {
        [
            &mut self.stages.preprocess,
            &mut self.stages.f0,
            &mut self.stages.feature,
            &mut self.stages.train,
            &mut self.stages.index,
        ]
    }
}

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

/// `%y%m%d-%H%M`，例：`261122-0156`。字典序即时间序。
pub fn run_id_at(ts: DateTime<Local>) -> String {
    ts.format("%y%m%d-%H%M").to_string()
}

pub fn new_run_id() -> String {
    run_id_at(Local::now())
}

/// 同一分钟内重复建批时追加 `-2`、`-3`……
pub fn unique_run_id(taken: &[String], ts: DateTime<Local>) -> String {
    let base = run_id_at(ts);
    if !taken.iter().any(|t| t == &base) {
        return base;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{}-{}", base, n);
        if !taken.iter().any(|t| t == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// 校验 run_id 形态：`YYMMDD-HHMM`（11 字符），允许 `-N` 冲突后缀。
pub fn is_valid_run_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 11 || bytes.len() > 20 {
        return false;
    }
    let digits = |range: &[usize]| range.iter().all(|&i| bytes[i].is_ascii_digit());
    if !digits(&[0, 1, 2, 3, 4, 5]) || bytes[6] != b'-' || !digits(&[7, 8, 9, 10]) {
        return false;
    }
    if bytes.len() == 11 {
        return true;
    }
    // 冲突后缀：-2、-3……
    bytes[11] == b'-'
        && bytes.len() > 12
        && bytes[12] != b'0'
        && bytes[12..].iter().all(|b| b.is_ascii_digit())
}

fn hex64(v: u64) -> String {
    format!("{:016x}", v)
}

/// 对 (相对路径, 字节数) 列表求指纹，顺序无关。
pub fn fingerprint_entries<I>(entries: I) -> String
where
    I: IntoIterator<Item = (String, u64)>,
{
    let mut sorted: Vec<(String, u64)> = entries.into_iter().collect();
    sorted.sort_unstable();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |hash: &mut u64, bytes: &[u8]| {
        for b in bytes {
            *hash ^= *b as u64;
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(&mut hash, FINGERPRINT_SCHEMA.as_bytes());
    feed(&mut hash, b"\n");
    for (path, size) in &sorted {
        feed(&mut hash, path.as_bytes());
        feed(&mut hash, b"\0");
        feed(&mut hash, &size.to_le_bytes());
        feed(&mut hash, b"\n");
    }
    hex64(hash)
}

/// 目录指纹：递归收集 (相对路径, size)。忽略 `__pycache__` 与 `.DS_Store`。
/// 用于 `dataset.fingerprint` 与 `stages.*.manifest_hash` 两个场景。
pub fn fingerprint_dir(dir: &Path) -> Result<String> {
    if !dir.exists() {
        anyhow::bail!("目录不存在: {}", dir.display());
    }
    let mut entries: BTreeMap<String, u64> = BTreeMap::new();
    let walker = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_str().unwrap_or("");
            !IGNORED_DIRS.contains(&name)
        });
    for item in walker {
        let item = item.with_context(|| format!("遍历目录失败: {}", dir.display()))?;
        let path = item.path();
        if !item.file_type().is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == ".DS_Store" {
            continue;
        }
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let size = fs::metadata(path)
            .map(|m| m.len())
            .unwrap_or(0);
        entries.insert(rel, size);
    }
    Ok(fingerprint_entries(entries))
}

/// Python 管线代码的内容哈希。脚本缺失时返回空串，由调用方决定是否阻断。
pub fn pipeline_version(project_root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(FINGERPRINT_SCHEMA.as_bytes());
    for rel in PIPELINE_FILES {
        let path = project_root.join(rel);
        feed(rel.as_bytes());
        feed(b"\0");
        match fs::read(&path) {
            Ok(content) => {
                feed(&content.len().to_le_bytes());
                feed(&content);
            }
            Err(_) => feed(b"<missing>"),
        }
        feed(b"\n");
    }
    hex64(hash)
}

pub fn manifest_path(run_dir: &Path) -> PathBuf {
    run_dir.join(RUN_MANIFEST_FILE)
}

pub fn read_manifest(path: &Path) -> Result<RunManifest> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("读取 run.json 失败: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("解析 run.json 失败: {}", path.display()))
}

/// 先写临时文件再 rename，避免中途崩溃留下半截 JSON。
pub fn write_manifest(path: &Path, manifest: &RunManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建批次目录失败: {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).with_context(|| format!("写入临时清单失败: {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("提交清单失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(y, m, d, h, min, 0).unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voxril-run-manifest-{}-{}", tag, uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn run_id_is_sortable_and_fixed_length() {
        let id = run_id_at(at(2026, 11, 22, 1, 56));
        assert_eq!(id, "261122-0156");
        assert_eq!(id.len(), 11);
        assert!(is_valid_run_id(&id));
    }

    #[test]
    fn unique_run_id_appends_suffix_on_conflict() {
        let ts = at(2026, 11, 22, 1, 56);
        assert_eq!(unique_run_id(&[], ts), "261122-0156");
        assert_eq!(unique_run_id(&["261122-0156".to_string()], ts), "261122-0156-2");
        let taken = vec!["261122-0156".to_string(), "261122-0156-2".to_string()];
        assert_eq!(unique_run_id(&taken, ts), "261122-0156-3");
        assert!(is_valid_run_id("261122-0156-12"));
    }

    #[test]
    fn run_id_rejects_bad_shapes() {
        for bad in ["2611220156", "2611-2201", "261122-01a6", "261122-0156-", "261122-0156-0"] {
            assert!(!is_valid_run_id(bad), "应拒绝: {}", bad);
        }
    }

    #[test]
    fn fingerprint_is_order_independent_and_size_sensitive() {
        let a = fingerprint_entries(vec![
            ("b.wav".to_string(), 10),
            ("a.wav".to_string(), 20),
        ]);
        let b = fingerprint_entries(vec![
            ("a.wav".to_string(), 20),
            ("b.wav".to_string(), 10),
        ]);
        assert_eq!(a, b);

        let c = fingerprint_entries(vec![
            ("a.wav".to_string(), 21),
            ("b.wav".to_string(), 10),
        ]);
        assert_ne!(a, c);
    }

    #[test]
    fn fingerprint_dir_detects_changes_and_ignores_cache() {
        let dir = temp_dir("fp");
        fs::write(dir.join("a.wav"), b"1234").unwrap();
        fs::write(dir.join(".DS_Store"), b"x").unwrap();
        fs::create_dir_all(dir.join("__pycache__")).unwrap();
        fs::write(dir.join("__pycache__").join("x.pyc"), b"cache").unwrap();

        let first = fingerprint_dir(&dir).unwrap();

        // 缓存与系统文件不影响指纹
        fs::write(dir.join("__pycache__").join("y.pyc"), b"cache2").unwrap();
        fs::write(dir.join(".DS_Store"), b"yy").unwrap();
        assert_eq!(first, fingerprint_dir(&dir).unwrap());

        // 新增内容文件会改变指纹
        fs::write(dir.join("b.wav"), b"5").unwrap();
        let second = fingerprint_dir(&dir).unwrap();
        assert_ne!(first, second);

        // 删除文件同样会改变指纹
        fs::remove_file(dir.join("b.wav")).unwrap();
        assert_eq!(first, fingerprint_dir(&dir).unwrap());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pipeline_version_tracks_script_content() {
        let root = temp_dir("pipe");
        for rel in PIPELINE_FILES {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, b"print('v1')").unwrap();
        }
        let v1 = pipeline_version(&root);
        fs::write(root.join(PIPELINE_FILES[0]), b"print('v2')").unwrap();
        let v2 = pipeline_version(&root);
        assert_ne!(v1, v2);
        assert_eq!(v1.len(), 16);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn manifest_json_roundtrip_matches_schema() {
        let json = r#"{
          "run_id": "261122-0156",
          "created_at": 1763776560,
          "voice": "harrison",
          "sr": "40k",
          "version": "v2",
          "if_f0": true,
          "pipeline_version": "abc123",
          "dataset": { "source": "work/datasets/harrison", "fingerprint": "fp1", "splits": 371, "total_seconds": 1842.6 },
          "params": { "epochs": 150, "batch_size": 4, "save_epoch": 10, "f0_method": "rmvpe", "np": 4 },
          "stages": {
            "preprocess": { "status": "done", "manifest_hash": "h1" },
            "f0": { "status": "done", "manifest_hash": "h2" },
            "feature": { "status": "done", "manifest_hash": "h3" },
            "train": { "status": "done", "epoch": 150, "started_at": 1, "finished_at": 2 },
            "index": { "status": "done", "ntotal": 48228, "n_ivf": 1236 }
          },
          "assets": [
            { "kind": "weights", "path": "weights/harrison-261122-0156-e150.pth", "bytes": 55187535 }
          ],
          "reclaimable": { "bytes": 933000000, "reclaimed_at": null }
        }"#;
        let manifest: RunManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.run_id, "261122-0156");
        assert_eq!(manifest.created_at, 1763776560);
        assert_eq!(manifest.stages.index.ntotal, Some(48228));
        assert_eq!(manifest.reclaimable.reclaimed_at, None);

        let back = serde_json::to_string(&manifest).unwrap();
        let again: RunManifest = serde_json::from_str(&back).unwrap();
        assert_eq!(manifest, again);
    }

    #[test]
    fn write_and_read_manifest_roundtrip() {
        let dir = temp_dir("write");
        let manifest = RunManifest::new(
            "261122-0156",
            "harrison",
            "40k",
            "v2",
            true,
            "pv",
            DatasetInfo {
                source: "work/datasets/harrison".to_string(),
                fingerprint: "fp".to_string(),
                splits: 10,
                total_seconds: Some(60.0),
            },
            Some(TrainParams {
                epochs: 150,
                batch_size: 4,
                save_epoch: 10,
                f0_method: Some("rmvpe".to_string()),
                np: Some(4),
            }),
        );
        let path = manifest_path(&dir);
        write_manifest(&path, &manifest).unwrap();
        assert_eq!(read_manifest(&path).unwrap(), manifest);
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn invalidate_marks_done_stages_stale() {
        let mut manifest = RunManifest::new(
            "261122-0156",
            "harrison",
            "40k",
            "v2",
            true,
            "pv",
            DatasetInfo {
                source: "s".to_string(),
                fingerprint: "fp".to_string(),
                splits: 0,
                total_seconds: None,
            },
            Some(TrainParams {
                epochs: 1,
                batch_size: 1,
                save_epoch: 1,
                f0_method: None,
                np: None,
            }),
        );
        manifest.stages.preprocess.status = StageStatus::Done;
        manifest.stages.preprocess.manifest_hash = Some("h".to_string());
        manifest.stages.train.status = StageStatus::Pending;

        assert!(manifest.invalidate_if_changed("other", "pv"));
        assert_eq!(manifest.stages.preprocess.status, StageStatus::Stale);
        assert_eq!(manifest.stages.train.status, StageStatus::Pending);
        assert!(!manifest.invalidate_if_changed("other", "pv"));
    }
}
