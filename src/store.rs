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

    pub fn active_command_tokens(&self) -> Vec<String> {
        crate::command_state::resolved_active_command_tokens(self)
    }
}

pub fn parse_command_tokens(command: &str) -> Vec<String> {
    command
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn load_commands_from_file(
    path: &std::path::Path,
) -> color_eyre::eyre::Result<Vec<Vec<String>>> {
    use color_eyre::eyre::{bail, eyre};

    let content = std::fs::read_to_string(path)
        .map_err(|e| eyre!("Failed to read commands file {}: {e}", path.display()))?;
    let mut commands = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        commands.push(parse_command_tokens(line));
    }
    if commands.is_empty() {
        bail!("Commands file {} contains no commands", path.display());
    }
    Ok(commands)
}

pub fn commands_to_json(commands: &[Vec<String>]) -> Result<String> {
    Ok(serde_json::to_string(commands)?)
}

pub fn commands_from_json(json: &str) -> Result<Vec<Vec<String>>> {
    Ok(serde_json::from_str(json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_commands_from_file_parses_lines_like_dash_c() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cmds");
        std::fs::write(&path, "git status\n\n# comment\ndf -h\n  uptime  \n").unwrap();
        assert_eq!(
            load_commands_from_file(&path).unwrap(),
            vec![vec!["git", "status"], vec!["df", "-h"], vec!["uptime"],]
        );
    }

    #[test]
    fn load_commands_from_file_errors_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, "# only comments\n\n").unwrap();
        assert!(load_commands_from_file(&path).is_err());
    }
}
