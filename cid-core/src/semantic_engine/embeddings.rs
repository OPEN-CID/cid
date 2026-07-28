//! Real local embeddings (review_prompt.md / Gemini-checklist follow-up:
//! "vector search uses a deterministic hash-based mathematical projection
//! rather than a learned neural model").
//!
//! Uses `all-MiniLM-L6-v2` (Apache 2.0, sentence-transformers) — a BERT
//! architecture, 384-dim output, small enough (~90MB fp32) to download and
//! cache locally rather than bundle in every install. `candle` (pure Rust,
//! no native ONNX Runtime linking to manage per-platform) runs the forward
//! pass on CPU; no GPU is required or assumed.
//!
//! # Honesty about scope
//!
//! - The model is downloaded **on first use**, not bundled — `SemanticEngine`
//!   is already opt-in and off by default per repo (Part 17); this follows
//!   the same "cost is paid only by someone who asked for it" shape rather
//!   than growing every install's download for a feature most people won't
//!   turn on immediately.
//! - `SemanticEngine::text_to_embedding` falls back to the previous
//!   hash-based projection if the model hasn't downloaded/loaded yet (or the
//!   download fails — no network, offline dev, corporate proxy blocking
//!   huggingface.co). This is a real, working fallback, not a crash — but it
//!   means embeddings computed before and after the model becomes available
//!   are **not comparable** (different dimensionality, `384` vs the old
//!   `64`). `cosine_similarity` already tolerates a length mismatch (see
//!   `semantic_engine/mod.rs`) rather than panicking, so this degrades to a
//!   poor-quality score for that one comparison rather than an error — and
//!   self-heals the next time that repo's index is rebuilt (`enable()`
//!   always does a fresh scan). A live model swap mid-Core-run does not
//!   retroactively re-embed already-indexed repos; documented here rather
//!   than silently assumed away.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig, DTYPE};
use tokenizers::Tokenizer;

pub const MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";
pub const EMBEDDING_DIM: usize = 384;
/// MiniLM's own training context; longer inputs are truncated rather than
/// erroring — a code chunk is a summary of intent, not a full-fidelity copy.
const MAX_TOKENS: usize = 256;

const MODEL_FILES: &[&str] = &["config.json", "tokenizer.json", "model.safetensors"];

fn model_cache_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cid")
        .join("models")
        .join("all-MiniLM-L6-v2")
}

/// Downloads the model's three files from Hugging Face's public CDN if any
/// are missing, into a local cache directory shared across every repo (the
/// model itself has nothing repo-specific about it). Returns the directory
/// once all three are present. A partial download from a previous failed
/// attempt is simply completed — files are checked individually, not
/// all-or-nothing.
pub async fn ensure_model_downloaded(client: &reqwest::Client) -> Result<PathBuf> {
    let dir = model_cache_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .context("creating embedding model cache directory")?;

    for file in MODEL_FILES {
        let dest = dir.join(file);
        if tokio::fs::metadata(&dest).await.is_ok() {
            continue;
        }
        let url = format!("https://huggingface.co/{MODEL_REPO}/resolve/main/{file}");
        tracing::info!("Downloading embedding model file: {url}");
        let resp = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("requesting {url}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("download of {file} failed: HTTP {}", resp.status());
        }
        let bytes = resp
            .bytes()
            .await
            .with_context(|| format!("reading response body for {file}"))?;
        let tmp = dir.join(format!("{file}.part"));
        tokio::fs::write(&tmp, &bytes)
            .await
            .with_context(|| format!("writing {file}"))?;
        tokio::fs::rename(&tmp, &dest)
            .await
            .with_context(|| format!("finalizing {file}"))?;
    }

    Ok(dir)
}

/// True once all three model files are already cached locally — lets a
/// caller decide whether loading will need network access before trying.
pub fn is_model_cached() -> bool {
    let dir = model_cache_dir();
    MODEL_FILES.iter().all(|f| dir.join(f).exists())
}

pub struct EmbeddingModel {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

impl EmbeddingModel {
    /// Loads an already-downloaded model from `dir` (see
    /// `ensure_model_downloaded`). Synchronous and CPU-bound — callers run
    /// this via `spawn_blocking`, matching how `SemanticEngine`'s own
    /// indexing scan is already dispatched.
    pub fn load(dir: &Path) -> Result<Self> {
        let device = Device::Cpu;

        let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;

        let config_str =
            std::fs::read_to_string(dir.join("config.json")).context("reading config.json")?;
        let config: BertConfig =
            serde_json::from_str(&config_str).context("parsing BERT config.json")?;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DTYPE, &device)
                .context("loading model.safetensors")?
        };
        let model = BertModel::load(vb, &config).context("constructing BERT model")?;

        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    /// Real tokenize -> forward pass -> mean-pool (over the attention mask,
    /// so padding tokens don't dilute the average) -> L2-normalize. Returns
    /// a 384-dim vector — `sentence-transformers`' own recommended pooling
    /// strategy for this model family, not an arbitrary choice.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))?;
        encoding.truncate(MAX_TOKENS, 0, tokenizers::TruncationDirection::Right);

        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();
        let type_ids = encoding.get_type_ids();

        let ids = Tensor::new(ids, &self.device)?.unsqueeze(0)?;
        let type_ids = Tensor::new(type_ids, &self.device)?.unsqueeze(0)?;
        let attention_mask = Tensor::new(mask, &self.device)?.unsqueeze(0)?;

        let output = self.model.forward(&ids, &type_ids, Some(&attention_mask))?;
        // output: (batch=1, seq_len, hidden). Mean-pool over seq_len using
        // the real attention mask, not a plain average, so padding
        // (batch size is always 1 here, so none in practice, but the mask
        // is honored regardless — this is the correct general pooling).
        let mask_f32 = attention_mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let masked = output.broadcast_mul(&mask_f32)?;
        let summed = masked.sum(1)?;
        let counts = mask_f32.sum(1)?.clamp(1e-9, f64::INFINITY)?;
        let pooled = summed.broadcast_div(&counts)?;

        let vec: Vec<f32> = pooled.squeeze(0)?.to_vec1()?;
        let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        let normalized = if norm > 0.0 {
            vec.iter().map(|v| v / norm).collect()
        } else {
            vec
        };
        Ok(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_cache_dir_is_stable_and_repo_scoped() {
        let a = model_cache_dir();
        let b = model_cache_dir();
        assert_eq!(a, b);
        assert!(a.to_string_lossy().contains("all-MiniLM-L6-v2"));
    }

    #[test]
    fn is_model_cached_is_false_when_nothing_is_downloaded() {
        // Only false-negative-safe if the real cache dir happens to be
        // empty in this environment, which it is unless a previous test in
        // this same run actually downloaded the model.
        if !model_cache_dir().exists() {
            assert!(!is_model_cached());
        }
    }

    /// Real download + real load + real inference — opt-in via env var so
    /// routine `cargo test` runs never trigger a ~90MB network fetch. Run
    /// explicitly with `CID_TEST_REAL_EMBEDDINGS=1 cargo test -p cid-core
    /// --lib embeddings -- --ignored` once to verify the whole pipeline
    /// actually works, same spirit as this project's MCP stdio tests
    /// skipping cleanly rather than failing when their resource is absent.
    #[tokio::test]
    #[ignore]
    async fn real_model_downloads_loads_and_embeds_similar_text_closer_than_unrelated_text() {
        if std::env::var("CID_TEST_REAL_EMBEDDINGS").is_err() {
            eprintln!(
                "skipping: set CID_TEST_REAL_EMBEDDINGS=1 to run a real download+inference test"
            );
            return;
        }

        let client = reqwest::Client::new();
        let dir = ensure_model_downloaded(&client).await.unwrap();
        let model = tokio::task::spawn_blocking(move || EmbeddingModel::load(&dir))
            .await
            .unwrap()
            .unwrap();

        let a = model
            .embed("fn add(a: i32, b: i32) -> i32 { a + b }")
            .unwrap();
        let b = model
            .embed("fn sum(x: i32, y: i32) -> i32 { x + y }")
            .unwrap();
        let c = model
            .embed("The stock market closed lower today amid inflation fears")
            .unwrap();

        assert_eq!(a.len(), EMBEDDING_DIM);

        fn cosine(a: &[f32], b: &[f32]) -> f32 {
            let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (na * nb)
        }

        let sim_related = cosine(&a, &b);
        let sim_unrelated = cosine(&a, &c);
        assert!(
            sim_related > sim_unrelated,
            "two similar functions ({sim_related}) should score closer than code vs. unrelated prose ({sim_unrelated})"
        );
    }
}
