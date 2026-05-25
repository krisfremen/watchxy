use crate::config::RuntimeConfig;

pub fn command_title(command: &[String]) -> String {
    command.join(" ")
}

/// Parsed filter string from the tab picker prompt (Vim-style modifiers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterQuery {
    pub needle: String,
    pub ignore_case: bool,
}

/// Default is case-sensitive. Prefix with `\c` for ignore case, `\C` for explicit case match.
pub fn parse_filter_query(query: &str) -> FilterQuery {
    let query = query.trim();
    if query.is_empty() {
        return FilterQuery {
            needle: String::new(),
            ignore_case: false,
        };
    }
    if let Some(rest) = query.strip_prefix(r"\c") {
        return FilterQuery {
            needle: rest.to_string(),
            ignore_case: true,
        };
    }
    if let Some(rest) = query.strip_prefix(r"\C") {
        return FilterQuery {
            needle: rest.to_string(),
            ignore_case: false,
        };
    }
    FilterQuery {
        needle: query.to_string(),
        ignore_case: false,
    }
}

pub fn title_matches(title: &str, parsed: &FilterQuery) -> bool {
    if parsed.needle.is_empty() {
        return true;
    }
    if parsed.ignore_case {
        title.to_lowercase().contains(&parsed.needle.to_lowercase())
    } else {
        title.contains(&parsed.needle)
    }
}

/// Byte ranges in `title` to highlight for the current filter.
pub fn match_ranges(title: &str, parsed: &FilterQuery) -> Vec<(usize, usize)> {
    if parsed.needle.is_empty() {
        return Vec::new();
    }

    let (haystack, needle) = if parsed.ignore_case {
        (title.to_lowercase(), parsed.needle.to_lowercase())
    } else {
        (title.to_string(), parsed.needle.clone())
    };

    let mut ranges = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = haystack[start..].find(&needle) {
        let match_start = start + rel;
        let match_end = match_start + needle.len();
        ranges.push((match_start, match_end));
        start = match_end;
    }
    ranges
}

/// Indices into `runtime_config.commands` whose titles match the query.
pub fn filter_command_indices(runtime_config: &RuntimeConfig, query: &str) -> Vec<usize> {
    let parsed = parse_filter_query(query);
    if parsed.needle.is_empty() {
        return (0..runtime_config.commands.len()).collect();
    }
    runtime_config
        .commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| title_matches(&command_title(cmd), &parsed))
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
    fn filters_by_substring_case_sensitive_by_default() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, "git"), vec![0]);
        assert_eq!(filter_command_indices(&config, "GIT"), Vec::<usize>::new());
        assert_eq!(filter_command_indices(&config, "DF"), Vec::<usize>::new());
    }

    #[test]
    fn backslash_c_enables_ignore_case() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, r"\cgit"), vec![0]);
        assert_eq!(filter_command_indices(&config, r"\cDF"), vec![1]);
        assert_eq!(
            filter_command_indices(&config, r"\CGIT"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn parse_filter_query_vim_modifiers() {
        assert_eq!(
            parse_filter_query(r"\cGit"),
            FilterQuery {
                needle: "Git".into(),
                ignore_case: true,
            }
        );
        assert_eq!(
            parse_filter_query(r"\Cgit"),
            FilterQuery {
                needle: "git".into(),
                ignore_case: false,
            }
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let config = sample_config();
        assert!(filter_command_indices(&config, "zzz").is_empty());
    }

    #[test]
    fn command_title_joins_tokens() {
        assert_eq!(
            command_title(&["git".into(), "status".into()]),
            "git status"
        );
    }

    #[test]
    fn parse_filter_query_trims_whitespace() {
        assert_eq!(
            parse_filter_query("  status  "),
            FilterQuery {
                needle: "status".into(),
                ignore_case: false,
            }
        );
        assert_eq!(
            parse_filter_query(r"  \cdf  "),
            FilterQuery {
                needle: "df".into(),
                ignore_case: true,
            }
        );
    }

    #[test]
    fn modifier_only_query_lists_all_commands() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, r"\c"), vec![0, 1, 2]);
        assert_eq!(filter_command_indices(&config, r"\C"), vec![0, 1, 2]);
    }

    #[test]
    fn title_matches_respects_case_mode() {
        let sensitive = parse_filter_query("git");
        let insensitive = parse_filter_query(r"\cgit");

        assert!(title_matches("git status", &sensitive));
        assert!(!title_matches("Git status", &sensitive));

        assert!(title_matches("git status", &insensitive));
        assert!(title_matches("Git status", &insensitive));
    }

    #[test]
    fn title_matches_empty_needle_matches_everything() {
        let parsed = parse_filter_query("");
        assert!(title_matches("anything", &parsed));
    }

    #[test]
    fn match_ranges_case_sensitive() {
        let parsed = parse_filter_query("status");
        assert_eq!(match_ranges("git status", &parsed), vec![(4, 10)]);
    }

    #[test]
    fn match_ranges_case_insensitive_uses_original_slice() {
        let parsed = parse_filter_query(r"\cgit");
        assert_eq!(match_ranges("Git status", &parsed), vec![(0, 3)]);
    }

    #[test]
    fn match_ranges_finds_multiple_occurrences() {
        let parsed = parse_filter_query("ab");
        assert_eq!(match_ranges("abab", &parsed), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn match_ranges_empty_needle_returns_no_ranges() {
        let parsed = parse_filter_query("");
        assert!(match_ranges("git status", &parsed).is_empty());
    }

    #[test]
    fn filter_can_match_multiple_commands() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, "t"), vec![0, 2]);
        assert_eq!(filter_command_indices(&config, r"\ct"), vec![0, 2]);
    }

    #[test]
    fn filter_matches_substring_anywhere_in_title() {
        let config = sample_config();
        assert_eq!(filter_command_indices(&config, "-h"), vec![1]);
        assert_eq!(filter_command_indices(&config, "time"), vec![2]);
    }
}
