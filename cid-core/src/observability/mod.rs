/*!
 * Observability (Phase 6): Prometheus-style metrics and a local, Sentry-style
 * crash log — both self-hosted, matching Part 15's local-first architecture.
 * CID doesn't depend on an external SaaS account to be observable.
 *
 * The crash log's core guarantee, tested below: a captured report can only ever
 * contain the panic message (secret-redacted) and its source location
 * (`file:line:col`) — never file *contents*. The reporter never opens a file;
 * structurally, `CrashReport` has no field that could hold one.
 */

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Metrics {
    counters: RwLock<HashMap<&'static str, AtomicU64>>,
    labeled_counters: RwLock<HashMap<(&'static str, String), AtomicU64>>,
    gauges: RwLock<HashMap<&'static str, AtomicU64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc_counter(&self, name: &'static str) {
        let map = self.counters.read().unwrap();
        if let Some(c) = map.get(name) {
            c.fetch_add(1, Ordering::Relaxed);
            return;
        }
        drop(map);
        let mut map = self.counters.write().unwrap();
        map.entry(name)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A counter split by a label value, e.g. rpc method name — rendered as
    /// `cid_rpc_requests_total{method="session.create"} 3`.
    pub fn inc_labeled(&self, name: &'static str, label: &str) {
        let key = (name, label.to_string());
        {
            let map = self.labeled_counters.read().unwrap();
            if let Some(c) = map.get(&key) {
                c.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        let mut map = self.labeled_counters.write().unwrap();
        map.entry(key)
            .or_insert_with(|| AtomicU64::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_gauge(&self, name: &'static str, value: u64) {
        let map = self.gauges.read().unwrap();
        if let Some(g) = map.get(name) {
            g.store(value, Ordering::Relaxed);
            return;
        }
        drop(map);
        let mut map = self.gauges.write().unwrap();
        map.entry(name)
            .or_insert_with(|| AtomicU64::new(0))
            .store(value, Ordering::Relaxed);
    }

    /// Render in Prometheus text exposition format (version 0.0.4).
    pub fn render_prometheus(&self) -> String {
        let mut out = String::new();
        for (name, c) in self.counters.read().unwrap().iter() {
            out.push_str(&format!("# TYPE {name} counter\n"));
            out.push_str(&format!("{name} {}\n", c.load(Ordering::Relaxed)));
        }
        let mut by_name: HashMap<&'static str, Vec<(String, u64)>> = HashMap::new();
        {
            let guard = self.labeled_counters.read().unwrap();
            for ((name, label), c) in guard.iter() {
                by_name
                    .entry(name)
                    .or_default()
                    .push((label.clone(), c.load(Ordering::Relaxed)));
            }
        }
        for (name, mut entries) in by_name {
            out.push_str(&format!("# TYPE {name} counter\n"));
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (label, value) in entries {
                out.push_str(&format!("{name}{{method=\"{label}\"}} {value}\n"));
            }
        }
        for (name, g) in self.gauges.read().unwrap().iter() {
            out.push_str(&format!("# TYPE {name} gauge\n"));
            out.push_str(&format!("{name} {}\n", g.load(Ordering::Relaxed)));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Crash log
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    pub id: String,
    pub timestamp: chrono::DateTime<Utc>,
    /// The panic message, after `redact::redact_secrets` — never raw file content.
    pub message: String,
    pub location: Option<String>,
    pub thread_name: String,
}

const MAX_CRASH_REPORTS: usize = 200;

pub struct CrashLog {
    reports: Mutex<Vec<CrashReport>>,
    log_path: Option<std::path::PathBuf>,
}

impl CrashLog {
    pub fn new(log_path: Option<std::path::PathBuf>) -> Self {
        Self {
            reports: Mutex::new(Vec::new()),
            log_path,
        }
    }

    pub fn record(&self, report: CrashReport) {
        if let Some(path) = &self.log_path {
            if let Ok(line) = serde_json::to_string(&report) {
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{line}");
                }
            }
        }
        let mut reports = self.reports.lock().unwrap();
        reports.push(report);
        if reports.len() > MAX_CRASH_REPORTS {
            let excess = reports.len() - MAX_CRASH_REPORTS;
            reports.drain(0..excess);
        }
    }

    pub fn list(&self) -> Vec<CrashReport> {
        self.reports.lock().unwrap().clone()
    }
}

impl Default for CrashLog {
    fn default() -> Self {
        Self::new(None)
    }
}

static GLOBAL_CRASH_LOG: OnceLock<std::sync::Arc<CrashLog>> = OnceLock::new();

/// Installs a panic hook that captures a redacted `CrashReport` into `log`
/// (kept alive for the process lifetime) in addition to Rust's default stderr
/// output. Call once at startup.
pub fn install_panic_hook(log: std::sync::Arc<CrashLog>) {
    let _ = GLOBAL_CRASH_LOG.set(log.clone());
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let message = panic_message(info);
        let location = info.location().map(|l| l.to_string());
        let thread_name = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        log.record(CrashReport {
            id: crate::api::types::new_id(),
            timestamp: Utc::now(),
            message: crate::redact::redact_secrets(&message),
            location,
            thread_name,
        });
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with non-string payload".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn metrics_render_in_prometheus_text_format() {
        let m = Metrics::new();
        m.inc_counter("cid_sessions_created_total");
        m.inc_counter("cid_sessions_created_total");
        m.inc_labeled("cid_rpc_requests_total", "session.create");
        m.set_gauge("cid_ws_connections_current", 3);

        let text = m.render_prometheus();
        assert!(text.contains("cid_sessions_created_total 2"));
        assert!(text.contains("cid_rpc_requests_total{method=\"session.create\"} 1"));
        assert!(text.contains("cid_ws_connections_current 3"));
    }

    #[test]
    fn crash_log_keeps_only_the_most_recent_reports() {
        let log = CrashLog::new(None);
        for i in 0..(MAX_CRASH_REPORTS + 10) {
            log.record(CrashReport {
                id: format!("r{i}"),
                timestamp: Utc::now(),
                message: format!("panic {i}"),
                location: None,
                thread_name: "main".into(),
            });
        }
        let reports = log.list();
        assert_eq!(reports.len(), MAX_CRASH_REPORTS);
        assert_eq!(reports.last().unwrap().message, "panic 209");
    }

    #[test]
    fn crash_report_has_no_field_that_could_hold_file_contents() {
        // Structural guarantee: the only string fields are the (redacted) panic
        // message, an optional `file:line:col` location, and a thread name —
        // there is no "source" or "context" field a future change could
        // accidentally populate with real file contents.
        let report = CrashReport {
            id: "r1".into(),
            timestamp: Utc::now(),
            message: "boom".into(),
            location: Some("src/foo.rs:10:5".into()),
            thread_name: "main".into(),
        };
        let value = serde_json::to_value(&report).unwrap();
        let keys: std::collections::BTreeSet<String> =
            value.as_object().unwrap().keys().cloned().collect();
        let allowed: std::collections::BTreeSet<String> =
            ["id", "timestamp", "message", "location", "thread_name"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(keys, allowed);
    }

    #[test]
    fn captured_panic_messages_are_secret_redacted() {
        let log = Arc::new(CrashLog::new(None));
        install_panic_hook(log.clone());

        let result = std::panic::catch_unwind(|| {
            panic!("failed with key sk-ant-api03-AAAAAAAAAAAAAAAAAAAA");
        });
        assert!(result.is_err());

        let reports = log.list();
        let last = reports.last().expect("a crash report should be recorded");
        assert!(
            !last.message.contains("AAAAAAAAAAAAAAAAAAAA"),
            "panic message was not redacted: {}",
            last.message
        );
    }
}
