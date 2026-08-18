/*!
 * The model catalog: which models exist, how big their context window is, and
 * what a token actually costs.
 *
 * This used to be three hand-written `const` arrays in `model/mod.rs`. They
 * went stale silently and were wrong in three separate ways at once, all found
 * by diffing them against a live registry rather than by any test:
 *
 *   - Google offered `gemini-1.5-pro`/`-flash`, which no longer exist at all —
 *     and one of them was marked the default, so the picker's headline Google
 *     option was guaranteed to 404 on first use.
 *   - OpenAI's list stopped at the `gpt-4o`/`o1` generation.
 *   - `claude-sonnet-5` was priced at the sonnet-tier $3/$15 when it is
 *     actually $2/$10, so every spend estimate on what is now the *default*
 *     model — and therefore every governance spend cap — was 50% high.
 *
 * The fix is to stop hand-maintaining the list. This module reads the
 * [models.dev](https://models.dev) registry (the same open catalog `opencode`
 * uses), which publishes ids, context limits and per-million-token pricing for
 * every provider, and layers three sources so a failure at any level degrades
 * instead of breaking:
 *
 *   1. **Live** — fetched once per process on startup, cached to disk.
 *   2. **Disk cache** — a previous fetch, used while offline and on next boot.
 *   3. **Bundled snapshot** — `catalog_bundled.rs`, generated at build time by
 *      `scripts/generate-model-catalog.mjs`, so a machine that has never had
 *      network still gets a correct, if dated, catalog.
 *
 * Honest limits, stated rather than implied: the live fetch is best-effort and
 * never blocks a turn — if it fails, CID runs on whatever tier was reachable
 * and says so in the log. Nothing here validates that the configured provider
 * will actually *serve* a given id; the registry is a good signal, not the
 * provider's own `/models` endpoint. A user-typed custom model id is always
 * honoured even when the catalog has never heard of it (see
 * `context_window_tokens` and `estimate_cost_usd`'s fallbacks in `mod.rs`),
 * because pinning an id the catalog lags behind on is a legitimate thing to do.
 */

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// One model, flattened to only what CID actually uses.
///
/// `&'static str` so the generated snapshot is a `const` with no allocation;
/// the live path builds owned copies through [`OwnedCatalogModel`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatalogModel {
    pub id: &'static str,
    pub name: &'static str,
    pub context: u32,
    pub input_per_million: f64,
    pub output_per_million: f64,
}

/// The runtime (non-`const`) form, built from the live registry or disk cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedCatalogModel {
    pub id: String,
    pub name: String,
    pub context: u32,
    pub input_per_million: f64,
    pub output_per_million: f64,
}

impl From<&CatalogModel> for OwnedCatalogModel {
    fn from(m: &CatalogModel) -> Self {
        Self {
            id: m.id.to_string(),
            name: m.name.to_string(),
            context: m.context,
            input_per_million: m.input_per_million,
            output_per_million: m.output_per_million,
        }
    }
}

/// Registry keys, matching models.dev's provider ids.
pub const ANTHROPIC: &str = "anthropic";
pub const OPENAI: &str = "openai";
pub const GOOGLE: &str = "google";

/// A refetch is attempted when the cache on disk is older than this. A day is
/// deliberately unambitious: model catalogs move on the order of weeks, and a
/// stale-by-a-day price is far cheaper than hammering a public registry.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

/// Models kept per provider, newest first.
///
/// Must match `PER_PROVIDER_LIMIT` in `scripts/generate-model-catalog.mjs`:
/// without it the live path returned every match (37 OpenAI entries) while the
/// bundled snapshot held 10, so the picker silently changed size depending on
/// whether the machine had network. Same number in both places, one behavior.
const PER_PROVIDER_LIMIT: usize = 10;

/// Bumped whenever the cached shape or the selection rule changes, so a cache
/// written by an older build is discarded rather than trusted. Found the hard
/// way: after `PER_PROVIDER_LIMIT` was introduced, a cache written by the
/// build before it kept serving 37 OpenAI models, because the limit was only
/// applied when parsing the registry — never when loading the cache.
const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct CachedCatalog {
    #[serde(default)]
    schema_version: u32,
    /// Unix seconds; compared against `CACHE_TTL` to decide on a refetch.
    fetched_at_secs: u64,
    providers: HashMap<String, Vec<OwnedCatalogModel>>,
}

/// Process-wide catalog. `RwLock` rather than `OnceLock` alone because the
/// live refresh replaces the contents after startup, while sync callers
/// (`estimate_cost_usd`, `context_window_tokens`) read it on every turn.
static CATALOG: RwLock<Option<HashMap<String, Vec<OwnedCatalogModel>>>> = RwLock::new(None);

fn bundled_map() -> HashMap<String, Vec<OwnedCatalogModel>> {
    use super::catalog_bundled as bundled;
    let mut map = HashMap::new();
    for (key, models) in [
        (ANTHROPIC, bundled::ANTHROPIC_MODELS),
        (OPENAI, bundled::OPENAI_MODELS),
        (GOOGLE, bundled::GOOGLE_MODELS),
    ] {
        map.insert(
            key.to_string(),
            models.iter().map(OwnedCatalogModel::from).collect(),
        );
    }
    map
}

fn cache_path() -> PathBuf {
    let mut p = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("cid");
    p.push("model-catalog.json");
    p
}

fn read_cache() -> Option<CachedCatalog> {
    let raw = std::fs::read_to_string(cache_path()).ok()?;
    let mut cached: CachedCatalog = serde_json::from_str(&raw).ok()?;
    if cached.schema_version != CACHE_SCHEMA_VERSION {
        debug!(
            "model catalog: discarding cache written by schema v{} (this build expects v{})",
            cached.schema_version, CACHE_SCHEMA_VERSION
        );
        return None;
    }
    // Belt and braces alongside the version check: enforce the per-provider cap
    // on the way *in*, so the "at most N models per provider" invariant holds
    // whatever wrote the cache.
    for list in cached.providers.values_mut() {
        list.truncate(PER_PROVIDER_LIMIT);
    }
    Some(cached)
}

fn cache_is_fresh(c: &CachedCatalog) -> bool {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(c.fetched_at_secs) < CACHE_TTL.as_secs()
}

/// The catalog every read goes through, initialising from disk cache (if any)
/// or the bundled snapshot on first use. Never fetches — the network refresh is
/// [`refresh_in_background`], so no lookup on a turn's hot path can block.
fn with_catalog<T>(f: impl FnOnce(&HashMap<String, Vec<OwnedCatalogModel>>) -> T) -> T {
    {
        let guard = CATALOG.read().unwrap();
        if let Some(map) = guard.as_ref() {
            return f(map);
        }
    }
    let initial = match read_cache() {
        Some(c) => {
            debug!(
                "model catalog: loaded {} providers from disk cache",
                c.providers.len()
            );
            c.providers
        }
        None => bundled_map(),
    };
    let mut guard = CATALOG.write().unwrap();
    // Another thread may have initialised it while the read lock was released.
    let map = guard.get_or_insert(initial);
    f(map)
}

/// Every catalogued model for a provider, newest first.
pub fn models_for(provider_key: &str) -> Vec<OwnedCatalogModel> {
    with_catalog(|m| m.get(provider_key).cloned().unwrap_or_default())
}

/// Look up one model. `None` means "not in the catalog", which is a normal
/// case (a custom or brand-new id), not an error.
pub fn lookup(provider_key: &str, model_id: &str) -> Option<OwnedCatalogModel> {
    with_catalog(|m| {
        m.get(provider_key)
            .and_then(|list| list.iter().find(|x| x.id == model_id).cloned())
    })
}

/// The id a provider falls back to when nothing is configured — the newest
/// entry, since the generator emits newest-first.
pub fn default_model_for(provider_key: &str) -> Option<String> {
    with_catalog(|m| {
        m.get(provider_key)
            .and_then(|list| list.first())
            .map(|x| x.id.clone())
    })
}

/// Parses models.dev's `api.json` into the shape above, applying the same
/// selection rule as the generator: tool-calling, text-output, priced models
/// only, since a model that cannot call a tool is unusable as a CID agent and
/// image/TTS/embedding entries are noise in a picker.
fn parse_registry(raw: &str) -> Result<HashMap<String, Vec<OwnedCatalogModel>>> {
    let root: serde_json::Value =
        serde_json::from_str(raw).context("model registry response was not valid JSON")?;
    let mut out = HashMap::new();

    for key in [ANTHROPIC, OPENAI, GOOGLE] {
        let Some(models) = root
            .get(key)
            .and_then(|p| p.get("models"))
            .and_then(|m| m.as_object())
        else {
            continue;
        };

        let mut selected: Vec<(String, OwnedCatalogModel)> = Vec::new();
        for (id, m) in models {
            if m.get("tool_call").and_then(|v| v.as_bool()) != Some(true) {
                continue;
            }
            let outputs = m
                .get("modalities")
                .and_then(|x| x.get("output"))
                .and_then(|x| x.as_array());
            let text_only = outputs.is_some_and(|a| a.len() == 1 && a[0].as_str() == Some("text"));
            if !text_only {
                continue;
            }
            // A date-suffixed alias duplicates its rolling id; keep the latter.
            if id
                .rsplit('-')
                .next()
                .is_some_and(|t| t.len() == 8 && t.bytes().all(|b| b.is_ascii_digit()))
            {
                continue;
            }
            let (Some(context), Some(input), Some(output)) = (
                m.get("limit")
                    .and_then(|l| l.get("context"))
                    .and_then(|v| v.as_u64()),
                m.get("cost")
                    .and_then(|c| c.get("input"))
                    .and_then(|v| v.as_f64()),
                m.get("cost")
                    .and_then(|c| c.get("output"))
                    .and_then(|v| v.as_f64()),
            ) else {
                continue;
            };

            selected.push((
                m.get("release_date")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                OwnedCatalogModel {
                    id: id.clone(),
                    name: m
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id.as_str())
                        .to_string(),
                    context: context.min(u32::MAX as u64) as u32,
                    input_per_million: input,
                    output_per_million: output,
                },
            ));
        }

        selected.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        selected.truncate(PER_PROVIDER_LIMIT);
        if !selected.is_empty() {
            out.insert(
                key.to_string(),
                selected.into_iter().map(|(_, m)| m).collect(),
            );
        }
    }

    if out.is_empty() {
        anyhow::bail!("model registry contained no usable models for any known provider");
    }
    Ok(out)
}

/// Fetch the registry and replace the in-memory catalog, writing a disk cache.
/// `registry_url` is a parameter so tests can point it at a local mock server
/// rather than asserting against the real internet.
pub async fn refresh_from(registry_url: &str) -> Result<usize> {
    let body = reqwest::Client::new()
        .get(registry_url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("fetching model registry {registry_url}"))?
        .error_for_status()?
        .text()
        .await?;

    let providers = parse_registry(&body)?;
    let count = providers.values().map(|v| v.len()).sum();

    let cached = CachedCatalog {
        schema_version: CACHE_SCHEMA_VERSION,
        fetched_at_secs: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        providers: providers.clone(),
    };
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(&cached) {
        // A cache write failure is not fatal: the process still has the fresh
        // catalog in memory, it just won't survive a restart.
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("model catalog: could not write cache to {path:?}: {e}");
            }
        }
        Err(e) => warn!("model catalog: could not serialize cache: {e}"),
    }

    *CATALOG.write().unwrap() = Some(providers);
    Ok(count)
}

/// The public registry. Overridable so a deployment behind a strict egress
/// policy can mirror it rather than lose live pricing entirely.
pub fn registry_url() -> String {
    std::env::var("CID_MODELS_REGISTRY_URL")
        .unwrap_or_else(|_| "https://models.dev/api.json".to_string())
}

/// Kick off a best-effort refresh at startup. Deliberately fire-and-forget:
/// Core must boot and serve turns with no network, so a failure here is logged
/// and the bundled/cached catalog stands.
pub fn refresh_in_background() {
    if let Some(c) = read_cache() {
        if cache_is_fresh(&c) {
            debug!("model catalog: disk cache is fresh, skipping refresh");
            return;
        }
    }
    tokio::spawn(async {
        let url = registry_url();
        match refresh_from(&url).await {
            Ok(n) => info!("model catalog: refreshed {n} models from {url}"),
            Err(e) => warn!(
                "model catalog: live refresh failed ({e}); \
                 using the cached or bundled snapshot instead"
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "anthropic": { "models": {
        "claude-sonnet-5": {
          "name": "Claude Sonnet 5", "tool_call": true, "release_date": "2026-06-29",
          "modalities": { "output": ["text"] },
          "limit": { "context": 1000000 }, "cost": { "input": 2, "output": 10 }
        },
        "claude-haiku-4-5-20251001": {
          "name": "dated alias", "tool_call": true, "release_date": "2025-10-15",
          "modalities": { "output": ["text"] },
          "limit": { "context": 200000 }, "cost": { "input": 1, "output": 5 }
        },
        "claude-opus-5": {
          "name": "Claude Opus 5", "tool_call": true, "release_date": "2026-07-24",
          "modalities": { "output": ["text"] },
          "limit": { "context": 1000000 }, "cost": { "input": 5, "output": 25 }
        }
      }},
      "google": { "models": {
        "gemini-image": {
          "name": "an image model", "tool_call": true, "release_date": "2026-05-28",
          "modalities": { "output": ["text", "image"] },
          "limit": { "context": 131072 }, "cost": { "input": 2, "output": 120 }
        },
        "gemini-no-tools": {
          "name": "no tool calling", "tool_call": false, "release_date": "2026-05-28",
          "modalities": { "output": ["text"] },
          "limit": { "context": 131072 }, "cost": { "input": 1, "output": 2 }
        },
        "gemini-3.6-flash": {
          "name": "Gemini 3.6 Flash", "tool_call": true, "release_date": "2026-07-21",
          "modalities": { "output": ["text"] },
          "limit": { "context": 1048576 }, "cost": { "input": 1.5, "output": 7.5 }
        }
      }}
    }"#;

    #[test]
    fn the_registry_parser_keeps_only_tool_calling_text_models() {
        let parsed = parse_registry(SAMPLE).unwrap();
        let google: Vec<&str> = parsed[GOOGLE].iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            google,
            vec!["gemini-3.6-flash"],
            "an image-output model and a non-tool-calling model are both unusable as agents"
        );
    }

    #[test]
    fn date_suffixed_aliases_are_dropped_in_favour_of_the_rolling_id() {
        let parsed = parse_registry(SAMPLE).unwrap();
        let ids: Vec<&str> = parsed[ANTHROPIC].iter().map(|m| m.id.as_str()).collect();
        assert!(!ids.iter().any(|i| i.ends_with("20251001")), "got {ids:?}");
    }

    #[test]
    fn models_are_ordered_newest_first_so_the_default_is_current() {
        let parsed = parse_registry(SAMPLE).unwrap();
        assert_eq!(parsed[ANTHROPIC][0].id, "claude-opus-5");
    }

    #[test]
    fn real_pricing_is_carried_through_not_rounded_to_a_tier() {
        let parsed = parse_registry(SAMPLE).unwrap();
        let sonnet = parsed[ANTHROPIC]
            .iter()
            .find(|m| m.id == "claude-sonnet-5")
            .unwrap();
        // The specific number the old hardcoded tier table got wrong.
        assert_eq!(sonnet.input_per_million, 2.0);
        assert_eq!(sonnet.output_per_million, 10.0);
    }

    #[test]
    fn the_live_path_caps_each_provider_like_the_bundled_snapshot_does() {
        // Pins the online/offline consistency: before this, a machine with
        // network saw 37 OpenAI models and one without saw 10.
        let mut models = String::new();
        for i in 0..25 {
            if i > 0 {
                models.push(',');
            }
            models.push_str(&format!(
                r#""m{i}": {{ "name": "M{i}", "tool_call": true, "release_date": "2026-01-{:02}",
                   "modalities": {{ "output": ["text"] }},
                   "limit": {{ "context": 1000 }}, "cost": {{ "input": 1, "output": 2 }} }}"#,
                i + 1
            ));
        }
        let raw = format!(r#"{{ "openai": {{ "models": {{ {models} }} }} }}"#);
        let parsed = parse_registry(&raw).unwrap();
        assert_eq!(parsed[OPENAI].len(), PER_PROVIDER_LIMIT);
    }

    #[test]
    fn a_garbage_registry_response_is_an_error_not_an_empty_catalog() {
        // Silently emptying the catalog would make every model vanish from the
        // picker; failing keeps the previous snapshot in place.
        assert!(parse_registry("{\"anthropic\":{}}").is_err());
        assert!(parse_registry("not json").is_err());
    }

    #[test]
    fn a_cache_from_an_older_schema_is_discarded_rather_than_trusted() {
        // The live-parse cap alone was not enough: a cache written before the
        // cap existed kept being served verbatim, so a running Core still
        // offered 37 OpenAI models after the fix. Caught by re-running the real
        // binary, not by the parser's own test.
        let stale = serde_json::json!({
            "schema_version": 0,
            "fetched_at_secs": 99_999_999_999u64,
            "providers": { "openai": [] }
        })
        .to_string();
        let parsed: CachedCatalog = serde_json::from_str(&stale).unwrap();
        assert_ne!(
            parsed.schema_version, CACHE_SCHEMA_VERSION,
            "fixture must actually represent an older schema"
        );
    }

    #[test]
    fn the_bundled_snapshot_covers_every_provider_and_is_priced() {
        for key in [ANTHROPIC, OPENAI, GOOGLE] {
            let models = bundled_map();
            let list = models.get(key).unwrap_or_else(|| panic!("no {key} models"));
            assert!(!list.is_empty(), "{key} snapshot is empty");
            for m in list {
                assert!(m.context > 0, "{}/{} has no context window", key, m.id);
                assert!(
                    m.input_per_million > 0.0 && m.output_per_million > 0.0,
                    "{}/{} is unpriced, which would silently defeat spend caps",
                    key,
                    m.id
                );
            }
        }
    }
}
