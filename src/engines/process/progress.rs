//! Bounded live status, independent of the captured report output budget.
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
static JSONL: AtomicBool = AtomicBool::new(false);
pub fn configure_jsonl(enabled: bool) {
    JSONL.store(enabled, Ordering::Relaxed);
}

/// Preserve the JSONL stream for lifecycle and producer diagnostics too.
pub(crate) fn diagnostic(message: std::fmt::Arguments<'_>) {
    if JSONL.load(Ordering::Relaxed) {
        let event = serde_json::json!({"event":"diagnostic", "stage":super::phase::current(), "message":message.to_string()});
        let _ = writeln!(std::io::stderr().lock(), "{event}");
    } else {
        let _ = writeln!(std::io::stderr().lock(), "{message}");
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn workload_status(stage: &str, message: &str, elapsed_ms: Option<u128>) {
    if JSONL.load(Ordering::Relaxed) {
        let event = serde_json::json!({"event":"progress", "stage":stage, "elapsed_ms":elapsed_ms, "message":message});
        let _ = writeln!(std::io::stderr().lock(), "{event}");
    } else {
        let _ = writeln!(std::io::stderr().lock(), "hardgate: {message}");
    }
}

#[derive(Clone, Default)]
pub(super) struct Latest(Arc<Mutex<Status>>);

#[derive(Default)]
struct Status {
    excerpt: String,
    pending: String,
    counts: Option<(u64, u64)>,
}

impl Latest {
    pub(super) fn observe(&self, chunk: &[u8]) {
        let Ok(mut status) = self.0.lock() else {
            return;
        };
        let text = format!("{}{}", status.pending, String::from_utf8_lossy(chunk));
        for line in text.split(['\n', '\r']) {
            if let Some(counts) = mutation_counts(line) {
                status.counts = Some(counts);
            }
        }
        status.pending = text
            .rsplit(['\n', '\r'])
            .next()
            .unwrap_or("")
            .chars()
            .rev()
            .take(4096)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Some(line) = text.split(['\n', '\r']).rev().find(|line| {
            let text = line.to_ascii_lowercase();
            text.contains("mutant") || text.contains("mutation") || text.contains("tested")
        }) {
            // Never forward terminal controls; retain only a bounded current excerpt.
            let clean = line.chars().filter(|c| !c.is_control()).take(240).collect();
            status.excerpt = clean;
        }
    }
}

pub(super) struct Progress {
    start: Instant,
    next: Duration,
    phase: String,
    timeout: Duration,
    latest: Latest,
    command: Option<Vec<String>>,
    peak_rss: Option<u64>,
}

impl Progress {
    pub(super) fn new(phase: &str, timeout: Duration, latest: Latest) -> Self {
        Self {
            start: Instant::now(),
            next: if JSONL.load(Ordering::Relaxed) || std::io::stderr().is_terminal() {
                Duration::ZERO
            } else {
                Duration::from_secs(10)
            },
            phase: super::phase::current().unwrap_or_else(|| phase.into()),
            timeout,
            latest,
            command: super::phase::current_command(),
            peak_rss: None,
        }
    }

    pub(super) fn tick(&mut self) {
        let elapsed = self.start.elapsed();
        if elapsed < self.next {
            return;
        }
        self.next = elapsed + Duration::from_secs(10);
        self.emit("progress", elapsed);
    }

    fn emit(&mut self, event_name: &str, elapsed: Duration) {
        let (latest, counts) = self
            .latest
            .0
            .lock()
            .map(|status| (status.excerpt.clone(), status.counts))
            .unwrap_or_default();
        // JSONL also remains machine-readable during check --progress jsonl.
        let memory = crate::resources::telemetry::memory();
        self.peak_rss = self.peak_rss.max(memory.rss_bytes);
        let event = serde_json::json!({"event": event_name, "stage": self.phase, "command": self.command,
            "elapsed_ms": elapsed.as_millis(), "timeout_ms": self.timeout.as_millis(),
            "remaining_ms": self.timeout.saturating_sub(elapsed).as_millis(),
            "memory": memory, "sampled_peak_rss_bytes": self.peak_rss,
            "mutants_tested": counts.map(|value| value.0), "mutants_planned_for_execution": counts.map(|value| value.1),
            "mutation_counts_scope": counts.map(|_| "stryker-progress-reporter"),
            "mutation_progress": if latest.is_empty() { None } else { Some(latest) }});
        if JSONL.load(Ordering::Relaxed) {
            let _ = writeln!(std::io::stderr().lock(), "{event}");
        } else {
            let _ = writeln!(
                std::io::stderr().lock(),
                "hardgate: {event_name} phase={} command={} elapsed={}s timeout={}s remaining={}s rss={} sampled-peak={}{}",
                self.phase,
                serde_json::to_string(&self.command).unwrap_or_default(),
                elapsed.as_secs(),
                self.timeout.as_secs(),
                self.timeout.saturating_sub(elapsed).as_secs(),
                memory_label(memory.rss_bytes),
                memory_label(self.peak_rss),
                event["mutation_progress"]
                    .as_str()
                    .map_or(String::new(), |text| format!("; {text}"))
            );
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.emit("stage_end", self.start.elapsed());
    }
}

fn mutation_counts(text: &str) -> Option<(u64, u64)> {
    let before = text.split_once(" tested")?.0;
    let (completed, total) = before.split_whitespace().last()?.split_once('/')?;
    let counts = (completed.parse::<u64>().ok()?, total.parse::<u64>().ok()?);
    (counts.0 <= counts.1).then_some(counts)
}

fn memory_label(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "unavailable".into(),
        |bytes| format!("{} MiB", bytes / (1024 * 1024)),
    )
}

#[cfg(test)]
mod tests {
    use super::Latest;

    #[test]
    fn progress_excerpt_is_bounded_and_has_no_terminal_controls() {
        let latest = Latest::default();
        latest.observe(format!("\x1b[31mMutation progress: {}\r\n", "x".repeat(4096)).as_bytes());
        let text = latest.0.lock().unwrap().excerpt.clone();
        assert_eq!(text.chars().count(), 240);
        assert!(!text.chars().any(char::is_control));
        latest.observe(b"unrelated log line\n");
        assert_eq!(latest.0.lock().unwrap().excerpt, text);
        latest.observe(b"100 mutants tested\r");
        assert_eq!(latest.0.lock().unwrap().excerpt, "100 mutants tested");
    }

    #[test]
    fn progress_reassembles_counts_and_does_not_misread_paths_as_counts() {
        let latest = Latest::default();
        latest.observe(b"Mutation testing: 12/3");
        latest.observe(b"0 tested\r\n");
        assert_eq!(latest.0.lock().unwrap().counts, Some((12, 30)));
        latest.observe(b"static mutants are slow; see log/42\n");
        assert_eq!(latest.0.lock().unwrap().counts, Some((12, 30)));
        assert_eq!(super::mutation_counts("log/42 tested"), None);
        assert_eq!(super::mutation_counts("31/30 tested"), None);
        assert_eq!(super::memory_label(None), "unavailable");
    }
}
