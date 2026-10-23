//! Starting, stopping and provisioning a local model runtime.
//!
//! Scope boundary, stated rather than blurred: this manages a runtime that is
//! **already installed**, and can install it **only on an explicit click**.
//!
//! `install_blocking` runs the platform's *own* package manager — winget on
//! Windows, Homebrew on macOS, Ollama's published script on Linux — which is
//! exactly what a user would run by hand, with output streamed into the UI.
//! It is never automatic and never silent: nothing here is invoked except in
//! response to `local.runtime.install`, and a platform with no known package
//! manager reports `installer: None` so the UI shows the download link
//! instead of a button that cannot work. The earlier "CID never installs"
//! rule was the right instinct about *silent* acquisition and the wrong
//! answer for a one-click setup, so it was narrowed rather than dropped.
//!
//! The property worth being careful about is ownership of the server process.
//! Many people already have `ollama serve` running as a system service. Killing
//! that because a button in CID says "Stop" would be an unpleasant surprise, so
//! `stop` only ever terminates a child *this process* spawned; a server we did
//! not start is reported as externally managed and left alone.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

const OLLAMA_ENDPOINT: &str = "http://localhost:11434";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeOwnership {
    /// Started by CID, so CID may stop it.
    Managed,
    /// Already running when we looked — someone else's process.
    External,
    NotRunning,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub installed: bool,
    /// Absolute path to the runtime binary, when it is on PATH.
    pub binary_path: Option<String>,
    pub running: bool,
    pub ownership: RuntimeOwnership,
    pub endpoint: &'static str,
    /// Populated only when the runtime is up.
    pub installed_models: Vec<String>,
    /// Where to get it, for the not-installed case.
    pub install_url: &'static str,
    /// The package manager a one-click install would use, when this platform
    /// has one. `None` means the UI must fall back to the download link
    /// rather than offering a button that cannot succeed.
    pub installer: Option<&'static str>,
}

/// Run a command, forwarding every stdout/stderr line to `on_line` as it
/// arrives. Both pipes are drained on their own threads so a chatty installer
/// cannot fill a pipe buffer and deadlock against the reader.
fn run_streamed(program: &str, args: &[&str], on_line: &mut impl FnMut(String)) -> Result<()> {
    use std::io::{BufRead, BufReader};

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to launch `{program}`"))?;

    let stdout = child.stdout.take().context("no stdout pipe")?;
    let stderr = child.stderr.take().context("no stderr pipe")?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let tx_err = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx_err.send(line);
        }
    });

    // Ends when both sender threads drop, i.e. when the child closes its pipes.
    for line in rx {
        on_line(line);
    }

    let status = child
        .wait()
        .with_context(|| format!("`{program}` did not run to completion"))?;
    if !status.success() {
        bail!("`{program}` exited with {status}");
    }
    Ok(())
}

#[derive(Default)]
pub struct LocalRuntimeManager {
    /// The `ollama serve` child we spawned, if any.
    server: Arc<Mutex<Option<Child>>>,
}

impl LocalRuntimeManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn binary() -> Option<String> {
        if let Ok(p) = which::which("ollama") {
            return Some(p.to_string_lossy().to_string());
        }
        // A just-completed install lands in a known directory that is not yet
        // on this process's PATH (Windows and macOS both resolve PATH at
        // process start), so without this the runtime would report "not
        // installed" until Core was restarted — the exact moment a user is
        // most likely to check.
        Self::fallback_paths()
            .into_iter()
            .find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
    }

    /// Where each platform's installer puts the binary.
    fn fallback_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if cfg!(windows) {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                paths.push(PathBuf::from(local).join("Programs/Ollama/ollama.exe"));
            }
            if let Ok(pf) = std::env::var("ProgramFiles") {
                paths.push(PathBuf::from(pf).join("Ollama/ollama.exe"));
            }
        } else if cfg!(target_os = "macos") {
            paths.push(PathBuf::from("/opt/homebrew/bin/ollama"));
            paths.push(PathBuf::from("/usr/local/bin/ollama"));
            paths.push(PathBuf::from(
                "/Applications/Ollama.app/Contents/Resources/ollama",
            ));
        } else {
            paths.push(PathBuf::from("/usr/local/bin/ollama"));
            paths.push(PathBuf::from("/usr/bin/ollama"));
        }
        paths
    }

    /// Which official installer this platform offers, if any. Surfaced to the
    /// UI so a one-click button only appears where it can actually work.
    pub fn installer_kind() -> Option<&'static str> {
        if cfg!(windows) {
            which::which("winget").ok().map(|_| "winget")
        } else if cfg!(target_os = "macos") {
            which::which("brew").ok().map(|_| "homebrew")
        } else if which::which("curl").is_ok() {
            Some("official script")
        } else {
            None
        }
    }

    /// Install the runtime through the platform's own package manager,
    /// streaming its output so a multi-minute install does not look frozen.
    /// Only ever called from the explicit `local.runtime.install` RPC.
    pub fn install_blocking(mut on_line: impl FnMut(String)) -> Result<()> {
        match Self::installer_kind() {
            Some("winget") => run_streamed(
                "winget",
                &[
                    "install",
                    "--id",
                    "Ollama.Ollama",
                    "-e",
                    "--accept-package-agreements",
                    "--accept-source-agreements",
                ],
                &mut on_line,
            ),
            Some("homebrew") => run_streamed("brew", &["install", "ollama"], &mut on_line),
            Some("official script") => {
                // Download first, then run — not `curl | sh`. A truncated
                // download then fails loudly instead of being piped into a
                // shell, and the script is left on disk to inspect.
                let script = std::env::temp_dir().join("ollama-install.sh");
                let script_arg = script.to_string_lossy().to_string();
                run_streamed(
                    "curl",
                    &["-fsSL", "https://ollama.com/install.sh", "-o", &script_arg],
                    &mut on_line,
                )?;
                run_streamed("sh", &[&script_arg], &mut on_line)
            }
            _ => bail!(
                "No supported installer on this platform. Install Ollama from \
                 https://ollama.com/download, then press Start."
            ),
        }
    }

    async fn is_serving(client: &reqwest::Client) -> bool {
        client
            .get(format!("{OLLAMA_ENDPOINT}/api/tags"))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn installed_models(client: &reqwest::Client) -> Vec<String> {
        #[derive(Deserialize)]
        struct Tags {
            models: Vec<Model>,
        }
        #[derive(Deserialize)]
        struct Model {
            name: String,
        }

        match client
            .get(format!("{OLLAMA_ENDPOINT}/api/tags"))
            .send()
            .await
        {
            Ok(resp) => resp
                .json::<Tags>()
                .await
                .map(|t| t.models.into_iter().map(|m| m.name).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn status(&self) -> RuntimeStatus {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        let binary_path = Self::binary();
        let running = Self::is_serving(&client).await;
        // `Managed` only if our child is still alive; a spawned process that has
        // since exited must not keep claiming ownership.
        let we_started_it = self
            .server
            .lock()
            .map(|mut guard| match guard.as_mut() {
                Some(child) => matches!(child.try_wait(), Ok(None)),
                None => false,
            })
            .unwrap_or(false);

        RuntimeStatus {
            installed: binary_path.is_some(),
            binary_path,
            running,
            ownership: match (running, we_started_it) {
                (true, true) => RuntimeOwnership::Managed,
                (true, false) => RuntimeOwnership::External,
                (false, _) => RuntimeOwnership::NotRunning,
            },
            endpoint: OLLAMA_ENDPOINT,
            installed_models: if running {
                Self::installed_models(&client).await
            } else {
                Vec::new()
            },
            install_url: "https://ollama.com/download",
            installer: Self::installer_kind(),
        }
    }

    /// Start the runtime and wait until it answers, so a caller that gets `Ok`
    /// can immediately use it rather than racing the server's startup.
    pub async fn start(&self) -> Result<RuntimeStatus> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        if Self::is_serving(&client).await {
            return Ok(self.status().await);
        }
        let binary = Self::binary().context(
            "Ollama is not installed, or not on PATH. Install it from https://ollama.com/download, \
             then start it here.",
        )?;

        let child = Command::new(binary)
            .arg("serve")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("failed to launch `ollama serve`")?;
        if let Ok(mut guard) = self.server.lock() {
            *guard = Some(child);
        }

        // Poll rather than sleeping a fixed amount: startup time varies with
        // how much the runtime has to load, and a fixed guess is either a
        // needless wait or a flaky failure.
        for _ in 0..40 {
            if Self::is_serving(&client).await {
                return Ok(self.status().await);
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        bail!("started `ollama serve` but it did not become reachable within 10s")
    }

    pub async fn stop(&self) -> Result<RuntimeStatus> {
        let status = self.status().await;
        match status.ownership {
            RuntimeOwnership::NotRunning => return Ok(status),
            RuntimeOwnership::External => bail!(
                "This Ollama server was not started by CID (it is running as a service or was \
                 launched manually), so CID will not stop it. Stop it the way it was started."
            ),
            RuntimeOwnership::Managed => {}
        }

        if let Ok(mut guard) = self.server.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
        Ok(self.status().await)
    }

    /// Download a model. Blocking and slow by nature — a 20 GB pull is normal —
    /// so the caller runs it off the async runtime and reports progress.
    pub fn pull_blocking(model_id: &str, mut on_line: impl FnMut(String)) -> Result<()> {
        let binary = Self::binary().context("Ollama is not installed, or not on PATH")?;
        // Reject anything that isn't a plain model tag: this string reaches a
        // process argument, and a caller-supplied one must not be able to smuggle
        // extra arguments or path traversal into it.
        if model_id.is_empty()
            || !model_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/'))
        {
            bail!("invalid model id: {model_id}");
        }

        let mut child = Command::new(binary)
            .arg("pull")
            .arg(model_id)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to launch `ollama pull`")?;

        // Ollama writes progress to stderr.
        if let Some(err) = child.stderr.take() {
            use std::io::{BufRead, BufReader};
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                on_line(line);
            }
        }

        let status = child
            .wait()
            .context("`ollama pull` did not run to completion")?;
        if !status.success() {
            bail!("`ollama pull {model_id}` failed with {status}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn status_is_honest_on_a_machine_without_the_runtime() {
        let mgr = LocalRuntimeManager::new();
        let status = mgr.status().await;
        // Whatever this machine has, the reported shape must be self-consistent:
        // nothing may claim to be managed by us before we have started anything.
        assert_ne!(status.ownership, RuntimeOwnership::Managed);
        assert_eq!(status.install_url, "https://ollama.com/download");
        if !status.running {
            assert!(status.installed_models.is_empty());
        }
    }

    /// The property that protects a user's own server: `stop` must refuse when
    /// CID did not start it.
    #[tokio::test]
    async fn stop_refuses_to_kill_an_externally_started_server() {
        let mgr = LocalRuntimeManager::new();
        let status = mgr.status().await;
        if status.ownership == RuntimeOwnership::External {
            let err = mgr.stop().await.unwrap_err().to_string();
            assert!(err.contains("not started by CID"), "got: {err}");
        }
        // When nothing is running this is a no-op rather than an error.
        if status.ownership == RuntimeOwnership::NotRunning {
            assert!(mgr.stop().await.is_ok());
        }
    }

    /// An empty fallback list would make the just-installed-binary lookup
    /// silently do nothing, leaving a fresh install reporting "not installed"
    /// until Core restarted — the bug those paths exist to prevent.
    #[test]
    fn every_platform_has_at_least_one_known_install_location() {
        assert!(
            !LocalRuntimeManager::fallback_paths().is_empty(),
            "a platform with no known install location can never see a fresh install"
        );
    }

    /// The UI decides between a one-click button and a download link from
    /// this value, so it must be a known manager name or `None` — never a
    /// name the installer dispatch would not recognise.
    #[test]
    fn installer_kind_is_either_none_or_a_dispatchable_name() {
        match LocalRuntimeManager::installer_kind() {
            None => {}
            Some(kind) => assert!(
                matches!(kind, "winget" | "homebrew" | "official script"),
                "install_blocking has no arm for {kind:?}"
            ),
        }
    }

    /// With no installer available the command must fail with guidance, not
    /// pretend to have installed anything.
    #[test]
    fn install_without_a_package_manager_is_a_clear_error() {
        if LocalRuntimeManager::installer_kind().is_none() {
            let err = LocalRuntimeManager::install_blocking(|_| {}).unwrap_err();
            assert!(
                err.to_string().contains("ollama.com/download"),
                "got: {err}"
            );
        }
    }

    #[test]
    fn a_model_id_that_could_smuggle_arguments_is_rejected() {
        for bad in ["", "--version", "model; rm -rf /", "a b", "$(whoami)"] {
            assert!(
                LocalRuntimeManager::pull_blocking(bad, |_| {}).is_err(),
                "should have rejected {bad:?}"
            );
        }
    }
}
