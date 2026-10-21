//! Automatic model routing: cost- and performance-aware picks per turn.
//!
//! The user enables it per install (`settings.routing_mode`, default
//! `manual` = today's behavior verbatim). When on, each Implementer or
//! subagent turn resolves through `route_task` instead of the static
//! role/global chain. Three rules keep it predictable:
//!
//! 1. An explicit choice always wins: per-session and per-step overrides
//!    bypass routing entirely — auto never overrules a human.
//! 2. Routing only considers providers that are actually enabled (key or
//!    endpoint present). A single enabled provider is used as-is;
//!    optimization never invents availability.
//! 3. Every pick carries a `RoutingDecision` with the plain-language reason,
//!    surfaced in the UI badge and the `model.route.recommend` RPC — an
//!    automatic choice must be inspectable, never mysterious.
//!
//! Boundaries, stated plainly: task complexity is a keyword/length heuristic
//! (`classify_task`), not a model call — spending a call to choose a call
//! would defeat the cost purpose every turn. Planner/Reviewer turns stay on
//! explicit resolution (planning quality dominates outcomes and planning is
//! infrequent, so there is nothing meaningful to optimize there). The local
//! probe only covers Ollama, matching `LocalRuntimeManager`'s own scope.

use std::time::Duration;

use crate::api::router::AppState;
use crate::api::types::{ModelProvider, RoutingDecision, RoutingMode, Settings, TaskComplexity};

use super::catalog;
use super::{
    catalog_key, is_provider_enabled, provider_api_key, provider_default_model, provider_endpoint,
    ResolvedModelConfig,
};

/// Words that signal work needing a flagship: system-level design,
/// large-scale restructuring, or open-ended hard problems. Matched as
/// substrings on the lowercased task — deliberately coarse; see the module
/// docs for why this is a heuristic and not a call.
const COMPLEX_MARKERS: &[&str] = &[
    "architect",
    "redesign",
    "rewrite",
    "migration",
    "migrate",
    "from scratch",
    "security audit",
    "overhaul",
    "distributed",
    "design a system",
    "scale to",
];

/// Words that signal small, bounded work a cheap model handles well.
const SIMPLE_MARKERS: &[&str] = &[
    "typo",
    "rename",
    "explain",
    "what is",
    "what does",
    "summar",
    "format",
    "lint",
    "comment",
    "document",
    "find ",
    "where is",
    "read ",
    "fix the typo",
    "grammar",
];

/// Classify a turn's task text. Pure and unit-tested: length sets the
/// baseline (a one-liner rarely needs a flagship), markers adjust it.
/// Pasted logs/diffs inflate length without raising difficulty, so length
/// alone can only ever vote Complex at extreme sizes, never Simple.
pub fn classify_task(task: &str) -> TaskComplexity {
    let lower = task.to_lowercase();
    if COMPLEX_MARKERS.iter().any(|m| lower.contains(m)) || task.len() > 2500 {
        return TaskComplexity::Complex;
    }
    if task.len() < 200 && SIMPLE_MARKERS.iter().any(|m| lower.contains(m)) {
        return TaskComplexity::Simple;
    }
    TaskComplexity::Standard
}

/// Pick which installed Ollama model to run. Pure and unit-tested: prefers
/// the machine's recommended model when it is actually installed (matched on
/// the tag base, since Ollama reports `name:tag` and the catalog holds exact
/// tags), otherwise the first installed model with that fact recorded in the
/// reason — never a model that is not on disk.
pub fn select_local_model(installed: &[String], recommended_id: Option<&str>) -> Option<String> {
    if installed.is_empty() {
        return None;
    }
    if let Some(want) = recommended_id {
        let base = want.split(':').next().unwrap_or(want);
        if let Some(hit) = installed
            .iter()
            .find(|m| m.split(':').next().unwrap_or(m) == base)
        {
            return Some(hit.clone());
        }
        if let Some(hit) = installed.iter().find(|m| m.as_str() == want) {
            return Some(hit.clone());
        }
    }
    Some(installed[0].clone())
}

/// Enabled first-party providers with their catalog default and $/M cost
/// (input + output per million tokens). OpenAI-compatible endpoints are
/// deliberately absent: with no registry price their cost is unknown, and
/// "cheapest" must never mean "unknown". The sole-provider fallback in
/// `route_task` covers the case where compatible is all there is.
fn priced_defaults(settings: &Settings) -> Vec<(ModelProvider, String, f64)> {
    let mut out = Vec::new();
    for provider in [
        ModelProvider::Anthropic,
        ModelProvider::OpenAI,
        ModelProvider::Google,
    ] {
        if !is_provider_enabled(settings, &provider) {
            continue;
        }
        let key = match catalog_key(&provider) {
            Some(k) => k,
            None => continue,
        };
        let model_id = provider_default_model(settings, &provider);
        let per_m: f64 = catalog::lookup(key, &model_id)
            .map(|m| m.input_per_million + m.output_per_million)
            .unwrap_or(f64::MAX);
        out.push((provider, model_id, per_m));
    }
    out.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Route one turn: pick (provider, model, reason) for `task` under `settings`
/// policy. Async only because the local-runtime probe is I/O; the decision
/// itself (`decide_route`) is pure and fully unit-tested, including every
/// degrade path. Never fails closed on probe trouble — a dead local runtime
/// degrades to cloud candidates rather than failing the turn.
pub(crate) async fn route_task(
    settings: &Settings,
    task: &str,
    app_state: &AppState,
) -> anyhow::Result<(ResolvedModelConfig, RoutingDecision)> {
    let mode = RoutingMode::parse(&settings.routing_mode);
    // Probe local runtimes for every mode except performance: a flagship-only
    // policy has no use for a probe that can only ever lose.
    let local_model: Option<String> = if mode == RoutingMode::AutoPerformance {
        None
    } else {
        probe_local_model(app_state).await
    };
    decide_route(settings, task, local_model)
}

/// The pure decision core: same inputs, same pick, on any machine — which is
/// what makes the degrade paths (nothing available, local-first with no
/// local runtime) testable without depending on ambient keys or daemons.
pub(crate) fn decide_route(
    settings: &Settings,
    task: &str,
    local_model: Option<String>,
) -> anyhow::Result<(ResolvedModelConfig, RoutingDecision)> {
    let mode = RoutingMode::parse(&settings.routing_mode);
    let complexity = classify_task(task);
    let priced = priced_defaults(settings);

    // Local-first is strict: it is a promise that nothing leaves the
    // machine, so no local runtime means a refusal with the remedy, never a
    // silent cloud fallback. The other modes treat local as preferred.
    if mode == RoutingMode::AutoLocal {
        match local_model {
            Some(tag) => return Ok(local_config(settings, &tag, &mode, &complexity)),
            None => anyhow::bail!(
                "local-first routing found no running local runtime — start Ollama (Models → Local models) or switch routing to balanced"
            ),
        }
    }
    if matches!(mode, RoutingMode::AutoCost | RoutingMode::AutoBalanced) {
        if let Some(tag) = local_model {
            return Ok(local_config(settings, &tag, &mode, &complexity));
        }
    }

    // Sole-provider case (including the OpenAI-compatible slot): use what is
    // enabled rather than failing an optimization with one answer.
    if priced.is_empty() {
        if is_provider_enabled(settings, &ModelProvider::OpenAICompatible) {
            return Ok(compatible_fallback(settings, &mode, &complexity));
        }
        anyhow::bail!(
            "auto routing found nothing to route to — no provider key/endpoint is configured and no local runtime is running"
        );
    }
    let candidates = priced;
    let (provider, model_id, per_m) = match mode {
        RoutingMode::AutoCost => candidates
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no priced provider available"))?,
        RoutingMode::AutoPerformance => candidates
            .last()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no priced provider available"))?,
        _ => {
            // Balanced: complexity picks the tier — cheapest default for
            // simple turns, median for standard, priciest for complex —
            // instead of always burning flagship.
            let idx = match complexity {
                TaskComplexity::Simple => 0,
                TaskComplexity::Standard => candidates.len() / 2,
                TaskComplexity::Complex => candidates.len().saturating_sub(1),
            };
            candidates
                .get(idx)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no priced provider available"))?
        }
    };

    let reason = format!(
        "{} · {} task → {:?} {} (${per_m:.2}/M)",
        mode.as_str(),
        match complexity {
            TaskComplexity::Simple => "simple",
            TaskComplexity::Standard => "standard",
            TaskComplexity::Complex => "complex",
        },
        provider,
        model_id,
    );
    Ok((
        ResolvedModelConfig {
            provider: provider.clone(),
            model_id: model_id.clone(),
            api_key: provider_api_key(settings, &provider),
            endpoint: provider_endpoint(settings, &provider),
        },
        RoutingDecision {
            provider,
            model_id,
            mode,
            complexity,
            reason,
            is_local: false,
        },
    ))
}

fn local_config(
    settings: &Settings,
    tag: &str,
    mode: &RoutingMode,
    complexity: &TaskComplexity,
) -> (ResolvedModelConfig, RoutingDecision) {
    let provider = ModelProvider::Ollama;
    let reason = format!(
        "{} · local Ollama model {tag} running on this machine ($0) — preferred over cloud while it works",
        mode.as_str(),
    );
    (
        ResolvedModelConfig {
            provider: provider.clone(),
            model_id: tag.to_string(),
            api_key: provider_api_key(settings, &provider),
            endpoint: provider_endpoint(settings, &provider),
        },
        RoutingDecision {
            provider,
            model_id: tag.to_string(),
            mode: mode.clone(),
            complexity: complexity.clone(),
            reason,
            is_local: true,
        },
    )
}

fn compatible_fallback(
    settings: &Settings,
    mode: &RoutingMode,
    complexity: &TaskComplexity,
) -> (ResolvedModelConfig, RoutingDecision) {
    let provider = ModelProvider::OpenAICompatible;
    let model_id = provider_default_model(settings, &provider);
    let reason = format!(
        "{} · {:?} is the only enabled provider, used as-is (no catalog price, so no cost comparison applies)",
        mode.as_str(),
        provider,
    );
    (
        ResolvedModelConfig {
            provider: provider.clone(),
            model_id: model_id.clone(),
            api_key: provider_api_key(settings, &provider),
            endpoint: provider_endpoint(settings, &provider),
        },
        RoutingDecision {
            provider,
            model_id,
            mode: mode.clone(),
            complexity: complexity.clone(),
            reason,
            is_local: false,
        },
    )
}

/// What manual mode resolves to, for the recommend RPC: the same static
/// chain a turn would use, explained rather than optimized. Keeps the
/// endpoint truthful in every mode instead of showing a hypothetical auto
/// pick while the install is set to manual.
pub(crate) fn manual_decision(resolved: &ResolvedModelConfig, task: &str) -> RoutingDecision {
    RoutingDecision {
        provider: resolved.provider.clone(),
        model_id: resolved.model_id.clone(),
        mode: RoutingMode::Manual,
        complexity: classify_task(task),
        reason: "manual mode — static role/global resolution, no automatic choice made".to_string(),
        is_local: false,
    }
}

/// Probe Ollama and match an installed model against this machine's
/// recommendation. Bounded: the manager's own 2s HTTP timeouts plus an outer
/// 6s cap, so a wedged daemon delays a turn by seconds, never hangs it.
async fn probe_local_model(app_state: &AppState) -> Option<String> {
    let status = tokio::time::timeout(
        Duration::from_secs(6),
        app_state.local_runtime_manager.status(),
    )
    .await
    .ok()?;
    if !status.running || status.installed_models.is_empty() {
        return None;
    }
    let system = crate::local_models::system::SystemCapability::detect();
    let recommended = crate::local_models::catalog::recommendations(&system)
        .into_iter()
        .find(|m| m.recommended)
        .map(|m| m.option.id.to_string());
    select_local_model(&status.installed_models, recommended.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_plain_tasks_are_simple() {
        assert_eq!(classify_task("fix typo in readme"), TaskComplexity::Simple);
        assert_eq!(
            classify_task("where is the auth middleware defined?"),
            TaskComplexity::Simple
        );
    }

    #[test]
    fn architecture_language_is_complex() {
        assert_eq!(
            classify_task("architect a plugin system for MCP servers"),
            TaskComplexity::Complex
        );
        assert_eq!(
            classify_task("plan the migration from SQLite to Postgres"),
            TaskComplexity::Complex
        );
    }

    #[test]
    fn ordinary_feature_work_is_standard() {
        assert_eq!(
            classify_task("Add a per-session model override with an RPC and a UI switcher"),
            TaskComplexity::Standard
        );
        assert_eq!(
            classify_task("Fix the checkpoint rewind bug"),
            TaskComplexity::Standard
        );
    }

    #[test]
    fn local_pick_prefers_the_recommended_model_when_installed() {
        let installed = vec![
            "tinyllama:latest".to_string(),
            "qwen2.5-coder:7b".to_string(),
        ];
        assert_eq!(
            select_local_model(&installed, Some("qwen2.5-coder:7b")),
            Some("qwen2.5-coder:7b".to_string())
        );
    }

    #[test]
    fn local_pick_matches_tags_by_base_name() {
        let installed = vec!["qwen2.5-coder:14b".to_string()];
        assert_eq!(
            select_local_model(&installed, Some("qwen2.5-coder:7b")),
            Some("qwen2.5-coder:14b".to_string())
        );
    }

    #[test]
    fn local_pick_falls_back_to_first_installed_with_nothing_matching() {
        let installed = vec!["somemodel:v1".to_string()];
        assert_eq!(
            select_local_model(&installed, Some("qwen2.5-coder:7b")),
            Some("somemodel:v1".to_string())
        );
        let empty: Vec<String> = vec![];
        assert_eq!(select_local_model(&empty, Some("qwen2.5-coder:7b")), None);
    }

    #[test]
    fn routing_modes_parse_leniently_and_default_to_manual() {
        assert_eq!(RoutingMode::parse("auto"), RoutingMode::AutoBalanced);
        assert_eq!(RoutingMode::parse("COST"), RoutingMode::AutoCost);
        assert_eq!(RoutingMode::parse("local-first"), RoutingMode::AutoLocal);
        assert_eq!(RoutingMode::parse(""), RoutingMode::Manual);
        assert_eq!(RoutingMode::parse("nonsense"), RoutingMode::Manual);
        assert!(!RoutingMode::Manual.is_auto());
        assert!(RoutingMode::parse("auto").is_auto());
    }
}
