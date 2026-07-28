//! Phase 2 Slack Bridge
//!
//! HTTP webhook-based bridge (no full Slack SDK needed for Phase 2).
//! Slack message/reaction → Mission trigger.
//! Mission status/approval → Slack channel post.
//! Slash command `/cid` support.
//!
//! Config per Workspace: SlackConfig with webhook_url, signing_secret, bot_token.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use crate::api::types::{new_id, now_utc, SlackConfig, SlackTrigger, SlackTriggerParams};

pub struct SlackBridge {
    configs: RwLock<HashMap<String, SlackConfig>>,
    triggers: RwLock<HashMap<String, SlackTrigger>>,
    http_client: reqwest::Client,
    event_tx: broadcast::Sender<String>,
}

impl SlackBridge {
    pub fn new(event_tx: broadcast::Sender<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            configs: RwLock::new(HashMap::new()),
            triggers: RwLock::new(HashMap::new()),
            http_client: client,
            event_tx,
        }
    }

    pub async fn configure(
        &self,
        workspace_id: &str,
        config: SlackConfig,
    ) -> anyhow::Result<SlackConfig> {
        let mut guard = self.configs.write().await;
        let entry = guard
            .entry(workspace_id.to_string())
            .or_insert_with(SlackConfig::default);
        *entry = config;
        entry.workspace_id = Some(workspace_id.to_string());
        Ok(entry.clone())
    }

    pub async fn get_config(&self, workspace_id: &str) -> Option<SlackConfig> {
        let guard = self.configs.read().await;
        guard.get(workspace_id).cloned()
    }

    pub async fn trigger_mission(
        &self,
        params: SlackTriggerParams,
    ) -> anyhow::Result<SlackTrigger> {
        let config = match params.workspace_id.as_deref() {
            Some(wid) => self.get_config(wid).await,
            None => Some(SlackConfig::default()),
        }
        .unwrap_or_default();

        if config.enabled
            && !config.allowed_channels.is_empty()
            && !config.allowed_channels.contains(&params.message.channel_id)
        {
            anyhow::bail!("Channel not in allowed list");
        }

        let text = params.message.text.clone();
        let (command, args) = self.parse_command(&text, &config);

        let id = new_id();
        let trigger = SlackTrigger {
            id: id.clone(),
            message: params.message.clone(),
            triggered_at: now_utc(),
            parsed_command: command,
            parsed_args: args,
            mission_id: None,
        };

        let mut guard = self.triggers.write().await;
        guard.insert(id.clone(), trigger.clone());
        drop(guard);

        self.emit_event("slack.trigger.received", &trigger);

        Ok(trigger)
    }

    fn parse_command(&self, text: &str, config: &SlackConfig) -> (Option<String>, Option<String>) {
        let prefix = config.trigger_prefix.as_deref().unwrap_or("/cid");

        let trimmed = text.trim();
        let cmd_text = trimmed
            .strip_prefix(prefix)
            .map(str::trim)
            .unwrap_or(trimmed);

        if cmd_text.is_empty() {
            return (Some("help".to_string()), None);
        }

        let parts: Vec<&str> = cmd_text.splitn(2, char::is_whitespace).collect();
        let command = if parts.is_empty() {
            None
        } else {
            Some(parts[0].to_string())
        };
        let args = parts.get(1).map(|s| s.to_string());

        (command, args)
    }

    pub async fn post_status(&self, workspace_id: &str, message: &str) -> anyhow::Result<()> {
        let config = match self.get_config(workspace_id).await {
            Some(c) if c.enabled && !c.webhook_url.is_empty() => c,
            _ => {
                debug!("Slack not configured for workspace {}", workspace_id);
                return Ok(());
            }
        };

        let channel = config.default_channel.as_deref().unwrap_or("#general");
        let body = serde_json::json!({
            "channel": channel,
            "text": message,
            "username": "CID",
            "icon_emoji": ":robot_face:",
        });

        let resp = self
            .http_client
            .post(&config.webhook_url)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                info!("Posted Slack status to {}", channel);
                Ok(())
            }
            Ok(r) => {
                warn!("Slack webhook returned {}", r.status());
                anyhow::bail!("Slack webhook returned {}", r.status())
            }
            Err(e) => {
                warn!("Failed to post to Slack: {}", e);
                anyhow::bail!("Failed to post to Slack: {}", e)
            }
        }
    }

    pub async fn link_trigger_to_mission(
        &self,
        trigger_id: &str,
        mission_id: &str,
    ) -> anyhow::Result<()> {
        let mut guard = self.triggers.write().await;
        if let Some(trigger) = guard.get_mut(trigger_id) {
            trigger.mission_id = Some(mission_id.to_string());
            let clone = trigger.clone();
            drop(guard);
            self.emit_event("slack.trigger.linked", &clone);
        }
        Ok(())
    }

    pub async fn list_triggers(&self) -> Vec<SlackTrigger> {
        let guard = self.triggers.read().await;
        guard.values().cloned().collect()
    }

    fn emit_event(&self, method: &str, payload: &impl serde::Serialize) {
        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: serde_json::to_value(payload).unwrap_or_default(),
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
    use crate::api::types::SlackMessage;

    #[test]
    fn test_default_slack_config() {
        let cfg = SlackConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.trigger_prefix, Some("/cid".to_string()));
        assert!(cfg.webhook_url.is_empty());
    }

    #[test]
    fn test_slack_message_serde() {
        let msg = SlackMessage {
            id: "msg-1".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "general".to_string(),
            user_id: "U456".to_string(),
            user_name: "testuser".to_string(),
            text: "Hello CID!".to_string(),
            timestamp: "1234567890.123".to_string(),
            thread_ts: None,
            reactions: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: SlackMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "Hello CID!");
    }

    #[test]
    fn test_command_parsing() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);
        let config = SlackConfig::default();

        let (cmd, args) = bridge.parse_command("/cid plan Implement feature X", &config);
        assert_eq!(cmd, Some("plan".to_string()));
        assert_eq!(args, Some("Implement feature X".to_string()));

        let (cmd, args) = bridge.parse_command("/cid", &config);
        assert_eq!(cmd, Some("help".to_string()));
        assert_eq!(args, None);
    }

    #[tokio::test]
    async fn test_bridge_creation() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);
        let cfg = bridge.get_config("workspace-1").await;
        assert!(cfg.is_none());
    }

    #[tokio::test]
    async fn test_configure_and_get() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);

        let cfg = SlackConfig {
            enabled: true,
            webhook_url: "https://hooks.slack.com/services/xxx".to_string(),
            ..Default::default()
        };

        let saved = bridge.configure("ws-1", cfg).await.unwrap();
        assert!(saved.enabled);

        let retrieved = bridge.get_config("ws-1").await.unwrap();
        assert_eq!(
            retrieved.webhook_url,
            "https://hooks.slack.com/services/xxx"
        );
    }

    #[tokio::test]
    async fn test_trigger_mission() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);

        let cfg = SlackConfig {
            enabled: true,
            allowed_channels: vec!["C123".to_string()],
            ..Default::default()
        };
        bridge.configure("ws-1", cfg).await.unwrap();

        let msg = SlackMessage {
            id: "msg-1".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "general".to_string(),
            user_id: "U456".to_string(),
            user_name: "testuser".to_string(),
            text: "/cid build a login page".to_string(),
            timestamp: "1234567890.123".to_string(),
            thread_ts: None,
            reactions: vec![],
        };

        let trigger = bridge
            .trigger_mission(SlackTriggerParams {
                message: msg,
                workspace_id: Some("ws-1".to_string()),
            })
            .await
            .unwrap();

        assert_eq!(trigger.parsed_command, Some("build".to_string()));
        assert_eq!(trigger.parsed_args, Some("a login page".to_string()));

        let triggers = bridge.list_triggers().await;
        assert_eq!(triggers.len(), 1);
    }

    #[tokio::test]
    async fn test_trigger_blocked_channel() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);

        let cfg = SlackConfig {
            enabled: true,
            allowed_channels: vec!["C999".to_string()],
            ..Default::default()
        };
        bridge.configure("ws-1", cfg).await.unwrap();

        let msg = SlackMessage {
            id: "msg-1".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "general".to_string(),
            user_id: "U456".to_string(),
            user_name: "testuser".to_string(),
            text: "/cid build".to_string(),
            timestamp: "1234567890.123".to_string(),
            thread_ts: None,
            reactions: vec![],
        };

        let result = bridge
            .trigger_mission(SlackTriggerParams {
                message: msg,
                workspace_id: Some("ws-1".to_string()),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_link_trigger_to_mission() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = SlackBridge::new(tx);

        let msg = SlackMessage {
            id: "msg-1".to_string(),
            channel_id: "C123".to_string(),
            channel_name: "general".to_string(),
            user_id: "U456".to_string(),
            user_name: "testuser".to_string(),
            text: "/cid build".to_string(),
            timestamp: "1234567890.123".to_string(),
            thread_ts: None,
            reactions: vec![],
        };

        let trigger = bridge
            .trigger_mission(SlackTriggerParams {
                message: msg,
                workspace_id: None,
            })
            .await
            .unwrap();

        bridge
            .link_trigger_to_mission(&trigger.id, "mission-1")
            .await
            .unwrap();

        let triggers = bridge.list_triggers().await;
        assert_eq!(triggers[0].mission_id, Some("mission-1".to_string()));
    }
}
