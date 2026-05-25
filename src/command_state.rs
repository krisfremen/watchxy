//! Command resolution and wake handling shared by the executor and store.

use crate::store::{self, RuntimeConfig as StoreRuntimeConfig};

pub const NO_COMMAND_CONFIGURED: &str = "(no command configured)";
pub const COMMAND_PRODUCED_NO_OUTPUT: &str = "(command produced no output)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeRequest {
    Active,
    All,
}

pub fn is_command_runnable(command: &[String]) -> bool {
    !command.is_empty()
}

/// Active command tokens, falling back to the legacy `command` string when `commands` is empty.
pub fn resolved_active_command_tokens(config: &StoreRuntimeConfig) -> Vec<String> {
    if let Some(tokens) = config.commands.get(config.active_command_index as usize) {
        if !tokens.is_empty() {
            return tokens.clone();
        }
    }
    let legacy = store::parse_command_tokens(&config.command);
    if !legacy.is_empty() {
        return legacy;
    }
    if let Some(first) = config.commands.first() {
        if !first.is_empty() {
            return first.clone();
        }
    }
    Vec::new()
}

/// All configured commands for a run-all cycle.
pub fn resolved_all_commands(config: &StoreRuntimeConfig) -> Vec<(u32, Vec<String>)> {
    if config.commands.is_empty() {
        let tokens = resolved_active_command_tokens(config);
        if tokens.is_empty() {
            return Vec::new();
        }
        return vec![(config.active_command_index, tokens)];
    }

    config
        .commands
        .iter()
        .enumerate()
        .map(|(i, cmd)| (i as u32, cmd.clone()))
        .filter(|(_, cmd)| is_command_runnable(cmd))
        .collect()
}

pub fn merge_wake(current: Option<WakeRequest>, incoming: WakeRequest) -> WakeRequest {
    match (current, incoming) {
        (Some(WakeRequest::All), _) | (_, WakeRequest::All) => WakeRequest::All,
        _ => WakeRequest::Active,
    }
}

pub fn coalesce_pending_wake(
    initial: Option<WakeRequest>,
    rx: &mut tokio::sync::mpsc::Receiver<WakeRequest>,
) -> Option<WakeRequest> {
    let mut merged = initial;
    while let Ok(incoming) = rx.try_recv() {
        merged = Some(match merged {
            None => incoming,
            Some(current) => merge_wake(Some(current), incoming),
        });
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RuntimeConfig as StoreRuntimeConfig;

    fn config_with_commands(
        commands: Vec<Vec<String>>,
        active: u32,
        legacy: &str,
    ) -> StoreRuntimeConfig {
        StoreRuntimeConfig {
            interval: 2000,
            command: legacy.to_string(),
            commands,
            active_command_index: active,
        }
    }

    #[test]
    fn resolved_active_falls_back_to_legacy_command_string() {
        let config = config_with_commands(vec![], 0, "echo hello");
        assert_eq!(
            resolved_active_command_tokens(&config),
            vec!["echo", "hello"]
        );
    }

    #[test]
    fn resolved_active_uses_indexed_commands_when_present() {
        let config = config_with_commands(
            vec![
                vec!["git".into(), "status".into()],
                vec!["df".into(), "-h".into()],
            ],
            1,
            "git status",
        );
        assert_eq!(resolved_active_command_tokens(&config), vec!["df", "-h"]);
    }

    #[test]
    fn resolved_all_returns_every_non_empty_command() {
        let config = config_with_commands(
            vec![vec!["true".into()], vec![], vec!["false".into()]],
            0,
            "true",
        );
        assert_eq!(
            resolved_all_commands(&config),
            vec![
                (0, vec!["true".to_string()]),
                (2, vec!["false".to_string()])
            ]
        );
    }

    #[test]
    fn merge_wake_all_wins_over_active() {
        assert_eq!(
            merge_wake(Some(WakeRequest::Active), WakeRequest::All),
            WakeRequest::All
        );
        assert_eq!(
            merge_wake(Some(WakeRequest::All), WakeRequest::Active),
            WakeRequest::All
        );
        assert_eq!(merge_wake(None, WakeRequest::Active), WakeRequest::Active);
    }

    #[test]
    fn is_command_runnable_rejects_empty() {
        assert!(!is_command_runnable(&[]));
        assert!(is_command_runnable(&["echo".into()]));
    }
}
