//! Bounded live status, independent of the captured report output budget.
use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
static JSONL: AtomicBool = AtomicBool::new(false);
pub fn configure_jsonl(enabled: bool) {
    JSONL.store(enabled, Ordering::Relaxed);
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
pub(super) struct Latest(Arc<Mutex<String>>);

impl Latest {
    pub(super) fn observe(&self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        if let Some(line) = text.split(['\n', '\r']).rev().find(|line| {
            let text = line.to_ascii_lowercase();
            text.contains("mutant") || text.contains("mutation") || text.contains("tested")
        }) {
            // Never forward terminal controls; retain only a bounded current excerpt.
            let clean = line.chars().filter(|c| !c.is_control()).take(240).collect();
            if let Ok(mut latest) = self.0.lock() {
                *latest = clean;
            }
        }
    }
}

pub(super) struct Progress {
    start: Instant,
    next: Duration,
    phase: String,
    timeout: Duration,
    latest: Latest,
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
        }
    }

    pub(super) fn tick(&mut self) {
        let elapsed = self.start.elapsed();
        if elapsed < self.next {
            return;
        }
        self.next = elapsed + Duration::from_secs(10);
        let latest = self
            .latest
            .0
            .lock()
            .map(|text| text.clone())
            .unwrap_or_default();
        // JSONL also remains machine-readable during check --progress jsonl.
        let event = serde_json::json!({"event": "progress", "stage": self.phase, "elapsed_ms": elapsed.as_millis(), "timeout_ms": self.timeout.as_millis(), "mutation_progress": if latest.is_empty() { None } else { Some(latest) }});
        if JSONL.load(Ordering::Relaxed) {
            let _ = writeln!(std::io::stderr().lock(), "{event}");
        } else {
            let _ = writeln!(
                std::io::stderr().lock(),
                "hardgate: phase={} elapsed={}s timeout={}s{}",
                self.phase,
                elapsed.as_secs(),
                self.timeout.as_secs(),
                event["mutation_progress"]
                    .as_str()
                    .map_or(String::new(), |text| format!("; {text}"))
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Latest;

    #[test]
    fn progress_excerpt_is_bounded_and_has_no_terminal_controls() {
        let latest = Latest::default();
        latest.observe(format!("\x1b[31mMutation progress: {}\r\n", "x".repeat(4096)).as_bytes());
        let text = latest.0.lock().unwrap().clone();
        assert_eq!(text.chars().count(), 240);
        assert!(!text.chars().any(char::is_control));
        latest.observe(b"unrelated log line\n");
        assert_eq!(*latest.0.lock().unwrap(), text);
        latest.observe(b"100 mutants tested\r");
        assert_eq!(*latest.0.lock().unwrap(), "100 mutants tested");
    }
}
