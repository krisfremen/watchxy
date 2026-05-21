pub mod memory;
pub mod sqlite;

use color_eyre::eyre::Result;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Local};

use crate::types::ExecutionId;

pub trait Store: Clone + Send + Sync + 'static {
    fn add_record(&mut self, record: Record) -> Result<()>;
    fn get_record(&self, id: ExecutionId) -> Result<Option<Record>>;
    fn get_latest_id(&self) -> Result<Option<ExecutionId>>;
    fn get_latest_id_for_command(&self, command_index: u32) -> Result<Option<ExecutionId>>;
    fn get_records(&self) -> Result<Vec<Record>>;
    fn get_runtime_config(&self) -> Result<Option<RuntimeConfig>>;
    fn set_runtime_config(&mut self, config: RuntimeConfig) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct Record {
    pub id: ExecutionId,
    pub start_time: DateTime<Local>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub end_time: DateTime<Local>,
    pub exit_code: i32,
    pub diff: Option<(u32, u32)>,
    pub previous_id: Option<ExecutionId>,
    pub command_index: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub interval: u64,
    /// Active command as a single string (legacy field and display).
    pub command: String,
    pub commands: Vec<Vec<String>>,
    pub active_command_index: u32,
}

impl RuntimeConfig {
    pub fn from_legacy(interval: u64, command: String) -> Self {
        let commands = vec![parse_command_tokens(&command)];
        Self {
            interval,
            command: command.clone(),
            commands,
            active_command_index: 0,
        }
    }

    pub fn active_command_tokens(&self) -> &[String] {
        self.commands
            .get(self.active_command_index as usize)
            .map(|c| c.as_slice())
            .unwrap_or(&[])
    }
}

pub fn parse_command_tokens(command: &str) -> Vec<String> {
    command.split(' ').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

pub fn commands_to_json(commands: &[Vec<String>]) -> Result<String> {
    Ok(serde_json::to_string(commands)?)
}

pub fn commands_from_json(json: &str) -> Result<Vec<Vec<String>>> {
    Ok(serde_json::from_str(json)?)
}
