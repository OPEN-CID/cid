use crate::api::types::PtyInstance;
use anyhow::{Context, Result};
use chrono::Utc;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

pub struct PtySession {
    pub instance: PtyInstance,
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub writer: Box<dyn Write + Send>,
    pub output_tx: broadcast::Sender<String>,
}

pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[allow(unused_mut)]
    pub fn create_pty(
        &self,
        session_id: &str,
        workdir: &str,
        cols: u16,
        rows: u16,
        workdir_kind: crate::api::types::PtyWorkdir,
    ) -> Result<PtyInstance> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open pty")?;

        let shell = if cfg!(windows) {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
        };

        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(workdir);

        if !cfg!(windows) && shell.contains("bash") {
            cmd.args(["-l"]);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn command in pty")?;
        let mut master = pair.master;
        // Need mut for take_writer
        let writer = master.take_writer()?;

        let id = uuid::Uuid::new_v4().to_string();
        let (tx, _rx) = broadcast::channel(1000);

        let instance = PtyInstance {
            id: id.clone(),
            session_id: session_id.to_string(),
            cols,
            rows,
            created_at: Utc::now(),
            cwd: workdir.to_string(),
            workdir: workdir_kind,
        };

        // Spawn reader task - single thread per PTY that broadcasts output
        let tx_clone = tx.clone();
        let mut reader = master.try_clone_reader()?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]).to_string();
                        // If no receivers, send will fail but that's ok (broadcast channel)
                        let _ = tx_clone.send(text);
                    }
                    Err(_) => break,
                }
            }
        });

        let session = PtySession {
            instance: instance.clone(),
            master,
            child,
            writer,
            output_tx: tx,
        };

        self.sessions.lock().unwrap().insert(id, session);
        Ok(instance)
    }

    pub fn write(&self, pty_id: &str, data: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(sess) = sessions.get_mut(pty_id) {
            sess.writer.write_all(data.as_bytes())?;
            sess.writer.flush()?;
            Ok(())
        } else {
            anyhow::bail!("PTY not found: {}", pty_id)
        }
    }

    pub fn resize(&self, pty_id: &str, cols: u16, rows: u16) -> Result<()> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(sess) = sessions.get(pty_id) {
            sess.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            Ok(())
        } else {
            anyhow::bail!("PTY not found: {}", pty_id)
        }
    }

    pub fn kill(&self, pty_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(mut sess) = sessions.remove(pty_id) {
            let _ = sess.child.kill();
            Ok(())
        } else {
            anyhow::bail!("PTY not found: {}", pty_id)
        }
    }

    pub fn list(&self, session_id: &str) -> Vec<PtyInstance> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .values()
            .filter(|s| s.instance.session_id == session_id)
            .map(|s| s.instance.clone())
            .collect()
    }

    /// Deprecated: use get_receiver instead to avoid thread-per-subscriber leak
    /// This method now just calls get_receiver and spawns a thread for backwards compat
    #[deprecated(note = "Use get_receiver instead to avoid thread leak")]
    pub fn subscribe_output<F>(&self, pty_id: &str, callback: F) -> Result<()>
    where
        F: Fn(String) + Send + 'static,
    {
        let mut rx = self.get_receiver(pty_id)?;
        std::thread::spawn(move || {
            while let Ok(data) = rx.blocking_recv() {
                callback(data);
            }
        });
        Ok(())
    }

    pub fn get_receiver(&self, pty_id: &str) -> Result<broadcast::Receiver<String>> {
        let sessions = self.sessions.lock().unwrap();
        if let Some(sess) = sessions.get(pty_id) {
            Ok(sess.output_tx.subscribe())
        } else {
            anyhow::bail!("PTY not found")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_pty_manager_new() {
        let mgr = PtyManager::new();
        assert_eq!(mgr.list("nonexistent").len(), 0);
    }
}
