//! Phase 2 Teams Bridge
//!
//! Microsoft Teams incoming webhook connector pattern.
//! Teams message → Session trigger.
//! Session status → Teams channel (Adaptive Card format).
//!
//! Config per Workspace: TeamsConfig with webhook_url.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use crate::api::types::{new_id, now_utc, TeamsConfig, TeamsTriggerParams};

pub struct TeamsBridge {
    configs: RwLock<HashMap<String, TeamsConfig>>,
    http_client: reqwest::Client,
    event_tx: broadcast::Sender<String>,
}

impl TeamsBridge {
    pub fn new(event_tx: broadcast::Sender<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            configs: RwLock::new(HashMap::new()),
            http_client: client,
            event_tx,
        }
    }

    pub async fn configure(
        &self,
        workspace_id: &str,
        config: TeamsConfig,
    ) -> anyhow::Result<TeamsConfig> {
        let mut guard = self.configs.write().await;
        let entry = guard
            .entry(workspace_id.to_string())
            .or_insert_with(TeamsConfig::default);
        *entry = config;
        entry.workspace_id = Some(workspace_id.to_string());
        Ok(entry.clone())
    }

    pub async fn get_config(&self, workspace_id: &str) -> Option<TeamsConfig> {
        let guard = self.configs.read().await;
        guard.get(workspace_id).cloned()
    }

    pub async fn trigger_session(
        &self,
        params: TeamsTriggerParams,
    ) -> anyhow::Result<serde_json::Value> {
        let config = match params.workspace_id.as_deref() {
            Some(wid) => self.get_config(wid).await,
            None => Some(TeamsConfig::default()),
        }
        .unwrap_or_default();

        if config.enabled
            && !config.allowed_channels.is_empty()
            && !config.allowed_channels.contains(&params.message.channel_id)
        {
            anyhow::bail!("Channel not in allowed list");
        }

        if config.enabled
            && !config.allowed_teams.is_empty()
            && !config.allowed_teams.contains(&params.message.team_id)
        {
            anyhow::bail!("Team not in allowed list");
        }

        let command = self.extract_command(&params.message.text, &config);

        let result = serde_json::json!({
            "trigger_id": new_id(),
            "message": params.message,
            "parsed_command": command,
            "triggered_at": now_utc(),
        });

        self.emit_event("teams.trigger.received", &result);

        Ok(result)
    }

    fn extract_command(&self, text: &str, config: &TeamsConfig) -> Option<String> {
        let trimmed = text.trim();
        for keyword in &config.trigger_keywords {
            if let Some(rest) = trimmed.strip_prefix(keyword.as_str()) {
                return Some(rest.trim().to_string());
            }
        }
        Some(trimmed.to_string())
    }

    pub async fn post_status(
        &self,
        workspace_id: &str,
        title: &str,
        body: &str,
        facts: Option<Vec<(String, String)>>,
    ) -> anyhow::Result<()> {
        let config = match self.get_config(workspace_id).await {
            Some(c) if c.enabled && !c.webhook_url.is_empty() => c,
            _ => {
                debug!("Teams not configured for workspace {}", workspace_id);
                return Ok(());
            }
        };

        let adaptive_card = self.build_adaptive_card(title, body, facts);

        let payload = serde_json::json!({
            "type": "message",
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": adaptive_card,
            }]
        });

        let resp = self
            .http_client
            .post(&config.webhook_url)
            .json(&payload)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                if status.is_success() {
                    info!("Posted Teams status for workspace {}", workspace_id);
                } else {
                    warn!("Teams webhook returned {}: {}", status, body);
                    anyhow::bail!("Teams webhook returned {}: {}", status, body);
                }
            }
            Err(e) => {
                warn!("Failed to post to Teams: {}", e);
                anyhow::bail!("Failed to post to Teams: {}", e)
            }
        }
        Ok(())
    }

    fn build_adaptive_card(
        &self,
        title: &str,
        body: &str,
        facts: Option<Vec<(String, String)>>,
    ) -> serde_json::Value {
        let mut items = Vec::new();

        items.push(serde_json::json!({
            "type": "TextBlock",
            "text": body,
            "wrap": true,
        }));

        if let Some(facts_list) = facts {
            if !facts_list.is_empty() {
                let fact_set: Vec<serde_json::Value> = facts_list
                    .iter()
                    .map(|(k, v)| {
                        serde_json::json!({
                            "title": k,
                            "value": v,
                        })
                    })
                    .collect();

                items.push(serde_json::json!({
                    "type": "FactSet",
                    "facts": fact_set,
                }));
            }
        }

        serde_json::json!({
            "type": "AdaptiveCard",
            "version": "1.4",
            "body": [{
                "type": "TextBlock",
                "text": title,
                "size": "large",
                "weight": "bolder",
            }],
            "items": items,
            "actions": [{
                "type": "Action.OpenUrl",
                "title": "View in CID",
                "url": "cid://dashboard",
            }]
        })
    }

    fn emit_event(&self, method: &str, payload: &serde_json::Value) {
        let notif = crate::api::types::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: payload.clone(),
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
    use crate::api::types::TeamsMessage;

    #[test]
    fn test_default_teams_config() {
        let cfg = TeamsConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.webhook_url.is_empty());
        assert_eq!(cfg.trigger_keywords, vec!["@cid"]);
    }

    #[test]
    fn test_teams_message_serde() {
        let msg = TeamsMessage {
            id: "tmsg-1".to_string(),
            team_id: "team-1".to_string(),
            team_name: "Engineering".to_string(),
            channel_id: "ch-1".to_string(),
            channel_name: "General".to_string(),
            user_id: "user-1".to_string(),
            user_name: "dev".to_string(),
            text: "@cid deploy to staging".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: TeamsMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.text, "@cid deploy to staging");
    }

    #[test]
    fn test_command_extraction() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);
        let config = TeamsConfig::default();

        let cmd = bridge.extract_command("@cid build a feature", &config);
        assert_eq!(cmd, Some("build a feature".to_string()));

        let cmd = bridge.extract_command("hello world", &config);
        assert_eq!(cmd, Some("hello world".to_string()));
    }

    #[test]
    fn test_adaptive_card() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);

        let card = bridge.build_adaptive_card(
            "Session Complete",
            "The session has finished successfully.",
            Some(vec![
                ("Status".to_string(), "Done".to_string()),
                ("Branch".to_string(), "cid/abc123".to_string()),
            ]),
        );

        assert_eq!(card["type"], "AdaptiveCard");
        assert_eq!(card["version"], "1.4");
        assert!(!card["items"].is_null());
    }

    #[tokio::test]
    async fn test_bridge_creation() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);
        let cfg = bridge.get_config("ws-1").await;
        assert!(cfg.is_none());
    }

    #[tokio::test]
    async fn test_configure_and_get() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);

        let cfg = TeamsConfig {
            enabled: true,
            webhook_url: "https://teams.webhook.office.com/xxx".to_string(),
            ..Default::default()
        };

        let saved = bridge.configure("ws-1", cfg).await.unwrap();
        assert!(saved.enabled);

        let retrieved = bridge.get_config("ws-1").await.unwrap();
        assert_eq!(
            retrieved.webhook_url,
            "https://teams.webhook.office.com/xxx"
        );
    }

    #[tokio::test]
    async fn test_trigger_session() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);

        let cfg = TeamsConfig {
            enabled: true,
            allowed_channels: vec!["ch-1".to_string()],
            allowed_teams: vec!["team-1".to_string()],
            ..Default::default()
        };
        bridge.configure("ws-1", cfg).await.unwrap();

        let msg = TeamsMessage {
            id: "tmsg-1".to_string(),
            team_id: "team-1".to_string(),
            team_name: "Engineering".to_string(),
            channel_id: "ch-1".to_string(),
            channel_name: "General".to_string(),
            user_id: "user-1".to_string(),
            user_name: "dev".to_string(),
            text: "@cid deploy to staging".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let result = bridge
            .trigger_session(TeamsTriggerParams {
                message: msg,
                workspace_id: Some("ws-1".to_string()),
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_trigger_blocked_team() {
        let (tx, _rx) = broadcast::channel(10);
        let bridge = TeamsBridge::new(tx);

        let cfg = TeamsConfig {
            enabled: true,
            allowed_teams: vec!["other-team".to_string()],
            ..Default::default()
        };
        bridge.configure("ws-1", cfg).await.unwrap();

        let msg = TeamsMessage {
            id: "tmsg-1".to_string(),
            team_id: "team-1".to_string(),
            team_name: "Engineering".to_string(),
            channel_id: "ch-1".to_string(),
            channel_name: "General".to_string(),
            user_id: "user-1".to_string(),
            user_name: "dev".to_string(),
            text: "@cid deploy".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let result = bridge
            .trigger_session(TeamsTriggerParams {
                message: msg,
                workspace_id: Some("ws-1".to_string()),
            })
            .await;

        assert!(result.is_err());
    }
}
