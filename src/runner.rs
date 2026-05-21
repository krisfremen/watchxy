use color_eyre::Result;
use std::{sync::Arc, time::Duration};

use dissimilar::{diff, Chunk};
use tokio::{
    sync::{mpsc, Mutex},
    time::sleep,
};

use crate::{
    action::Action,
    bytes::normalize_stdout,
    command_state::{
        coalesce_pending_wake, is_command_runnable, resolved_active_command_tokens,
        resolved_all_commands, NO_COMMAND_CONFIGURED,
    },
    config::RuntimeConfig,
    exec::exec,
    store::{Record, RuntimeConfig as StoreRuntimeConfig, Store},
    types::ExecutionId,
};

pub use crate::command_state::WakeRequest;

struct ExecutorState {
    command: Vec<String>,
    command_index: u32,
    interval_ms: u64,
}

fn store_config_or_fallback<S: Store>(
    store: &S,
    fallback: &RuntimeConfig,
) -> Result<StoreRuntimeConfig> {
    if let Some(config) = store.get_runtime_config()? {
        return Ok(config);
    }
    Ok(StoreRuntimeConfig {
        interval: fallback.interval.num_milliseconds() as u64,
        command: fallback.active_command_display(),
        commands: fallback.commands.clone(),
        active_command_index: fallback.active_command_index as u32,
    })
}

fn load_executor_state<S: Store>(store: &S, fallback: &RuntimeConfig) -> Result<ExecutorState> {
    let config = store_config_or_fallback(store, fallback)?;
    Ok(ExecutorState {
        command: resolved_active_command_tokens(&config),
        command_index: config.active_command_index,
        interval_ms: config.interval,
    })
}

fn load_all_commands<S: Store>(store: &S, fallback: &RuntimeConfig) -> Result<Vec<(u32, Vec<String>)>> {
    let config = store_config_or_fallback(store, fallback)?;
    Ok(resolved_all_commands(&config))
}

async fn execute_command<S: Store>(
    actions: &mpsc::UnboundedSender<Action>,
    store: &mut S,
    counter: &mut u32,
    command_index: u32,
    command: Vec<String>,
    shell: Option<(String, Vec<String>)>,
) -> Result<()> {
    *counter += 1;
    let id = ExecutionId(*counter);
    let start_time = chrono::Local::now();
    if let Err(e) = actions.send(Action::StartExecution(id, start_time, command_index)) {
        eprintln!("Failed to send start: {:?}", e);
    }

    let (stdout, stderr, status) = if !is_command_runnable(&command) {
        (
            Vec::new(),
            NO_COMMAND_CONFIGURED.as_bytes().to_vec(),
            1,
        )
    } else {
        match exec(command, shell).await {
            Ok(result) => result,
            Err(e) => (vec![], e.to_string().bytes().collect(), 1),
        }
    };

    let exit_code = status;
    let utf8_stdout = String::from_utf8_lossy(&stdout).to_string();
    let end_time = chrono::Local::now();

    let latest_id = store.get_latest_id_for_command(command_index)?;
    let diff = if let Some(latest_id) = latest_id {
        if let Some(record) = store.get_record(latest_id)? {
            let old_stdout = String::from_utf8_lossy(&record.stdout).to_string();
            Some(count_diff(&old_stdout, &utf8_stdout))
        } else {
            None
        }
    } else {
        None
    };

    if let Some((diff_add, diff_delete)) = diff {
        if diff_add != 0 || diff_delete != 0 {
            if let Err(e) = actions.send(Action::DiffDetected) {
                eprintln!("Failed to send diff detected: {:?}", e);
            }
        }
    }

    let record = Record {
        id,
        start_time,
        stdout,
        stderr,
        end_time,
        exit_code,
        diff,
        previous_id: latest_id,
        command_index,
    };
    store.add_record(record)?;

    if let Err(e) = actions.send(Action::FinishExecution(id, start_time, diff, exit_code)) {
        eprintln!("Failed to send result: {:?}", e);
    }

    Ok(())
}

#[cfg(test)]
pub(crate) async fn execute_command_for_test<S: Store>(
    actions: &mpsc::UnboundedSender<Action>,
    store: &mut S,
    counter: &mut u32,
    command_index: u32,
    command: Vec<String>,
    shell: Option<(String, Vec<String>)>,
) -> Result<()> {
    execute_command(actions, store, counter, command_index, command, shell).await
}

pub async fn run_executor<S: Store>(
    actions: mpsc::UnboundedSender<Action>,
    mut store: S,
    runtime_config: RuntimeConfig,
    shell: Option<(String, Vec<String>)>,
    is_suspend: Arc<Mutex<bool>>,
    mut wake_rx: mpsc::Receiver<WakeRequest>,
) -> Result<()> {
    let latest_id = store.get_latest_id()?;
    let mut counter = latest_id.map(|id| id.0 + 1).unwrap_or(0);
    let mut pending_wake = None;
    let mut first_run = true;
    loop {
        if *is_suspend.lock().await {
            pending_wake = wait_interval_or_wake(Duration::from_secs(1), &mut wake_rx).await;
            continue;
        }

        let run_all = pending_wake == Some(WakeRequest::All)
            || (first_run && !load_all_commands(&store, &runtime_config)?.is_empty());
        first_run = false;

        if run_all {
            let commands = load_all_commands(&store, &runtime_config)?;
            for (command_index, command) in commands {
                execute_command(
                    &actions,
                    &mut store,
                    &mut counter,
                    command_index,
                    command,
                    shell.clone(),
                )
                .await?;
            }
        } else {
            let state = load_executor_state(&store, &runtime_config)?;
            execute_command(
                &actions,
                &mut store,
                &mut counter,
                state.command_index,
                state.command,
                shell.clone(),
            )
            .await?;
        }

        let interval_ms = load_executor_state(&store, &runtime_config)?.interval_ms;
        pending_wake = wait_interval_or_wake(Duration::from_millis(interval_ms), &mut wake_rx).await;
    }
}

/// Sleep for `interval` unless a wake signal arrives; returns the coalesced wake request if any.
pub async fn wait_interval_or_wake(
    interval: Duration,
    wake_rx: &mut mpsc::Receiver<WakeRequest>,
) -> Option<WakeRequest> {
    let first = tokio::select! {
        _ = sleep(interval) => None,
        req = wake_rx.recv() => req,
    };
    coalesce_pending_wake(first, wake_rx)
}

pub async fn run_executor_precise<S: Store>(
    actions: mpsc::UnboundedSender<Action>,
    mut store: S,
    runtime_config: RuntimeConfig,
    shell: Option<(String, Vec<String>)>,
    is_suspend: Arc<Mutex<bool>>,
    mut wake_rx: mpsc::Receiver<WakeRequest>,
) -> Result<()> {
    let latest_id = store.get_latest_id()?;
    let mut counter = latest_id.map(|id| id.0 + 1).unwrap_or(0);
    let mut pending_wake = None;
    let mut first_run = true;
    loop {
        let cycle_start = chrono::Local::now();
        if *is_suspend.lock().await {
            pending_wake = wait_interval_or_wake(Duration::from_secs(1), &mut wake_rx).await;
            continue;
        }

        let run_all = pending_wake == Some(WakeRequest::All)
            || (first_run && !load_all_commands(&store, &runtime_config)?.is_empty());
        first_run = false;

        if run_all {
            let commands = load_all_commands(&store, &runtime_config)?;
            for (command_index, command) in commands {
                execute_command(
                    &actions,
                    &mut store,
                    &mut counter,
                    command_index,
                    command,
                    shell.clone(),
                )
                .await?;
            }
        } else {
            let state = load_executor_state(&store, &runtime_config)?;
            execute_command(
                &actions,
                &mut store,
                &mut counter,
                state.command_index,
                state.command,
                shell.clone(),
            )
            .await?;
        }

        let interval_ms = load_executor_state(&store, &runtime_config)?.interval_ms;
        let interval = Duration::from_millis(interval_ms);
        let elapsed = chrono::Local::now().signed_duration_since(cycle_start);
        if let Ok(elapsed_std) = elapsed.to_std() {
            if elapsed_std < interval {
                let sleep_time = interval - elapsed_std;
                pending_wake = wait_interval_or_wake(sleep_time, &mut wake_rx).await;
            } else {
                pending_wake = None;
            }
        } else {
            pending_wake = None;
        }
    }
}

fn count_diff(old: &str, current: &str) -> (u32, u32) {
    diff(old, current)
        .iter()
        .map(|c| match c {
            Chunk::Delete(s) => (0, s.chars().count() as u32),
            Chunk::Insert(s) => (s.chars().count() as u32, 0),
            _ => (0, 0),
        })
        .reduce(|t1, t2| (t1.0 + t2.0, t1.1 + t2.1))
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use std::time::{Duration as StdDuration, Instant};

    use chrono::Duration;
    use tokio::sync::mpsc;
    use tokio::time::sleep;

    use super::{
        count_diff, execute_command_for_test, run_executor, wait_interval_or_wake, WakeRequest,
    };
    use crate::command_state::resolved_active_command_tokens;
    use crate::store::{RuntimeConfig as StoreRuntimeConfig, Store};
    use crate::types::ExecutionId;
    use crate::{action::Action, config::RuntimeConfig, store::memory::MemoryStore};

    #[tokio::test]
    async fn wait_interval_or_wake_returns_early_on_wake() {
        let (wake_tx, mut wake_rx) = mpsc::channel(1);
        wake_tx.send(WakeRequest::Active).await.unwrap();

        let start = Instant::now();
        let wake = wait_interval_or_wake(StdDuration::from_secs(60), &mut wake_rx).await;
        assert_eq!(wake, Some(WakeRequest::Active));
        assert!(start.elapsed() < StdDuration::from_millis(500));
    }

    #[tokio::test]
    async fn wait_interval_or_wake_waits_for_interval_without_wake() {
        let (_wake_tx, mut wake_rx) = mpsc::channel(1);

        let start = Instant::now();
        let wake = wait_interval_or_wake(StdDuration::from_millis(50), &mut wake_rx).await;
        assert_eq!(wake, None);
        assert!(start.elapsed() >= StdDuration::from_millis(45));
    }

    async fn drain_until<F>(
        rx: &mut mpsc::UnboundedReceiver<Action>,
        mut pred: F,
        limit: StdDuration,
    ) -> bool
    where
        F: FnMut(&Action) -> bool,
    {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            while let Ok(action) = rx.try_recv() {
                if pred(&action) {
                    return true;
                }
            }
            sleep(StdDuration::from_millis(5)).await;
        }
        false
    }

    #[tokio::test]
    async fn wake_skips_long_interval_between_command_runs() {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();
        let (wake_tx, wake_rx) = mpsc::channel(1);
        let store = MemoryStore::new();
        let runtime_config =
            RuntimeConfig::from_single_command(Duration::milliseconds(30_000), vec!["true".to_string()]);
        let is_suspend = std::sync::Arc::new(tokio::sync::Mutex::new(false));

        let handle = tokio::spawn(run_executor(
            action_tx,
            store,
            runtime_config,
            None,
            is_suspend,
            wake_rx,
        ));

        assert!(
            drain_until(
                &mut action_rx,
                |a| matches!(a, Action::FinishExecution(..)),
                StdDuration::from_secs(5),
            )
            .await,
            "expected first command run to finish"
        );

        let after_wake = Instant::now();
        wake_tx.send(WakeRequest::Active).await.unwrap();

        assert!(
            drain_until(
                &mut action_rx,
                |a| matches!(a, Action::FinishExecution(..)),
                StdDuration::from_secs(2),
            )
            .await,
            "expected second run to finish soon after wake"
        );
        assert!(
            after_wake.elapsed() < StdDuration::from_secs(2),
            "wake should skip the 30s interval (took {:?})",
            after_wake.elapsed()
        );

        handle.abort();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn coalesce_wake_prefers_all_over_active() {
        let (wake_tx, mut wake_rx) = mpsc::channel(8);
        wake_tx.send(WakeRequest::Active).await.unwrap();
        wake_tx.send(WakeRequest::All).await.unwrap();

        let wake = wait_interval_or_wake(StdDuration::from_secs(60), &mut wake_rx).await;
        assert_eq!(wake, Some(WakeRequest::All));
    }

    #[tokio::test]
    async fn executor_runs_legacy_command_when_commands_vec_empty() {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();
        let mut store = MemoryStore::new();
        store
            .set_runtime_config(StoreRuntimeConfig {
                interval: 1000,
                command: "echo watchxy-test-marker".to_string(),
                commands: vec![],
                active_command_index: 0,
            })
            .unwrap();

        let config = store.get_runtime_config().unwrap().unwrap();
        let tokens = resolved_active_command_tokens(&config);
        let mut counter = 0u32;
        execute_command_for_test(
            &action_tx,
            &mut store,
            &mut counter,
            0,
            tokens,
            None,
        )
        .await
        .unwrap();

        assert!(
            drain_until(
                &mut action_rx,
                |a| matches!(a, Action::FinishExecution(..)),
                StdDuration::from_secs(5),
            )
            .await
        );

        let record = store
            .get_record(ExecutionId(1))
            .unwrap()
            .expect("record");
        let stdout = String::from_utf8_lossy(&record.stdout);
        assert!(
            stdout.contains("watchxy-test-marker"),
            "stdout was: {stdout:?}"
        );
    }

    #[tokio::test]
    async fn executor_records_error_when_command_empty() {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();
        let mut store = MemoryStore::new();
        store
            .set_runtime_config(StoreRuntimeConfig {
                interval: 1000,
                command: String::new(),
                commands: vec![],
                active_command_index: 0,
            })
            .unwrap();

        let mut counter = 0u32;
        execute_command_for_test(&action_tx, &mut store, &mut counter, 0, vec![], None)
            .await
            .unwrap();

        assert!(
            drain_until(
                &mut action_rx,
                |a| matches!(a, Action::FinishExecution(..)),
                StdDuration::from_secs(2),
            )
            .await
        );

        let record = store
            .get_record(ExecutionId(1))
            .unwrap()
            .expect("record");
        let stderr = String::from_utf8_lossy(&record.stderr);
        assert!(stderr.contains("no command configured"));
    }

    #[tokio::test]
    async fn startup_runs_all_commands_immediately() {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();
        let (_wake_tx, wake_rx) = mpsc::channel(8);
        let mut store = MemoryStore::new();
        store
            .set_runtime_config(StoreRuntimeConfig {
                interval: 60_000,
                command: "echo one".to_string(),
                commands: vec![
                    vec!["echo".into(), "one".into()],
                    vec!["echo".into(), "two".into()],
                ],
                active_command_index: 0,
            })
            .unwrap();

        let runtime_config =
            RuntimeConfig::from_commands(Duration::milliseconds(60_000), vec![]);
        let is_suspend = std::sync::Arc::new(tokio::sync::Mutex::new(false));

        let handle = tokio::spawn(run_executor(
            action_tx,
            store,
            runtime_config,
            None,
            is_suspend,
            wake_rx,
        ));

        let mut finishes = 0;
        let deadline = Instant::now() + StdDuration::from_secs(5);
        while Instant::now() < deadline && finishes < 2 {
            while let Ok(action) = action_rx.try_recv() {
                if matches!(action, Action::FinishExecution(..)) {
                    finishes += 1;
                }
            }
            sleep(StdDuration::from_millis(10)).await;
        }
        assert_eq!(
            finishes, 2,
            "startup should run every configured command before waiting on interval"
        );

        handle.abort();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn wake_all_runs_every_configured_command() {
        let (action_tx, mut action_rx) = mpsc::unbounded_channel();
        let (wake_tx, wake_rx) = mpsc::channel(8);
        let mut store = MemoryStore::new();
        store
            .set_runtime_config(StoreRuntimeConfig {
                interval: 60_000,
                command: "echo one".to_string(),
                commands: vec![
                    vec!["echo".into(), "one".into()],
                    vec!["echo".into(), "two".into()],
                ],
                active_command_index: 0,
            })
            .unwrap();

        let runtime_config =
            RuntimeConfig::from_commands(Duration::milliseconds(60_000), vec![]);
        let is_suspend = std::sync::Arc::new(tokio::sync::Mutex::new(false));

        let handle = tokio::spawn(run_executor(
            action_tx,
            store,
            runtime_config,
            None,
            is_suspend,
            wake_rx,
        ));

        let mut startup_finishes = 0;
        let startup_deadline = Instant::now() + StdDuration::from_secs(5);
        while Instant::now() < startup_deadline && startup_finishes < 2 {
            while let Ok(action) = action_rx.try_recv() {
                if matches!(action, Action::FinishExecution(..)) {
                    startup_finishes += 1;
                }
            }
            sleep(StdDuration::from_millis(10)).await;
        }
        assert_eq!(
            startup_finishes, 2,
            "startup should run every configured command first"
        );

        sleep(StdDuration::from_millis(100)).await;
        while action_rx.try_recv().is_ok() {}

        wake_tx.send(WakeRequest::All).await.unwrap();
        sleep(StdDuration::from_millis(50)).await;

        let mut wake_finishes = 0;
        let wake_deadline = Instant::now() + StdDuration::from_secs(5);
        while Instant::now() < wake_deadline && wake_finishes < 2 {
            while let Ok(action) = action_rx.try_recv() {
                if matches!(action, Action::FinishExecution(..)) {
                    wake_finishes += 1;
                }
            }
            sleep(StdDuration::from_millis(10)).await;
        }
        assert_eq!(wake_finishes, 2, "Run All should finish both commands");

        handle.abort();
        let _ = handle.await;
    }

    #[test]
    fn test_count_diff() {
        let current = "hello world!";
        let old = "hello world";

        let result = count_diff(old, current);

        assert_eq!(result, (1, 0))
    }

    #[test]
    fn test_count_delete_diff() {
        let current = "hello world";
        let old = "hello oorld!";

        let result = count_diff(old, current);

        assert_eq!(result, (1, 2))
    }
}
