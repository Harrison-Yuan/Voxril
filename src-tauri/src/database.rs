use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainConfig {
    pub exp_dir: String,
    pub sr: String,
    pub trainset_dir: String,
    pub np_process: i32,
    pub f0_method: String,
    pub if_f0: bool,
    pub save_epoch: i32,
    pub total_epoch: i32,
    pub batch_size: i32,
    pub if_save_latest: bool,
    pub if_save_every_weights: bool,
    pub gpus: String,
}

/// 训练批次索引（磁盘上的 `run.json` 是唯一真源，本表仅作查询索引）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub voice: String,
    pub sr: String,
    pub if_f0: bool,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub epochs: i32,
    pub dataset_fingerprint: String,
    pub pipeline_version: String,
    pub stages_json: String,
    pub assets_json: String,
    pub bytes_total: i64,
    pub reclaimed_at: Option<i64>,
    pub note: String,
}

/// 推理/混音成品索引。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputRecord {
    pub id: i64,
    /// converted | mixed
    pub kind: String,
    pub path: String,
    pub song: String,
    pub voice: String,
    pub run_id: Option<String>,
    pub transpose: i32,
    pub created_at: i64,
    pub bytes: i64,
    pub starred: bool,
    /// 是否已定稿（移入 `outputs/final`，不再自动清理）。
    pub finalized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainHistory {
    pub id: String,
    pub exp_dir: String,
    pub version: String,
    pub sr: String,
    pub f0_method: String,
    pub total_epoch: i32,
    pub completed_epoch: i32,
    pub status: String,
    pub start_time: i64,
    pub end_time: Option<i64>,
    pub config: String,
    pub index_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct WorkflowStatus {
    pub exp_dir: String,
    pub preprocess: String,
    pub features: String,
    pub train: String,
    pub index: String,
    pub train_epoch: i32,
    pub total_epoch: i32,
}

/// 数据库句柄。`Connection` 内部共享一个后台线程，克隆代价极低，
/// 便于在后台监视任务里登记批次状态。
#[derive(Clone)]
pub struct Database {
    conn: Connection,
}

impl Database {
    pub async fn new(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .await
            .context("Failed to open database connection")?;
        
        conn.call(|conn| {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS configs (
                    exp_dir TEXT PRIMARY KEY,
                    sr TEXT NOT NULL,
                    trainset_dir TEXT NOT NULL,
                    np_process INTEGER NOT NULL,
                    f0_method TEXT NOT NULL,
                    if_f0 INTEGER NOT NULL,
                    save_epoch INTEGER NOT NULL,
                    total_epoch INTEGER NOT NULL,
                    batch_size INTEGER NOT NULL,
                    if_save_latest INTEGER NOT NULL,
                    if_save_every_weights INTEGER NOT NULL,
                    gpus TEXT NOT NULL
                )",
                [],
            )?;

            // 旧库迁移：configs 去掉从未被读取的 version / run_id 列
            // （批次关联已由磁盘上的 run.json 承担，不再依赖配置表）
            {
                let mut drop_version = false;
                let mut drop_run_id = false;
                {
                    let mut stmt = conn.prepare("PRAGMA table_info(configs)")?;
                    let mut rows = stmt.query([])?;
                    while let Some(row) = rows.next()? {
                        let name: String = row.get(1)?;
                        if name == "version" { drop_version = true; }
                        if name == "run_id" { drop_run_id = true; }
                    }
                }
                if drop_version {
                    conn.execute("ALTER TABLE configs DROP COLUMN version", [])?;
                }
                if drop_run_id {
                    conn.execute("ALTER TABLE configs DROP COLUMN run_id", [])?;
                }
            }

            conn.execute(
                "CREATE TABLE IF NOT EXISTS history (
                    id TEXT PRIMARY KEY,
                    exp_dir TEXT NOT NULL,
                    version TEXT NOT NULL,
                    sr TEXT NOT NULL,
                    f0_method TEXT NOT NULL,
                    total_epoch INTEGER NOT NULL,
                    completed_epoch INTEGER NOT NULL,
                    status TEXT NOT NULL,
                    start_time INTEGER NOT NULL,
                    end_time INTEGER,
                    config TEXT NOT NULL,
                    index_path TEXT
                )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS workflow (
                    exp_dir TEXT PRIMARY KEY,
                    preprocess TEXT NOT NULL,
                    features TEXT NOT NULL,
                    train TEXT NOT NULL,
                    idx TEXT NOT NULL,
                    train_epoch INTEGER NOT NULL,
                    total_epoch INTEGER NOT NULL
                )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS runs (
                    run_id TEXT PRIMARY KEY,
                    voice TEXT NOT NULL,
                    sr TEXT NOT NULL,
                    if_f0 INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    finished_at INTEGER,
                    epochs INTEGER NOT NULL,
                    dataset_fingerprint TEXT NOT NULL,
                    pipeline_version TEXT NOT NULL,
                    stages_json TEXT NOT NULL,
                    assets_json TEXT NOT NULL,
                    bytes_total INTEGER NOT NULL,
                    reclaimed_at INTEGER,
                    note TEXT NOT NULL
                )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS outputs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    kind TEXT NOT NULL,
                    path TEXT NOT NULL UNIQUE,
                    song TEXT NOT NULL,
                    voice TEXT NOT NULL,
                    run_id TEXT,
                    transpose INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    bytes INTEGER NOT NULL,
                    starred INTEGER NOT NULL DEFAULT 0,
                    finalized INTEGER NOT NULL DEFAULT 0
                )",
                [],
            )?;

            // 旧库迁移：outputs 追加 finalized 列（必须位于最后，读取侧按位置取值）
            let mut has_finalized = false;
            {
                let mut stmt = conn.prepare("PRAGMA table_info(outputs)")?;
                let mut rows = stmt.query([])?;
                while let Some(row) = rows.next()? {
                    let name: String = row.get(1)?;
                    if name == "finalized" {
                        has_finalized = true;
                        break;
                    }
                }
            }
            if !has_finalized {
                conn.execute(
                    "ALTER TABLE outputs ADD COLUMN finalized INTEGER NOT NULL DEFAULT 0",
                    [],
                )?;
            }

            conn.execute(
                "CREATE TABLE IF NOT EXISTS app_settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )?;

            Ok(())
        })
        .await
        .context("Failed to create database tables")?;

        Ok(Self { conn })
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_string();
        
        self.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT value FROM app_settings WHERE key = ?1"
            )?;
            
            match stmt.query_row([&key], |row| row.get(0)) {
                Ok(value) => Ok(Some(value)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await
        .context("Failed to get setting")
    }

    pub async fn set_setting(&self, key: String, value: String) -> Result<()> {
        self.conn.call(move |conn| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            
            conn.execute(
                "INSERT OR REPLACE INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![key, value, now],
            )?;
            Ok(())
        })
        .await
        .context("Failed to set setting")
    }

    pub async fn get_all_configs(&self) -> Result<Vec<TrainConfig>> {
        self.conn.call(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM configs ORDER BY exp_dir")?;
            let rows = stmt.query_map([], |row| {
                Ok(TrainConfig {
                    exp_dir: row.get(0)?,
                    sr: row.get(1)?,
                    trainset_dir: row.get(2)?,
                    np_process: row.get(3)?,
                    f0_method: row.get(4)?,
                    if_f0: row.get::<_, i32>(5)? != 0,
                    save_epoch: row.get(6)?,
                    total_epoch: row.get(7)?,
                    batch_size: row.get(8)?,
                    if_save_latest: row.get::<_, i32>(9)? != 0,
                    if_save_every_weights: row.get::<_, i32>(10)? != 0,
                    gpus: row.get(11)?,
                })
            })?;

            let mut configs = Vec::new();
            for row in rows {
                configs.push(row?);
            }
            Ok(configs)
        })
        .await
        .context("Failed to get all configs")
    }

    pub async fn get_config(&self, exp_dir: &str) -> Result<Option<TrainConfig>> {
        let exp_dir = exp_dir.to_string();
        
        self.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM configs WHERE exp_dir = ?1"
            )?;
            
            match stmt.query_row([&exp_dir], |row| {
                Ok(TrainConfig {
                    exp_dir: row.get(0)?,
                    sr: row.get(1)?,
                    trainset_dir: row.get(2)?,
                    np_process: row.get(3)?,
                    f0_method: row.get(4)?,
                    if_f0: row.get::<_, i32>(5)? != 0,
                    save_epoch: row.get(6)?,
                    total_epoch: row.get(7)?,
                    batch_size: row.get(8)?,
                    if_save_latest: row.get::<_, i32>(9)? != 0,
                    if_save_every_weights: row.get::<_, i32>(10)? != 0,
                    gpus: row.get(11)?,
                })
            }) {
                Ok(config) => Ok(Some(config)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await
        .context("Failed to get config")
    }

    pub async fn save_config(&self, config: TrainConfig) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO configs (
                    exp_dir, sr, trainset_dir, np_process, f0_method, if_f0,
                    save_epoch, total_epoch, batch_size, if_save_latest, if_save_every_weights,
                    gpus
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(exp_dir) DO UPDATE SET
                    sr = excluded.sr,
                    trainset_dir = excluded.trainset_dir,
                    np_process = excluded.np_process,
                    f0_method = excluded.f0_method,
                    if_f0 = excluded.if_f0,
                    save_epoch = excluded.save_epoch,
                    total_epoch = excluded.total_epoch,
                    batch_size = excluded.batch_size,
                    if_save_latest = excluded.if_save_latest,
                    if_save_every_weights = excluded.if_save_every_weights,
                    gpus = excluded.gpus",
                rusqlite::params![
                    config.exp_dir,
                    config.sr,
                    config.trainset_dir,
                    config.np_process,
                    config.f0_method,
                    config.if_f0 as i32,
                    config.save_epoch,
                    config.total_epoch,
                    config.batch_size,
                    config.if_save_latest as i32,
                    config.if_save_every_weights as i32,
                    config.gpus,
                ],
            )?;
            Ok(())
        })
        .await
        .context("Failed to save config")
    }

    pub async fn get_all_history(&self) -> Result<Vec<TrainHistory>> {
        self.conn.call(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM history ORDER BY start_time DESC")?;
            let rows = stmt.query_map([], |row| {
                Ok(TrainHistory {
                    id: row.get(0)?,
                    exp_dir: row.get(1)?,
                    version: row.get(2)?,
                    sr: row.get(3)?,
                    f0_method: row.get(4)?,
                    total_epoch: row.get(5)?,
                    completed_epoch: row.get(6)?,
                    status: row.get(7)?,
                    start_time: row.get(8)?,
                    end_time: row.get(9)?,
                    config: row.get(10)?,
                    index_path: row.get(11)?,
                })
            })?;

            let mut history = Vec::new();
            for row in rows {
                history.push(row?);
            }
            Ok(history)
        })
        .await
        .context("Failed to get all history")
    }

    pub async fn add_history(&self, record: TrainHistory) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute(
                "INSERT INTO history VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    record.id,
                    record.exp_dir,
                    record.version,
                    record.sr,
                    record.f0_method,
                    record.total_epoch,
                    record.completed_epoch,
                    record.status,
                    record.start_time,
                    record.end_time,
                    record.config,
                    record.index_path,
                ],
            )?;
            Ok(())
        })
        .await
        .context("Failed to add history")
    }

    pub async fn update_history(&self, id: String, completed_epoch: Option<i32>, status: Option<String>, end_time: Option<i64>, index_path: Option<String>) -> Result<()> {
        self.conn.call(move |conn| {
            if let Some(epoch) = completed_epoch {
                conn.execute(
                    "UPDATE history SET completed_epoch = ?1 WHERE id = ?2",
                    rusqlite::params![epoch, &id],
                )?;
            }
            if let Some(s) = status {
                conn.execute(
                    "UPDATE history SET status = ?1 WHERE id = ?2",
                    rusqlite::params![s, &id],
                )?;
            }
            if let Some(t) = end_time {
                conn.execute(
                    "UPDATE history SET end_time = ?1 WHERE id = ?2",
                    rusqlite::params![t, &id],
                )?;
            }
            if let Some(p) = index_path {
                conn.execute(
                    "UPDATE history SET index_path = ?1 WHERE id = ?2",
                    rusqlite::params![p, &id],
                )?;
            }
            Ok(())
        })
        .await
        .context("Failed to update history")
    }

    pub async fn delete_history(&self, id: String) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute("DELETE FROM history WHERE id = ?1", rusqlite::params![id])?;
            Ok(())
        })
        .await
        .context("Failed to delete history")
    }

    pub async fn delete_all_history(&self) -> Result<()> {
        self.conn.call(|conn| {
            conn.execute("DELETE FROM history", [])?;
            Ok(())
        })
        .await
        .context("Failed to delete all history")
    }

    // ───────────────────────── runs ─────────────────────────

    pub async fn upsert_run(&self, record: RunRecord) -> Result<()> {
        self.conn.call(move |conn| {
            // reclaimed_at 只能由 mark_run_reclaimed 写入，重复登记批次时不得被覆盖回 NULL
            conn.execute(
                "INSERT INTO runs (
                    run_id, voice, sr, if_f0, created_at, finished_at, epochs,
                    dataset_fingerprint, pipeline_version, stages_json, assets_json,
                    bytes_total, reclaimed_at, note
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(run_id) DO UPDATE SET
                    voice = excluded.voice,
                    sr = excluded.sr,
                    if_f0 = excluded.if_f0,
                    created_at = excluded.created_at,
                    finished_at = excluded.finished_at,
                    epochs = excluded.epochs,
                    dataset_fingerprint = excluded.dataset_fingerprint,
                    pipeline_version = excluded.pipeline_version,
                    stages_json = excluded.stages_json,
                    assets_json = excluded.assets_json,
                    bytes_total = excluded.bytes_total,
                    note = excluded.note",
                rusqlite::params![
                    record.run_id,
                    record.voice,
                    record.sr,
                    record.if_f0 as i32,
                    record.created_at,
                    record.finished_at,
                    record.epochs,
                    record.dataset_fingerprint,
                    record.pipeline_version,
                    record.stages_json,
                    record.assets_json,
                    record.bytes_total,
                    record.reclaimed_at,
                    record.note,
                ],
            )?;
            Ok(())
        })
        .await
        .context("Failed to upsert run")
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let run_id = run_id.to_string();

        self.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT run_id, voice, sr, if_f0, created_at, finished_at, epochs,
                        dataset_fingerprint, pipeline_version, stages_json, assets_json,
                        bytes_total, reclaimed_at, note
                 FROM runs WHERE run_id = ?1",
            )?;

            match stmt.query_row([&run_id], row_to_run) {
                Ok(record) => Ok(Some(record)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await
        .context("Failed to get run")
    }

    /// 按创建时间倒序列出批次；`voice` 为 None 表示全部声音。
    pub async fn list_runs(&self, voice: Option<String>) -> Result<Vec<RunRecord>> {
        self.conn.call(move |conn| {
            let sql = if voice.is_some() {
                "SELECT run_id, voice, sr, if_f0, created_at, finished_at, epochs,
                        dataset_fingerprint, pipeline_version, stages_json, assets_json,
                        bytes_total, reclaimed_at, note
                 FROM runs WHERE voice = ?1 ORDER BY created_at DESC"
            } else {
                "SELECT run_id, voice, sr, if_f0, created_at, finished_at, epochs,
                        dataset_fingerprint, pipeline_version, stages_json, assets_json,
                        bytes_total, reclaimed_at, note
                 FROM runs ORDER BY created_at DESC"
            };

            let mut stmt = conn.prepare(sql)?;
            let mut records = Vec::new();
            if let Some(voice) = voice {
                let rows = stmt.query_map([voice], row_to_run)?;
                for row in rows {
                    records.push(row?);
                }
            } else {
                let rows = stmt.query_map([], row_to_run)?;
                for row in rows {
                    records.push(row?);
                }
            }
            Ok(records)
        })
        .await
        .context("Failed to list runs")
    }

    pub async fn mark_run_reclaimed(&self, run_id: String, at: i64) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute(
                "UPDATE runs SET reclaimed_at = ?1 WHERE run_id = ?2",
                rusqlite::params![at, run_id],
            )?;
            Ok(())
        })
        .await
        .context("Failed to mark run reclaimed")
    }

    pub async fn delete_run(&self, run_id: String) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute("DELETE FROM runs WHERE run_id = ?1", rusqlite::params![run_id])?;
            Ok(())
        })
        .await
        .context("Failed to delete run")
    }

    // ──────────────────────── outputs ────────────────────────

    /// 以 path 为唯一键登记成品；重复登记时刷新元数据但保留 starred 状态。
    /// 返回该成品的 id。
    pub async fn upsert_output(&self, record: OutputRecord) -> Result<i64> {
        self.conn.call(move |conn| {
            let id: i64 = conn.query_row(
                "INSERT INTO outputs (kind, path, song, voice, run_id, transpose, created_at, bytes, finalized)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(path) DO UPDATE SET
                     kind = excluded.kind,
                     song = excluded.song,
                     voice = excluded.voice,
                     run_id = excluded.run_id,
                     transpose = excluded.transpose,
                     created_at = excluded.created_at,
                     bytes = excluded.bytes
                 RETURNING id",
                rusqlite::params![
                    record.kind,
                    record.path,
                    record.song,
                    record.voice,
                    record.run_id,
                    record.transpose,
                    record.created_at,
                    record.bytes,
                    record.finalized as i32,
                ],
                |row| row.get(0),
            )?;
            Ok(id)
        })
        .await
        .context("Failed to upsert output")
    }

    /// 按创建时间倒序列出成品；`kind` 为 None 表示全部类型。
    pub async fn list_outputs(&self, kind: Option<String>) -> Result<Vec<OutputRecord>> {
        self.conn.call(move |conn| {
            let sql = if kind.is_some() {
                "SELECT id, kind, path, song, voice, run_id, transpose, created_at, bytes, starred, finalized
                 FROM outputs WHERE kind = ?1 ORDER BY created_at DESC"
            } else {
                "SELECT id, kind, path, song, voice, run_id, transpose, created_at, bytes, starred, finalized
                 FROM outputs ORDER BY created_at DESC"
            };

            let mut stmt = conn.prepare(sql)?;
            let mut records = Vec::new();
            if let Some(kind) = kind {
                let rows = stmt.query_map([kind], row_to_output)?;
                for row in rows {
                    records.push(row?);
                }
            } else {
                let rows = stmt.query_map([], row_to_output)?;
                for row in rows {
                    records.push(row?);
                }
            }
            Ok(records)
        })
        .await
        .context("Failed to list outputs")
    }

    pub async fn get_output(&self, id: i64) -> Result<Option<OutputRecord>> {
        self.conn.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, kind, path, song, voice, run_id, transpose, created_at, bytes, starred, finalized
                 FROM outputs WHERE id = ?1",
            )?;
            match stmt.query_row([id], row_to_output) {
                Ok(record) => Ok(Some(record)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(tokio_rusqlite::Error::Rusqlite(e)),
            }
        })
        .await
        .context("Failed to get output")
    }

    pub async fn set_output_starred(&self, id: i64, starred: bool) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute(
                "UPDATE outputs SET starred = ?1 WHERE id = ?2",
                rusqlite::params![starred as i32, id],
            )?;
            Ok(())
        })
        .await
        .context("Failed to update output starred")
    }

    pub async fn delete_output(&self, id: i64) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute("DELETE FROM outputs WHERE id = ?1", rusqlite::params![id])?;
            Ok(())
        })
        .await
        .context("Failed to delete output")
    }

    pub async fn delete_output_by_path(&self, path: String) -> Result<()> {
        self.conn.call(move |conn| {
            conn.execute("DELETE FROM outputs WHERE path = ?1", rusqlite::params![path])?;
            Ok(())
        })
        .await
        .context("Failed to delete output by path")
    }
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        run_id: row.get(0)?,
        voice: row.get(1)?,
        sr: row.get(2)?,
        if_f0: row.get::<_, i32>(3)? != 0,
        created_at: row.get(4)?,
        finished_at: row.get(5)?,
        epochs: row.get(6)?,
        dataset_fingerprint: row.get(7)?,
        pipeline_version: row.get(8)?,
        stages_json: row.get(9)?,
        assets_json: row.get(10)?,
        bytes_total: row.get(11)?,
        reclaimed_at: row.get(12)?,
        note: row.get(13)?,
    })
}

fn row_to_output(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutputRecord> {
    Ok(OutputRecord {
        id: row.get(0)?,
        kind: row.get(1)?,
        path: row.get(2)?,
        song: row.get(3)?,
        voice: row.get(4)?,
        run_id: row.get(5)?,
        transpose: row.get(6)?,
        created_at: row.get(7)?,
        bytes: row.get(8)?,
        starred: row.get::<_, i32>(9)? != 0,
        finalized: row.get::<_, i32>(10)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_db_path(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "voxril-db-{}-{}-{}.sqlite",
            tag,
            std::process::id(),
            nanos
        ))
    }

    fn remove_db(path: &Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    }

    fn sample_run(run_id: &str) -> RunRecord {
        RunRecord {
            run_id: run_id.to_string(),
            voice: "harrison".to_string(),
            sr: "40000".to_string(),
            if_f0: true,
            created_at: 1_700_000_000,
            finished_at: Some(1_700_000_500),
            epochs: 40,
            dataset_fingerprint: "abcd1234abcd1234".to_string(),
            pipeline_version: "1234abcd1234abcd".to_string(),
            stages_json: "{}".to_string(),
            assets_json: "[]".to_string(),
            bytes_total: 1024,
            reclaimed_at: None,
            note: String::new(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn migrates_legacy_configs_table() {
        let path = temp_db_path("legacy");

        // 构造旧版 configs 表（带已废弃的 version / run_id 两列）
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE configs (
                    exp_dir TEXT PRIMARY KEY, version TEXT NOT NULL, sr TEXT NOT NULL,
                    trainset_dir TEXT NOT NULL, np_process INTEGER NOT NULL,
                    f0_method TEXT NOT NULL, if_f0 INTEGER NOT NULL, save_epoch INTEGER NOT NULL,
                    total_epoch INTEGER NOT NULL, batch_size INTEGER NOT NULL,
                    if_save_latest INTEGER NOT NULL, if_save_every_weights INTEGER NOT NULL,
                    gpus TEXT NOT NULL, run_id TEXT
                );
                INSERT INTO configs VALUES
                    ('harrison/v2_40k','v2','40000','trainset',4,'rmvpe',1,10,20,8,1,1,'0','261122-0156');",
            )
            .unwrap();
        }

        let db = Database::new(&path).await.unwrap();

        // 废弃列被丢弃，剩余字段按位置正确读出
        let cfg = db.get_config("harrison/v2_40k").await.unwrap().unwrap();
        assert!(cfg.if_f0);
        assert_eq!(cfg.sr, "40000");
        assert_eq!(cfg.total_epoch, 20);
        assert_eq!(cfg.gpus, "0");

        // 保存后仍可读回
        let mut updated = cfg.clone();
        updated.total_epoch = 30;
        db.save_config(updated).await.unwrap();
        let cfg = db.get_config("harrison/v2_40k").await.unwrap().unwrap();
        assert_eq!(cfg.total_epoch, 30);

        remove_db(&path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_index_roundtrip_keeps_reclaimed_at() {
        let path = temp_db_path("runs");
        let db = Database::new(&path).await.unwrap();

        let mut other = sample_run("261122-0100");
        other.voice = "shine".to_string();
        db.upsert_run(sample_run("261122-0156")).await.unwrap();
        db.upsert_run(other).await.unwrap();

        let all = db.list_runs(None).await.unwrap();
        assert_eq!(all.len(), 2);

        let by_voice = db.list_runs(Some("harrison".to_string())).await.unwrap();
        assert_eq!(by_voice.len(), 1);
        assert_eq!(by_voice[0].run_id, "261122-0156");

        db.mark_run_reclaimed("261122-0156".to_string(), 1_700_009_999)
            .await
            .unwrap();

        // 重复登记不得把 reclaimed_at 覆盖回 NULL
        db.upsert_run(sample_run("261122-0156")).await.unwrap();
        let run = db.get_run("261122-0156").await.unwrap().unwrap();
        assert_eq!(run.reclaimed_at, Some(1_700_009_999));

        db.delete_run("261122-0156".to_string()).await.unwrap();
        assert!(db.get_run("261122-0156").await.unwrap().is_none());

        remove_db(&path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn output_upsert_is_idempotent_and_keeps_star() {
        let path = temp_db_path("outputs");
        let db = Database::new(&path).await.unwrap();

        let record = OutputRecord {
            id: 0,
            kind: "converted".to_string(),
            path: "/tmp/out/vocal_v1.wav".to_string(),
            song: "枪火".to_string(),
            voice: "harrison".to_string(),
            run_id: Some("261122-0156".to_string()),
            transpose: 3,
            created_at: 1_700_000_000,
            bytes: 2048,
            starred: false,
            finalized: false,
        };

        let id = db.upsert_output(record.clone()).await.unwrap();
        assert!(id > 0);
        // 同 path 再登记一次：id 不变、starred 保留
        db.set_output_starred(id, true).await.unwrap();
        let mut again = record.clone();
        again.bytes = 4096;
        again.starred = false;
        let id2 = db.upsert_output(again).await.unwrap();
        assert_eq!(id, id2);

        let outputs = db.list_outputs(Some("converted".to_string())).await.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].bytes, 4096);
        assert!(outputs[0].starred);

        db.delete_output_by_path("/tmp/out/vocal_v1.wav".to_string())
            .await
            .unwrap();
        assert!(db.list_outputs(None).await.unwrap().is_empty());

        remove_db(&path);
    }
}
