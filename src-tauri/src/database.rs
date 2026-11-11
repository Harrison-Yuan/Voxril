use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainConfig {
    pub exp_dir: String,
    pub version: String,
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
                    version TEXT NOT NULL,
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
            let mut stmt = conn.prepare("SELECT * FROM configs")?;
            let rows = stmt.query_map([], |row| {
                Ok(TrainConfig {
                    exp_dir: row.get(0)?,
                    version: row.get(1)?,
                    sr: row.get(2)?,
                    trainset_dir: row.get(3)?,
                    np_process: row.get(4)?,
                    f0_method: row.get(5)?,
                    if_f0: row.get::<_, i32>(6)? != 0,
                    save_epoch: row.get(7)?,
                    total_epoch: row.get(8)?,
                    batch_size: row.get(9)?,
                    if_save_latest: row.get::<_, i32>(10)? != 0,
                    if_save_every_weights: row.get::<_, i32>(11)? != 0,
                    gpus: row.get(12)?,
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
                    version: row.get(1)?,
                    sr: row.get(2)?,
                    trainset_dir: row.get(3)?,
                    np_process: row.get(4)?,
                    f0_method: row.get(5)?,
                    if_f0: row.get::<_, i32>(6)? != 0,
                    save_epoch: row.get(7)?,
                    total_epoch: row.get(8)?,
                    batch_size: row.get(9)?,
                    if_save_latest: row.get::<_, i32>(10)? != 0,
                    if_save_every_weights: row.get::<_, i32>(11)? != 0,
                    gpus: row.get(12)?,
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
                "INSERT OR REPLACE INTO configs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    config.exp_dir,
                    "v2".to_string(),
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
}
