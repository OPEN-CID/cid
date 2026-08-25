//! Phase 2 MCP Tasks Extension
//!
//! Extends existing MCP client in mcp/mod.rs with long-running task support.
//! Long-running MCP tool calls return a TaskHandle (pollable/subscribable).
//! TaskHandle: id, status (Pending/Running/Completed/Failed), progress, result.
//! Integrates with the Session thread to show "running" status.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

use crate::api::types::{new_id, now_utc, McpTaskCreateParams, McpTaskHandle, McpTaskStatus};

const DEFAULT_TASK_TIMEOUT_SECS: u64 = 300;

pub struct McpTasksManager {
    tasks: Arc<RwLock<HashMap<String, McpTaskHandle>>>,
    mcp_manager: Arc<crate::mcp::McpManager>,
    event_tx: broadcast::Sender<String>,
}

impl McpTasksManager {
    pub fn new(
        mcp_manager: Arc<crate::mcp::McpManager>,
        event_tx: broadcast::Sender<String>,
    ) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            mcp_manager,
            event_tx,
        }
    }

    pub async fn create_task(&self, params: McpTaskCreateParams) -> anyhow::Result<McpTaskHandle> {
        let id = new_id();
        let handle = McpTaskHandle {
            id: id.clone(),
            server_id: params.server_id.clone(),
            tool_name: params.tool_name.clone(),
            arguments: params.arguments.clone(),
            status: McpTaskStatus::Pending,
            progress: None,
            result: None,
            error: None,
            created_at: now_utc(),
            completed_at: None,
        };

        {
            let mut guard = self.tasks.write().await;
            guard.insert(id.clone(), handle.clone());
        }

        self.emit_event("mcp.task.created", &handle);

        let self_task = McpTasksRef {
            tasks: self.tasks.clone(),
            mcp_manager: self.mcp_manager.clone(),
            event_tx: self.event_tx.clone(),
        };

        let task_id = id.clone();
        let server_id = params.server_id.clone();
        let tool_name = params.tool_name.clone();
        let arguments = params.arguments;

        tokio::spawn(async move {
            self_task
                .execute_task(task_id, server_id, tool_name, arguments)
                .await;
        });

        Ok(handle)
    }

    pub async fn poll(&self, task_id: &str) -> Option<McpTaskHandle> {
        let guard = self.tasks.read().await;
        guard.get(task_id).cloned()
    }

    pub async fn subscribe(&self, task_id: &str) -> Option<McpTaskHandle> {
        self.poll(task_id).await
    }

    pub async fn cancel_task(&self, task_id: &str) -> anyhow::Result<Option<McpTaskHandle>> {
        let mut guard = self.tasks.write().await;
        if let Some(handle) = guard.get_mut(task_id) {
            if matches!(
                handle.status,
                McpTaskStatus::Pending | McpTaskStatus::Running
            ) {
                handle.status = McpTaskStatus::Cancelled;
                handle.completed_at = Some(now_utc());
                handle.error = Some("Task cancelled by user".to_string());
                let clone = handle.clone();
                drop(guard);
                self.emit_event("mcp.task.cancelled", &clone);
                return Ok(Some(clone));
            }
            return Ok(Some(handle.clone()));
        }
        Ok(None)
    }

    pub async fn list_tasks(&self) -> Vec<McpTaskHandle> {
        let guard = self.tasks.read().await;
        guard.values().cloned().collect()
    }

    fn emit_event(&self, method: &str, handle: &McpTaskHandle) {
        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: serde_json::to_value(handle).unwrap_or_default(),
        };
        if let Ok(s) = serde_json::to_string(&notif) {
            let _ = self.event_tx.send(s);
        }
    }
}

struct McpTasksRef {
    tasks: Arc<RwLock<HashMap<String, McpTaskHandle>>>,
    mcp_manager: Arc<crate::mcp::McpManager>,
    event_tx: broadcast::Sender<String>,
}

impl McpTasksRef {
    async fn execute_task(
        &self,
        task_id: String,
        server_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    ) {
        self.update_status(&task_id, McpTaskStatus::Running, Some(0.0), None, None);

        info!(
            "Executing MCP task {}: {} on server {}",
            task_id, tool_name, server_id
        );

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(DEFAULT_TASK_TIMEOUT_SECS),
            self.mcp_manager
                .call_tool(&server_id, &tool_name, arguments),
        )
        .await;

        match result {
            Ok(Ok(value)) => {
                self.update_status(
                    &task_id,
                    McpTaskStatus::Completed,
                    Some(1.0),
                    Some(value),
                    None,
                );
                info!("MCP task {} completed successfully", task_id);
            }
            Ok(Err(e)) => {
                self.update_status(
                    &task_id,
                    McpTaskStatus::Failed,
                    None,
                    None,
                    Some(format!("{:?}", e)),
                );
                warn!("MCP task {} failed: {:?}", task_id, e);
            }
            Err(_elapsed) => {
                self.update_status(
                    &task_id,
                    McpTaskStatus::Failed,
                    None,
                    None,
                    Some("Task timed out".to_string()),
                );
                warn!("MCP task {} timed out", task_id);
            }
        }
    }

    fn update_status(
        &self,
        task_id: &str,
        status: McpTaskStatus,
        progress: Option<f64>,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) {
        let handle_clone = {
            let guard = self.tasks.blocking_read();
            let mut cloned = match guard.get(task_id).cloned() {
                Some(h) => h,
                None => return,
            };
            cloned.status = status.clone();
            cloned.progress = progress;
            cloned.result = result;
            cloned.error = error;
            if matches!(
                status,
                McpTaskStatus::Completed | McpTaskStatus::Failed | McpTaskStatus::Cancelled
            ) {
                cloned.completed_at = Some(now_utc());
            }
            cloned
        };

        let method = match status {
            McpTaskStatus::Completed => "mcp.task.completed",
            McpTaskStatus::Failed => "mcp.task.failed",
            _ => "mcp.task.updated",
        };

        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: serde_json::to_value(handle_clone).unwrap_or_default(),
        };
        if let Ok(s) = serde_json::to_string(&notif) {
            let _ = self.event_tx.send(s);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_task_status_serde() {
        let statuses = vec![
            McpTaskStatus::Pending,
            McpTaskStatus::Running,
            McpTaskStatus::Completed,
            McpTaskStatus::Failed,
            McpTaskStatus::Cancelled,
        ];
        for s in statuses {
            let json = serde_json::to_string(&s).unwrap();
            let back: McpTaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn test_mcp_task_handle_serde() {
        let handle = McpTaskHandle {
            id: "task-1".to_string(),
            server_id: "srv-1".to_string(),
            tool_name: "long_task".to_string(),
            arguments: serde_json::json!({"input": "test"}),
            status: McpTaskStatus::Pending,
            progress: None,
            result: None,
            error: None,
            created_at: now_utc(),
            completed_at: None,
        };
        let json = serde_json::to_string(&handle).unwrap();
        let back: McpTaskHandle = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "task-1");
        assert_eq!(back.tool_name, "long_task");
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let mcp = Arc::new(crate::mcp::McpManager::new());
        let (tx, _rx) = broadcast::channel(10);
        let mgr = McpTasksManager::new(mcp, tx);
        let tasks = mgr.list_tasks().await;
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn test_create_and_poll_task() {
        let mcp = Arc::new(crate::mcp::McpManager::new());
        let (tx, _rx) = broadcast::channel(10);
        let mgr = McpTasksManager::new(mcp, tx);

        let handle = mgr
            .create_task(McpTaskCreateParams {
                server_id: "srv-1".to_string(),
                tool_name: "test_tool".to_string(),
                arguments: serde_json::json!({}),
            })
            .await
            .unwrap();

        assert_eq!(handle.status, McpTaskStatus::Pending);
        assert_eq!(handle.tool_name, "test_tool");

        let polled = mgr.poll(&handle.id).await.unwrap();
        assert_eq!(polled.id, handle.id);
    }

    #[tokio::test]
    async fn test_cancel_task() {
        let mcp = Arc::new(crate::mcp::McpManager::new());
        let (tx, _rx) = broadcast::channel(10);
        let mgr = McpTasksManager::new(mcp, tx);

        let handle = mgr
            .create_task(McpTaskCreateParams {
                server_id: "srv-1".to_string(),
                tool_name: "long_task".to_string(),
                arguments: serde_json::json!({}),
            })
            .await
            .unwrap();

        let cancelled = mgr.cancel_task(&handle.id).await.unwrap();
        assert!(cancelled.is_some());
        assert_eq!(cancelled.unwrap().status, McpTaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let mcp = Arc::new(crate::mcp::McpManager::new());
        let (tx, _rx) = broadcast::channel(10);
        let mgr = McpTasksManager::new(mcp, tx);

        let result = mgr.cancel_task("nonexistent").await.unwrap();
        assert!(result.is_none());
    }
}
