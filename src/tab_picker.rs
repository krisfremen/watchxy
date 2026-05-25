use crate::config::RuntimeConfig;

pub fn command_title(command: &[String]) -> String {
    command.join(" ")
}

/// Indices into `runtime_config.commands` whose titles match the query (Vimium-style substring).
pub fn filter_command_indices(runtime_config: &RuntimeConfig, query: &str) -> Vec<usize> {
    let query = query.trim();
    if query.is_empty() {
        return (0..runtime_config.commands.len()).collect();
    }
    let needle = query.to_lowercase();
    runtime_config
        .commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| command_title(cmd).to_lowercase().contains(&needle))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;
    use crate::config::RuntimeConfig;

    fn sample_config() -> RuntimeConfig {
        RuntimeConfig::from_commands(
            Duration::seconds(2),
            vec![
                vec!["git".into(), "status".into()],
                vec!["df".into(), "-h".into()],
                vec!["uptime".into()],
            ],
        )
    }

    #[test]
    fn empty_query_lists_all_commands() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, ""), vec![0, 1, 2]);
    }

    #[test]
    fn filters_by_substring_case_insensitive() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, "git"), vec![0]);
        assert_eq!(filter_command_indices(&config, "DF"), vec![1]);
    }

    #[test]
    fn no_match_returns_empty() {
        let config = sample_config();
        assert!(filter_command_indices(&config, "zzz").is_empty());
    }
}
