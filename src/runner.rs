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
    config::RuntimeConfig,
    exec::exec,
    store::{Record, Store},
    types::ExecutionId,
};

pub async fn run_executor<S: Store>(
    actions: mpsc::UnboundedSender<Action>,
    mut store: S,
    runtime_config: RuntimeConfig,
    shell: Option<(String, Vec<String>)>,
    is_suspend: Arc<Mutex<bool>>,
    mut wake_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let latest_id = store.get_latest_id()?;
    let mut counter = latest_id.map(|id| id.0 + 1).unwrap_or(0);
    loop {
        counter += 1;
        if *is_suspend.lock().await {
            wait_interval_or_wake(Duration::from_secs(1), &mut wake_rx).await;
            continue;
        }

        let id = ExecutionId(counter);
        let start_time = chrono::Local::now();
        if let Err(e) = actions.send(Action::StartExecution(id, start_time)) {
            eprintln!("Failed to send start: {:?}", e);
        }

        let result = exec(runtime_config.command.clone(), shell.clone()).await;
        let (stdout, stderr, status) = match result {
            Ok(result) => result,
            Err(e) => (vec![], e.to_string().bytes().collect(), 1),
        };

        let exit_code = status;
        let utf8_stdout = String::from_utf8_lossy(&stdout).to_string();
        let utf8_stderr = String::from_utf8_lossy(&stderr).to_string();
        let end_time = chrono::Local::now();

        let latest_id = store.get_latest_id()?;
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
        };
        store.add_record(record)?;

        if let Err(e) = actions.send(Action::FinishExecution(id, start_time, diff, exit_code)) {
            eprintln!("Failed to send result: {:?}", e);
        }

        let interval = store
            .get_runtime_config()?
            .map(|config| config.interval)
            .unwrap_or(runtime_config.interval.num_milliseconds() as u64);

        wait_interval_or_wake(Duration::from_millis(interval), &mut wake_rx).await;
    }
}

/// Sleep for `interval` unless a wake signal arrives (used for RunCommandNow).
async fn wait_interval_or_wake(interval: Duration, wake_rx: &mut mpsc::Receiver<()>) {
    tokio::select! {
        _ = sleep(interval) => {}
        Some(()) = wake_rx.recv() => {}
    }
}

pub async fn run_executor_precise<S: Store>(
    actions: mpsc::UnboundedSender<Action>,
    mut store: S,
    runtime_config: RuntimeConfig,
    shell: Option<(String, Vec<String>)>,
    is_suspend: Arc<Mutex<bool>>,
    mut wake_rx: mpsc::Receiver<()>,
) -> Result<()> {
    let latest_id = store.get_latest_id()?;
    let mut counter = latest_id.map(|id| id.0 + 1).unwrap_or(0);
    loop {
        counter += 1;
        let start_time = chrono::Local::now();
        if *is_suspend.lock().await {
            wait_interval_or_wake(Duration::from_secs(1), &mut wake_rx).await;
            continue;
        }

        let id = ExecutionId(counter);
        if let Err(e) = actions.send(Action::StartExecution(id, start_time)) {
            eprintln!("Failed to send start: {:?}", e);
        }

        let result = exec(runtime_config.command.clone(), shell.clone()).await;
        let (stdout, stderr, status) = match result {
            Ok(result) => result,
            Err(e) => (vec![], e.to_string().bytes().collect(), 1),
        };

        let exit_code = status;
        let utf8_stdout = String::from_utf8_lossy(&stdout).to_string();
        let utf8_stderr = String::from_utf8_lossy(&stderr).to_string();
        let end_time = chrono::Local::now();

        let latest_id = store.get_latest_id()?;
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
        };
        store.add_record(record)?;

        if let Err(e) = actions.send(Action::FinishExecution(id, start_time, diff, exit_code)) {
            eprintln!("Failed to send result: {:?}", e);
        }

        let elapased = chrono::Local::now().signed_duration_since(start_time);

        let interval = store
            .get_runtime_config()?
            .map(|config| config.interval)
            .unwrap_or(runtime_config.interval.num_milliseconds() as u64);

        let interval = Duration::from_millis(interval);

        if let Ok(elapsed_std) = elapased.to_std() {
            if elapsed_std < interval {
                let sleep_time = interval - elapsed_std;
                wait_interval_or_wake(sleep_time, &mut wake_rx).await;
            }
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

    use super::{count_diff, run_executor, wait_interval_or_wake};
    use crate::{action::Action, config::RuntimeConfig, store::memory::MemoryStore};

    #[tokio::test]
    async fn wait_interval_or_wake_returns_early_on_wake() {
        let (wake_tx, mut wake_rx) = mpsc::channel(1);
        wake_tx.send(()).await.unwrap();

        let start = Instant::now();
        wait_interval_or_wake(StdDuration::from_secs(60), &mut wake_rx).await;
        assert!(start.elapsed() < StdDuration::from_millis(500));
    }

    #[tokio::test]
    async fn wait_interval_or_wake_waits_for_interval_without_wake() {
        let (_wake_tx, mut wake_rx) = mpsc::channel(1);

        let start = Instant::now();
        wait_interval_or_wake(StdDuration::from_millis(50), &mut wake_rx).await;
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
        let runtime_config = RuntimeConfig {
            interval: Duration::milliseconds(30_000),
            command: vec!["true".to_string()],
        };
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
        wake_tx.send(()).await.unwrap();

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

    #[test]
    fn wake_channel_capacity_one_drops_extra_signals() {
        let (wake_tx, _wake_rx) = mpsc::channel(1);
        wake_tx.try_send(()).unwrap();
        assert!(wake_tx.try_send(()).is_err());
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
