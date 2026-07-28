//! Phase 2 Background Model Router
//!
//! Routes low-stakes work to a detected local runtime (Ollama, LM Studio, llama.cpp).
//! Supports: context summaries, commit message drafting, cheap lint suggestions,
//! cheap-tier plan execution.
//!
//! Configurable per Repo Channel (opt-in). Uses LocalRuntimeDetector from
//! local_models/mod.rs to detect available runtimes.
//!
//! Tasks are queued and processed with a concurrency limit. Results are posted
//! back as events via the broadcast channel.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, RwLock};
use tracing::warn;

use crate::api::types::{
    new_id, now_utc, BackgroundModelConfig, BackgroundTask, BackgroundTaskStatus,
    BackgroundTaskSubmitParams, BackgroundTaskType, LocalRuntime, ModelProvider,
};
use crate::local_models::LocalRuntimeDetector;

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

pub struct BackgroundModelRouter {
    configs: Arc<RwLock<HashMap<String, BackgroundModelConfig>>>,
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    detector: LocalRuntimeDetector,
    http_client: reqwest::Client,
    event_tx: broadcast::Sender<String>,
    running_count: Arc<RwLock<HashMap<String, usize>>>,
}

impl BackgroundModelRouter {
    pub fn new(event_tx: broadcast::Sender<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();

        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            tasks: Arc::new(RwLock::new(HashMap::new())),
            detector: LocalRuntimeDetector::new(),
            http_client: client,
            event_tx,
            running_count: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_status(&self, repo_channel_id: &str) -> BackgroundModelConfig {
        let guard = self.configs.read().await;
        guard
            .get(repo_channel_id)
            .cloned()
            .unwrap_or_else(|| BackgroundModelConfig {
                repo_channel_id: repo_channel_id.to_string(),
                ..Default::default()
            })
    }

    pub async fn configure(
        &self,
        repo_channel_id: &str,
        config: BackgroundModelConfig,
    ) -> BackgroundModelConfig {
        let mut guard = self.configs.write().await;
        let entry =
            guard
                .entry(repo_channel_id.to_string())
                .or_insert_with(|| BackgroundModelConfig {
                    repo_channel_id: repo_channel_id.to_string(),
                    ..Default::default()
                });
        *entry = config;
        entry.repo_channel_id = repo_channel_id.to_string();
        entry.clone()
    }

    pub async fn submit_task(
        &self,
        params: BackgroundTaskSubmitParams,
    ) -> anyhow::Result<BackgroundTask> {
        let id = new_id();
        let repo_id = params.repo_channel_id.clone();

        let task = BackgroundTask {
            id: id.clone(),
            task_type: params.task_type.clone(),
            repo_channel_id: repo_id.clone(),
            mission_id: params.mission_id.clone(),
            input: params.input,
            status: BackgroundTaskStatus::Pending,
            result: None,
            error: None,
            runtime: None,
            model: None,
            created_at: now_utc(),
            completed_at: None,
        };

        {
            let mut guard = self.tasks.write().await;
            guard.insert(id.clone(), task.clone());
        }

        let runner = BackgroundModelTaskRunner {
            configs: self.configs.clone(),
            tasks: self.tasks.clone(),
            detector: self.detector.clone(),
            http_client: self.http_client.clone(),
            event_tx: self.event_tx.clone(),
            running_count: self.running_count.clone(),
        };
        let task_id = id.clone();
        tokio::spawn(async move {
            runner.process_task(task_id).await;
        });

        Ok(task)
    }

    pub async fn list_tasks(&self, repo_channel_id: Option<&str>) -> Vec<BackgroundTask> {
        let guard = self.tasks.read().await;
        guard
            .values()
            .filter(|t| repo_channel_id.is_none_or(|r| t.repo_channel_id == r))
            .cloned()
            .collect()
    }
}

struct BackgroundModelTaskRunner {
    configs: Arc<RwLock<HashMap<String, BackgroundModelConfig>>>,
    tasks: Arc<RwLock<HashMap<String, BackgroundTask>>>,
    detector: LocalRuntimeDetector,
    http_client: reqwest::Client,
    event_tx: broadcast::Sender<String>,
    running_count: Arc<RwLock<HashMap<String, usize>>>,
}

impl BackgroundModelTaskRunner {
    async fn process_task(&self, task_id: String) {
        let (task, config) = {
            let guard = self.tasks.read().await;
            let task = match guard.get(&task_id) {
                Some(t) => t.clone(),
                None => return,
            };
            let config_guard = self.configs.read().await;
            let config = config_guard
                .get(&task.repo_channel_id)
                .cloned()
                .unwrap_or_default();
            (task, config)
        };

        if !config.enabled {
            self.update_task_status(&task_id, BackgroundTaskStatus::Failed)
                .await;
            self.set_task_error(&task_id, "Background model not enabled for this repo")
                .await;
            return;
        }

        let max_concurrent = config.max_concurrent_tasks.max(1);
        {
            let mut count_guard = self.running_count.write().await;
            let count = count_guard.entry(task.repo_channel_id.clone()).or_insert(0);
            if *count >= max_concurrent {
                warn!(
                    "Max concurrent background tasks ({}) reached for repo {}",
                    max_concurrent, task.repo_channel_id
                );
                self.update_task_status(&task_id, BackgroundTaskStatus::Pending)
                    .await;
                return;
            }
            *count += 1;
        }

        self.update_task_status(&task_id, BackgroundTaskStatus::Running)
            .await;

        let runtimes = self.detector.detect_all().await;
        let available = runtimes.iter().filter(|r| r.available).collect::<Vec<_>>();

        if available.is_empty() {
            self.update_task_status(&task_id, BackgroundTaskStatus::Failed)
                .await;
            self.set_task_error(&task_id, "No local runtime available")
                .await;
            return;
        }

        let selected = self.pick_runtime(&available, &config);

        let result = match task.task_type {
            BackgroundTaskType::Summarize => self.call_summarize(&task, selected).await,
            BackgroundTaskType::CommitMessage => self.call_commit_message(&task, selected).await,
            BackgroundTaskType::LintSuggestion => self.call_lint_suggestion(&task, selected).await,
            BackgroundTaskType::PlanExecution => self.call_plan_execution(&task, selected).await,
        };

        match result {
            Ok(value) => self.set_task_result(&task_id, value).await,
            Err(e) => self.set_task_error(&task_id, &e.to_string()).await,
        }

        {
            let mut count_guard = self.running_count.write().await;
            if let Some(c) = count_guard.get_mut(&task.repo_channel_id) {
                if *c > 0 {
                    *c -= 1;
                }
            }
        }
    }

    fn pick_runtime<'a>(
        &self,
        available: &[&'a LocalRuntime],
        config: &BackgroundModelConfig,
    ) -> &'a LocalRuntime {
        if let Some(pref) = &config.preferred_runtime {
            if let Some(rt) = available.iter().find(|r| r.runtime_type == *pref) {
                return rt;
            }
        }
        available.first().copied().unwrap_or(available[0])
    }

    async fn call_summarize(
        &self,
        task: &BackgroundTask,
        runtime: &LocalRuntime,
    ) -> anyhow::Result<serde_json::Value> {
        let prompt = format!(
            "Summarize the following context concisely:\n\n{}",
            serde_json::to_string(&task.input).unwrap_or_default()
        );
        let response = self.send_chat(runtime, &prompt).await?;
        Ok(serde_json::json!({ "summary": response }))
    }

    async fn call_commit_message(
        &self,
        task: &BackgroundTask,
        runtime: &LocalRuntime,
    ) -> anyhow::Result<serde_json::Value> {
        let diff = task
            .input
            .get("diff")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = format!(
            "Write a concise, conventional commit message for these changes (one line, 72 chars max):\n\n{}",
            diff
        );
        let response = self.send_chat(runtime, &prompt).await?;
        let message = response
            .lines()
            .next()
            .unwrap_or(&response)
            .trim()
            .to_string();
        Ok(serde_json::json!({ "message": message }))
    }

    async fn call_lint_suggestion(
        &self,
        task: &BackgroundTask,
        runtime: &LocalRuntime,
    ) -> anyhow::Result<serde_json::Value> {
        let code = task
            .input
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let language = task
            .input
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = format!(
            "Review this {} code for potential issues and suggest improvements (be concise):\n\n{}",
            language, code
        );
        let response = self.send_chat(runtime, &prompt).await?;
        Ok(serde_json::json!({ "suggestion": response }))
    }

    async fn call_plan_execution(
        &self,
        task: &BackgroundTask,
        runtime: &LocalRuntime,
    ) -> anyhow::Result<serde_json::Value> {
        let plan = task
            .input
            .get("plan")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = format!(
            "Execute the following plan step and return the result:\n\n{}",
            plan
        );
        let response = self.send_chat(runtime, &prompt).await?;
        Ok(serde_json::json!({ "result": response }))
    }

    async fn send_chat(&self, runtime: &LocalRuntime, prompt: &str) -> anyhow::Result<String> {
        let endpoint = match runtime.runtime_type {
            ModelProvider::Ollama => format!("{}/api/generate", runtime.endpoint),
            _ => format!("{}/v1/chat/completions", runtime.endpoint),
        };

        match runtime.runtime_type {
            ModelProvider::Ollama => {
                let model_name = runtime
                    .models
                    .first()
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                let body = serde_json::json!({
                    "model": model_name,
                    "prompt": prompt,
                    "stream": false,
                });
                let resp = self.http_client.post(&endpoint).json(&body).send().await?;
                let json: serde_json::Value = resp.json().await?;
                let response_text = json
                    .get("response")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                Ok(response_text)
            }
            _ => {
                let model_name = runtime
                    .models
                    .first()
                    .map(|m| m.id.clone())
                    .unwrap_or_default();
                let body = serde_json::json!({
                    "model": model_name,
                    "messages": [{"role": "user", "content": prompt}],
                    "temperature": 0.3,
                    "max_tokens": 1024,
                });
                let resp = self.http_client.post(&endpoint).json(&body).send().await?;
                let json: serde_json::Value = resp.json().await?;
                let response_text = json
                    .get("choices")
                    .and_then(|v| v.get(0))
                    .and_then(|v| v.get("message"))
                    .and_then(|v| v.get("content"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                Ok(response_text.to_string())
            }
        }
    }

    async fn update_task_status(&self, task_id: &str, status: BackgroundTaskStatus) {
        let mut guard = self.tasks.write().await;
        if let Some(task) = guard.get_mut(task_id) {
            task.status = status.clone();
            if matches!(
                status,
                BackgroundTaskStatus::Completed
                    | BackgroundTaskStatus::Failed
                    | BackgroundTaskStatus::Cancelled
            ) {
                task.completed_at = Some(now_utc());
            }
            let task_clone = task.clone();
            drop(guard);
            self.emit_event(
                "background_model.task.updated",
                &serde_json::to_value(task_clone).unwrap_or_default(),
            );
        }
    }

    async fn set_task_result(&self, task_id: &str, result: serde_json::Value) {
        let mut guard = self.tasks.write().await;
        if let Some(task) = guard.get_mut(task_id) {
            task.status = BackgroundTaskStatus::Completed;
            task.result = Some(result);
            task.completed_at = Some(now_utc());
            let task_clone = task.clone();
            drop(guard);
            self.emit_event(
                "background_model.task.completed",
                &serde_json::to_value(task_clone).unwrap_or_default(),
            );
        }
    }

    async fn set_task_error(&self, task_id: &str, error: &str) {
        let mut guard = self.tasks.write().await;
        if let Some(task) = guard.get_mut(task_id) {
            task.status = BackgroundTaskStatus::Failed;
            task.error = Some(error.to_string());
            task.completed_at = Some(now_utc());
        }
    }

    fn emit_event(&self, method: &str, params: &serde_json::Value) {
        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: params.clone(),
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
    fn test_default_config() {
        let cfg = BackgroundModelConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_concurrent_tasks, 2);
        assert_eq!(cfg.enabled_tasks.len(), 2);
    }

    #[test]
    fn test_background_task_types_serde() {
        assert_eq!(
            serde_json::to_string(&BackgroundTaskType::Summarize).unwrap(),
            "\"summarize\""
        );
        assert_eq!(
            serde_json::to_string(&BackgroundTaskType::CommitMessage).unwrap(),
            "\"commit_message\""
        );
    }

    #[test]
    fn test_background_task_status_serde() {
        let statuses = vec![
            BackgroundTaskStatus::Pending,
            BackgroundTaskStatus::Running,
            BackgroundTaskStatus::Completed,
            BackgroundTaskStatus::Failed,
            BackgroundTaskStatus::Cancelled,
        ];
        for s in statuses {
            let json = serde_json::to_string(&s).unwrap();
            let back: BackgroundTaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let (tx, _rx) = broadcast::channel(10);
        let router = BackgroundModelRouter::new(tx);
        let status = router.get_status("repo-1").await;
        assert!(!status.enabled);
        assert_eq!(status.repo_channel_id, "repo-1");
    }

    #[tokio::test]
    async fn test_configure_and_status() {
        let (tx, _rx) = broadcast::channel(10);
        let router = BackgroundModelRouter::new(tx);
        let cfg = BackgroundModelConfig {
            enabled: true,
            enabled_tasks: vec![BackgroundTaskType::Summarize],
            ..Default::default()
        };
        let updated = router.configure("repo-1", cfg).await;
        assert!(updated.enabled);
        assert_eq!(updated.enabled_tasks.len(), 1);

        let status = router.get_status("repo-1").await;
        assert!(status.enabled);
    }

    #[tokio::test]
    async fn test_submit_task_without_runtime() {
        let (tx, _rx) = broadcast::channel(10);
        let router = BackgroundModelRouter::new(tx);
        let cfg = BackgroundModelConfig {
            enabled: true,
            ..Default::default()
        };
        router.configure("repo-1", cfg).await;

        let task = router
            .submit_task(BackgroundTaskSubmitParams {
                task_type: BackgroundTaskType::Summarize,
                repo_channel_id: "repo-1".to_string(),
                mission_id: None,
                input: serde_json::json!({"content": "test"}),
            })
            .await
            .unwrap();

        assert_eq!(task.task_type, BackgroundTaskType::Summarize);
        assert_eq!(task.status, BackgroundTaskStatus::Pending);

        tokio::time::sleep(Duration::from_millis(500)).await;

        let tasks = router.list_tasks(Some("repo-1")).await;
        assert!(!tasks.is_empty());
    }
}
