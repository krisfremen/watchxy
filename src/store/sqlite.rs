use chrono::{DateTime, Local, Utc};
use color_eyre::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::store::{
    commands_from_json, commands_to_json, parse_command_tokens, Record, RuntimeConfig, Store,
};
use crate::types::ExecutionId;
use crate::widget::history_item::HistoryItem;

#[derive(Debug, Clone)]
pub struct SQLiteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SQLiteStore {
    pub fn new(path: PathBuf, init: bool) -> Result<Self> {
        if init && path.exists() {
            std::fs::remove_file(&path)?;
        }

        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
        )?;

        if init {
            conn.execute(
                "CREATE TABLE record (
                id INTEGER PRIMARY KEY,
                start_time TEXT NOT NULL,
                stdout BLOB NOT NULL,
                stderr BLOB NOT NULL,
                end_time TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                diff_add INTEGER,
                diff_delete INTEGER,
                previous_id INTEGER,
                command_index INTEGER NOT NULL DEFAULT 0
            )",
                (),
            )?;

            conn.execute(
                "CREATE TABLE runtime_config (
                interval INTEGER NOT NULL,
                command TEXT NOT NULL,
                commands_json TEXT,
                active_command_index INTEGER NOT NULL DEFAULT 0
            )",
                (),
            )?;
        } else {
            migrate_schema(&conn)?;
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

fn migrate_schema(conn: &Connection) -> Result<()> {
    let record_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(record)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !record_columns.iter().any(|c| c == "command_index") {
        conn.execute(
            "ALTER TABLE record ADD COLUMN command_index INTEGER NOT NULL DEFAULT 0",
            (),
        )?;
    }

    let config_columns: Vec<String> = conn
        .prepare("PRAGMA table_info(runtime_config)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !config_columns.iter().any(|c| c == "commands_json") {
        conn.execute("ALTER TABLE runtime_config ADD COLUMN commands_json TEXT", ())?;
    }
    if !config_columns.iter().any(|c| c == "active_command_index") {
        conn.execute(
            "ALTER TABLE runtime_config ADD COLUMN active_command_index INTEGER NOT NULL DEFAULT 0",
            (),
        )?;
    }
    Ok(())
}

impl Store for SQLiteStore {
    fn add_record(&mut self, record: Record) -> Result<()> {
        if let Ok(conn) = self.conn.lock() {
            conn.execute(
                "INSERT INTO record (
                id, start_time, stdout, stderr, end_time, exit_code, diff_add, diff_delete, previous_id, command_index
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                (
                    record.id,
                    record.start_time.to_utc().to_rfc3339(),
                    record.stdout,
                    record.stderr,
                    record.end_time.to_utc().to_rfc3339(),
                    record.exit_code,
                    record.diff.map(|(add, delete)| add as i64),
                    record.diff.map(|(add, delete)| delete as i64),
                    record.previous_id,
                    record.command_index,
                ),
            )?;
            Ok(())
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn get_record(&self, id: ExecutionId) -> Result<Option<Record>> {
        if let Ok(conn) = self.conn.lock() {
            let r = conn.query_row("SELECT * FROM record WHERE id = ?1", [id], |row| {
                let start_time = row.get::<_, DateTime<Utc>>(1)?;
                let end_time = row.get::<_, DateTime<Utc>>(4)?;
                let diff_add: Option<u32> = row.get(6)?;
                let diff_delete: Option<u32> = row.get(7)?;
                let diff = diff_add.zip(diff_delete);
                Ok(Record {
                    id: row.get(0)?,
                    start_time: start_time.with_timezone(&Local),
                    stdout: row.get(2)?,
                    stderr: row.get(3)?,
                    end_time: end_time.with_timezone(&Local),
                    exit_code: row.get(5)?,
                    diff,
                    previous_id: row.get(8)?,
                    command_index: row.get::<_, u32>(9).unwrap_or(0),
                })
            });

            match r {
                Ok(record) => Ok(Some(record)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn get_latest_id(&self) -> Result<Option<ExecutionId>> {
        if let Ok(conn) = self.conn.lock() {
            let r = conn.query_row(
                "SELECT id FROM record ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            );

            match r {
                Ok(id) => Ok(Some(id)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn get_latest_id_for_command(&self, command_index: u32) -> Result<Option<ExecutionId>> {
        if let Ok(conn) = self.conn.lock() {
            let r = conn.query_row(
                "SELECT id FROM record WHERE command_index = ?1 ORDER BY id DESC LIMIT 1",
                [command_index],
                |row| row.get(0),
            );

            match r {
                Ok(id) => Ok(Some(id)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn get_records(&self) -> Result<Vec<Record>> {
        if let Ok(conn) = self.conn.lock() {
            let mut stmt = conn.prepare("SELECT * FROM record")?;
            let records = stmt
                .query_map([], |row| {
                    let start_time = row.get::<_, DateTime<Utc>>(1)?;
                    let end_time = row.get::<_, DateTime<Utc>>(4)?;
                    let diff_add: Option<u32> = row.get(6)?;
                    let diff_delete: Option<u32> = row.get(7)?;
                    let diff = diff_add.zip(diff_delete);
                    Ok(Record {
                        id: row.get(0)?,
                        start_time: start_time.with_timezone(&Local),
                        stdout: row.get(2)?,
                        stderr: row.get(3)?,
                        end_time: end_time.with_timezone(&Local),
                        exit_code: row.get(5)?,
                        diff,
                        previous_id: row.get(8)?,
                        command_index: row.get::<_, u32>(9).unwrap_or(0),
                    })
                })?
                .collect::<rusqlite::Result<Vec<Record>>>()?;
            Ok(records)
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn get_runtime_config(&self) -> Result<Option<RuntimeConfig>> {
        if let Ok(conn) = self.conn.lock() {
            let r = conn.query_row(
                "SELECT interval, command, commands_json, active_command_index FROM runtime_config ORDER BY ROWID DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, u64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, u32>(3).unwrap_or(0),
                    ))
                },
            );

            match r {
                Ok((interval, command, commands_json, active_command_index)) => {
                    let commands = if let Some(json) = commands_json {
                        commands_from_json(&json)?
                    } else {
                        vec![parse_command_tokens(&command)]
                    };
                    Ok(Some(RuntimeConfig {
                        interval,
                        command,
                        commands,
                        active_command_index,
                    }))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            }
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }

    fn set_runtime_config(&mut self, config: RuntimeConfig) -> Result<()> {
        if let Ok(conn) = self.conn.lock() {
            let commands_json = if config.commands.is_empty() {
                None
            } else {
                Some(commands_to_json(&config.commands)?)
            };
            conn.execute(
                "INSERT INTO runtime_config (interval, command, commands_json, active_command_index) VALUES (?1, ?2, ?3, ?4)",
                (
                    config.interval,
                    config.command,
                    commands_json,
                    config.active_command_index,
                ),
            )?;
            Ok(())
        } else {
            color_eyre::eyre::bail!("Failed to get connection")
        }
    }
}
