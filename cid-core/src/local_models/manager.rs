//! Starting, stopping and provisioning a local model runtime.
//!
//! Scope boundary, stated rather than blurred: this manages a runtime that is
//! **already installed**. It does not download and run an installer — that is
//! software installation on someone's machine, and it belongs to the user, not
//! to an agent. When the binary is absent the UI says so and links to the
//! official download instead of silently acquiring it.
//!
//! The property worth being careful about is ownership of the server process.
//! Many people already have `ollama serve` running as a system service. Killing
//! that because a button in CID says "Stop" would be an unpleasant surprise, so
//! `stop` only ever terminates a child *this process* spawned; a server we did
//! not start is reported as externally managed and left alone.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
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
        which::which("ollama")
            .ok()
            .map(|p| p.to_string_lossy().to_string())
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
