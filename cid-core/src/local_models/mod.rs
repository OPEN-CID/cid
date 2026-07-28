//! Phase 1 local-model runtime detection
//! Detects Ollama, LM Studio, and llama.cpp server via HTTP probing.
//! - Ollama: http://localhost:11434/api/tags + /api/version
//! - LM Studio: http://localhost:1234/v1/models (+ best-effort version endpoints)
//! - llama.cpp: http://localhost:8080/health + /v1/models + /props
//!
//! Design goals:
//! - Swappable mid-Mission: detection is stateless and can be re-run anytime
//! - Graceful degradation: unavailable runtime => available=false, no error bubbling
//! - 2s timeout per request via reqwest client
//! - Tracing for observability

use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, info};

use crate::api::types::{LocalRuntime, ModelInfo, ModelProvider};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const OLLAMA_ENDPOINT: &str = "http://localhost:11434";
const LMSTUDIO_ENDPOINT: &str = "http://localhost:1234";
const LLAMACPP_ENDPOINT: &str = "http://localhost:8080";

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct LocalRuntimeDetector {
    client: reqwest::Client,
}

impl Default for LocalRuntimeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalRuntimeDetector {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .connect_timeout(Duration::from_secs(2))
            // Don't need to keep connections long, detection is ephemeral
            .pool_idle_timeout(Duration::from_secs(5))
            .user_agent("cid-core/local-runtime-detector")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { client }
    }

    /// Probe all known local runtimes in parallel.
    /// Always returns 3 entries (Ollama, LM Studio, llama.cpp) with available flag set.
    pub async fn detect_all(&self) -> Vec<LocalRuntime> {
        info!("Detecting all local runtimes");

        // Run detections concurrently
        let (ollama, lmstudio, llamacpp) = tokio::join!(
            self.detect_ollama(),
            self.detect_lmstudio(),
            self.detect_llamacpp()
        );

        vec![ollama, lmstudio, llamacpp]
    }

    // -----------------------------------------------------------------------
    // Ollama
    // -----------------------------------------------------------------------

    /// GET http://localhost:11434/api/tags -> {models: [{name}]}
    /// GET http://localhost:11434/api/version -> {version}
    pub async fn detect_ollama(&self) -> LocalRuntime {
        const NAME: &str = "Ollama";
        debug!("Probing Ollama at {}", OLLAMA_ENDPOINT);

        match self.probe_ollama().await {
            Ok((models, version)) => {
                info!(
                    "Ollama available at {} with {} models, version={:?}",
                    OLLAMA_ENDPOINT,
                    models.len(),
                    version
                );
                LocalRuntime {
                    runtime_type: ModelProvider::Ollama,
                    name: NAME.to_string(),
                    endpoint: OLLAMA_ENDPOINT.to_string(),
                    available: true,
                    models,
                    version,
                }
            }
            Err(e) => {
                debug!("Ollama not available: {}", e);
                LocalRuntime {
                    runtime_type: ModelProvider::Ollama,
                    name: NAME.to_string(),
                    endpoint: OLLAMA_ENDPOINT.to_string(),
                    available: false,
                    models: vec![],
                    version: None,
                }
            }
        }
    }

    async fn probe_ollama(&self) -> anyhow::Result<(Vec<ModelInfo>, Option<String>)> {
        let url = format!("{}/api/tags", OLLAMA_ENDPOINT);
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("Ollama /api/tags returned {}", resp.status());
        }

        let data: OllamaTagsResponse = resp.json().await?;
        let models = data
            .models
            .into_iter()
            .map(|m| ModelInfo {
                id: m.name.clone(),
                name: m.name.clone(),
                provider: ModelProvider::Ollama,
                context_length: None,
                default: false,
                available: true,
            })
            .collect();

        // Best-effort version detection
        let version = self.detect_ollama_version().await;

        Ok((models, version))
    }

    async fn detect_ollama_version(&self) -> Option<String> {
        let url = format!("{}/api/version", OLLAMA_ENDPOINT);
        match self.client.get(&url).send().await {
            Ok(r) if r.status().is_success() => match r.json::<OllamaVersionResponse>().await {
                Ok(v) => Some(v.version),
                Err(e) => {
                    debug!("Failed to parse Ollama version: {}", e);
                    None
                }
            },
            Ok(r) => {
                debug!("Ollama version endpoint returned {}", r.status());
                None
            }
            Err(e) => {
                debug!("Ollama version probe failed: {}", e);
                None
            }
        }
    }

    // -----------------------------------------------------------------------
    // LM Studio
    // -----------------------------------------------------------------------

    /// GET http://localhost:1234/v1/models -> OpenAI-compatible {data: [{id}]}
    pub async fn detect_lmstudio(&self) -> LocalRuntime {
        const NAME: &str = "LM Studio";
        debug!("Probing LM Studio at {}", LMSTUDIO_ENDPOINT);

        match self.probe_lmstudio().await {
            Ok((models, version)) => {
                info!(
                    "LM Studio available at {} with {} models, version={:?}",
                    LMSTUDIO_ENDPOINT,
                    models.len(),
                    version
                );
                LocalRuntime {
                    runtime_type: ModelProvider::LmStudio,
                    name: NAME.to_string(),
                    endpoint: LMSTUDIO_ENDPOINT.to_string(),
                    available: true,
                    models,
                    version,
                }
            }
            Err(e) => {
                debug!("LM Studio not available: {}", e);
                LocalRuntime {
                    runtime_type: ModelProvider::LmStudio,
                    name: NAME.to_string(),
                    endpoint: LMSTUDIO_ENDPOINT.to_string(),
                    available: false,
                    models: vec![],
                    version: None,
                }
            }
        }
    }

    async fn probe_lmstudio(&self) -> anyhow::Result<(Vec<ModelInfo>, Option<String>)> {
        let url = format!("{}/v1/models", LMSTUDIO_ENDPOINT);
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("LM Studio /v1/models returned {}", resp.status());
        }

        let data: OpenAIModelsResponse = resp.json().await?;
        let models = data
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                name: m.id.clone(),
                provider: ModelProvider::LmStudio,
                context_length: None,
                default: false,
                available: true,
            })
            .collect();

        let version = self.detect_lmstudio_version().await;

        Ok((models, version))
    }

    async fn detect_lmstudio_version(&self) -> Option<String> {
        // LM Studio doesn't have a standardized version endpoint.
        // Try a handful of commonly observed paths, plus Server header heuristics.
        let candidates = [
            format!("{}/api/v0/version", LMSTUDIO_ENDPOINT),
            format!("{}/v1/version", LMSTUDIO_ENDPOINT),
            format!("{}/api/version", LMSTUDIO_ENDPOINT),
        ];

        for url in &candidates {
            match self.client.get(url).send().await {
                Ok(r) if r.status().is_success() => {
                    // Read body once then try multiple parses to avoid moving response twice
                    if let Ok(text) = r.text().await {
                        if let Ok(v) = serde_json::from_str::<OllamaVersionResponse>(&text) {
                            return Some(v.version);
                        }
                        if let Ok(v) = serde_json::from_str::<GenericVersionResponse>(&text) {
                            if let Some(ver) = v.version.or(v.data) {
                                return Some(ver);
                            }
                        }
                        let trimmed = text.trim();
                        if !trimmed.is_empty() && trimmed.len() < 100 {
                            return Some(trimmed.to_string());
                        }
                    }
                }
                _ => continue,
            }
        }

        // Last resort: try to get Server header from /v1/models
        let url = format!("{}/v1/models", LMSTUDIO_ENDPOINT);
        if let Ok(r) = self.client.get(&url).send().await {
            if let Some(srv) = r.headers().get("server") {
                if let Ok(s) = srv.to_str() {
                    // e.g. "LM Studio server/0.2.31" or similar
                    debug!("LM Studio Server header: {}", s);
                    // Extract version-ish token
                    return Some(s.to_string());
                }
            }
        }

        None
    }

    // -----------------------------------------------------------------------
    // llama.cpp
    // -----------------------------------------------------------------------

    /// GET http://localhost:8080/health -> {"status":"ok"}
    /// GET http://localhost:8080/v1/models -> OpenAI-compatible
    /// GET http://localhost:8080/props -> {"model_path":"..."}
    pub async fn detect_llamacpp(&self) -> LocalRuntime {
        const NAME: &str = "llama.cpp";
        debug!("Probing llama.cpp at {}", LLAMACPP_ENDPOINT);

        match self.probe_llamacpp().await {
            Ok((models, version)) => {
                info!(
                    "llama.cpp available at {} with {} models, version={:?}",
                    LLAMACPP_ENDPOINT,
                    models.len(),
                    version
                );
                LocalRuntime {
                    runtime_type: ModelProvider::LlamaCpp,
                    name: NAME.to_string(),
                    endpoint: LLAMACPP_ENDPOINT.to_string(),
                    available: true,
                    models,
                    version,
                }
            }
            Err(e) => {
                debug!("llama.cpp not available: {}", e);
                LocalRuntime {
                    runtime_type: ModelProvider::LlamaCpp,
                    name: NAME.to_string(),
                    endpoint: LLAMACPP_ENDPOINT.to_string(),
                    available: false,
                    models: vec![],
                    version: None,
                }
            }
        }
    }

    async fn probe_llamacpp(&self) -> anyhow::Result<(Vec<ModelInfo>, Option<String>)> {
        // 1. Health check - if this fails, runtime is not available
        let health_url = format!("{}/health", LLAMACPP_ENDPOINT);
        let health_resp = self.client.get(&health_url).send().await?;

        // llama.cpp health can return 200 with {"status":"ok"} or {"status":"loading model"} or 503 if error
        if !health_resp.status().is_success() {
            // Some versions return 503 when loading but still indicate availability
            // We treat >=500 as unavailable unless body says loading
            let status = health_resp.status();
            let body = health_resp.text().await.unwrap_or_default();
            if status.as_u16() == 503 && body.contains("loading") {
                debug!("llama.cpp is loading: {}", body);
            } else {
                anyhow::bail!("llama.cpp /health returned {}: {}", status, body);
            }
        } else {
            // Parse health for logging
            if let Ok(text) = health_resp.text().await {
                debug!("llama.cpp health: {}", text);
            }
        }

        // 2. Try /v1/models (OpenAI compatible, available in newer llama.cpp servers)
        let models = match self.probe_llamacpp_v1_models().await {
            Ok(m) if !m.is_empty() => m,
            Ok(_) => {
                debug!("llama.cpp /v1/models empty, falling back to /props");
                self.probe_llamacpp_props().await.unwrap_or_default()
            }
            Err(e) => {
                debug!(
                    "llama.cpp /v1/models failed ({}), falling back to /props",
                    e
                );
                self.probe_llamacpp_props().await.unwrap_or_default()
            }
        };

        let version = self.detect_llamacpp_version().await;

        Ok((models, version))
    }

    async fn probe_llamacpp_v1_models(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let url = format!("{}/v1/models", LLAMACPP_ENDPOINT);
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("/v1/models returned {}", resp.status());
        }

        let data: OpenAIModelsResponse = resp.json().await?;
        Ok(data
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                name: m.id.clone(),
                provider: ModelProvider::LlamaCpp,
                context_length: None,
                default: true,
                available: true,
            })
            .collect())
    }

    async fn probe_llamacpp_props(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let url = format!("{}/props", LLAMACPP_ENDPOINT);
        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            anyhow::bail!("/props returned {}", resp.status());
        }

        let props: LlamaCppPropsResponse = resp.json().await?;
        let mut models = Vec::new();

        if let Some(model_path) = props.model_path {
            let name = std::path::Path::new(&model_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&model_path)
                .to_string();

            // Strip .gguf extension for nicer display? Keep original as id, clean name for display
            let display_name = name.trim_end_matches(".gguf").to_string();

            models.push(ModelInfo {
                id: name.clone(),
                name: if display_name.is_empty() {
                    name
                } else {
                    display_name
                },
                provider: ModelProvider::LlamaCpp,
                context_length: None,
                default: true,
                available: true,
            });
        } else if let Some(default_settings) = props.default_generation_settings {
            if let Some(model) = default_settings.model {
                models.push(ModelInfo {
                    id: model.clone(),
                    name: model,
                    provider: ModelProvider::LlamaCpp,
                    context_length: None,
                    default: true,
                    available: true,
                });
            }
        }

        // Fallback: if props had no model_path but total_slots >0, assume one generic model
        if models.is_empty() && props.total_slots.unwrap_or(0) > 0 {
            models.push(ModelInfo {
                id: "llama.cpp-model".to_string(),
                name: "llama.cpp model".to_string(),
                provider: ModelProvider::LlamaCpp,
                context_length: None,
                default: true,
                available: true,
            });
        }

        Ok(models)
    }

    async fn detect_llamacpp_version(&self) -> Option<String> {
        // Try /props for version field (some forks include it)
        let props_url = format!("{}/props", LLAMACPP_ENDPOINT);
        if let Ok(r) = self.client.get(&props_url).send().await {
            if r.status().is_success() {
                if let Ok(props) = r.json::<serde_json::Value>().await {
                    // Look for common version keys
                    for key in ["version", "build_info", "server_version"] {
                        if let Some(v) = props.get(key).and_then(|v| v.as_str()) {
                            return Some(v.to_string());
                        }
                    }
                }
            }
        }

        // Try Server header from /health
        let health_url = format!("{}/health", LLAMACPP_ENDPOINT);
        if let Ok(r) = self.client.get(&health_url).send().await {
            if let Some(srv) = r.headers().get("server") {
                if let Ok(s) = srv.to_str() {
                    debug!("llama.cpp Server header: {}", s);
                    return Some(s.to_string());
                }
            }
            if let Some(ver) = r.headers().get("x-llamacpp-version") {
                if let Ok(s) = ver.to_str() {
                    return Some(s.to_string());
                }
            }
        }

        None
    }
}

// ---------------------------------------------------------------------------
// Internal response types (serde)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
// Fields beyond `name` are deserialized for shape-completeness against
// Ollama's real `/api/tags` response but not currently surfaced to callers.
#[allow(dead_code)]
struct OllamaModelEntry {
    name: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    details: Option<serde_json::Value>,
    #[serde(default)]
    modified_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaVersionResponse {
    version: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIModelsResponse {
    #[serde(default)]
    data: Vec<OpenAIModelEntry>,
    // Deserialized for shape-completeness; not currently surfaced to callers.
    #[allow(dead_code)]
    #[serde(default)]
    object: Option<String>,
}

// Fields beyond `id` are deserialized for shape-completeness against the
// OpenAI-compatible `/v1/models` response but not currently surfaced.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAIModelEntry {
    id: String,
    #[serde(default)]
    object: Option<String>,
    #[serde(default)]
    owned_by: Option<String>,
    #[serde(default)]
    created: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GenericVersionResponse {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    data: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlamaCppPropsResponse {
    #[serde(default)]
    model_path: Option<String>,
    #[serde(default)]
    total_slots: Option<u32>,
    #[serde(default)]
    default_generation_settings: Option<LlamaCppDefaultSettings>,
}

#[derive(Debug, Deserialize)]
struct LlamaCppDefaultSettings {
    #[serde(default)]
    model: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests for parsing (no network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ollama_tags() {
        let json = r#"{
            "models": [
                {"name":"llama3.2:latest","model":"llama3.2:latest","size":2000000000,"digest":"abc"},
                {"name":"mistral:7b","model":"mistral:7b","size":4000000000,"digest":"def"}
            ]
        }"#;
        let parsed: OllamaTagsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.models.len(), 2);
        assert_eq!(parsed.models[0].name, "llama3.2:latest");
    }

    #[test]
    fn test_parse_openai_models() {
        let json = r#"{
            "object":"list",
            "data":[
                {"id":"lmstudio-community/Meta-Llama-3-8B","object":"model","owned_by":"lmstudio"},
                {"id":"qwen2-7b-instruct","object":"model"}
            ]
        }"#;
        let parsed: OpenAIModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.data[0].id, "lmstudio-community/Meta-Llama-3-8B");
    }

    #[test]
    fn test_parse_llamacpp_props() {
        let json = r#"{
            "model_path": "/models/llama-3-8b.gguf",
            "total_slots": 1,
            "default_generation_settings": {"model":"llama-3-8b"}
        }"#;
        let parsed: LlamaCppPropsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.model_path.unwrap(), "/models/llama-3-8b.gguf");
    }

    #[test]
    fn test_model_info_mapping() {
        let info = ModelInfo {
            id: "test-model".to_string(),
            name: "test-model".to_string(),
            provider: ModelProvider::Ollama,
            context_length: None,
            default: false,
            available: true,
        };
        assert_eq!(info.provider, ModelProvider::Ollama);
    }

    #[tokio::test]
    async fn test_detector_creation() {
        let _detector = LocalRuntimeDetector::new();
        // Should not panic, should have client configured
        // client is opaque, just ensuring new() doesn't panic
    }

    #[tokio::test]
    async fn test_detect_all_without_servers() {
        // When no servers are running, all should return available=false but not error
        let detector = LocalRuntimeDetector::new();
        let results = detector.detect_all().await;
        assert_eq!(results.len(), 3);
        // We don't assert available status because CI might have servers,
        // just that it returns 3 and doesn't panic
        for runtime in results {
            // models can be empty when unavailable
            if !runtime.available {
                assert!(runtime.models.is_empty());
            }
        }
    }
}
