//! Which local models are worth offering on *this* machine.
//!
//! Deliberately a small, curated list rather than Ollama's full library. The
//! point of this screen is "what can I actually run, and is it any good at
//! code" — a 300-entry list sorted by popularity answers neither. Every entry
//! is a code-capable instruct model available from Ollama under a licence that
//! permits local use.
//!
//! `min_memory_mb` is the practical floor for the listed quantisation, not the
//! file size: weights must be resident *and* leave room for the KV cache, which
//! grows with context. Sizing off the download size is the usual way these
//! recommendations end up wrong.

use serde::{Deserialize, Serialize};

use super::system::SystemCapability;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Runs with headroom to spare.
    Comfortable,
    /// Will load, but close enough to the limit that a long context or another
    /// heavy app can push it into swap.
    Tight,
    /// Not enough memory. Offered only so the user can see why it is excluded.
    TooLarge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelOption {
    /// The tag to pull, exactly as `ollama pull` expects it.
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static str,
    /// Approximate download size, for the progress the user is about to sit through.
    pub download_mb: u64,
    pub min_memory_mb: u64,
    pub context_tokens: u32,
    pub notes: &'static str,
}

/// Ordered smallest-first so the default recommendation on a modest machine is
/// the one most likely to work.
pub const LOCAL_MODELS: &[LocalModelOption] = &[
    LocalModelOption {
        id: "qwen2.5-coder:1.5b",
        name: "Qwen2.5 Coder 1.5B",
        parameters: "1.5B",
        download_mb: 986,
        min_memory_mb: 2_048,
        context_tokens: 32_768,
        notes: "Smallest useful coding model — fits almost anywhere.",
    },
    LocalModelOption {
        id: "qwen2.5-coder:7b",
        name: "Qwen2.5 Coder 7B",
        parameters: "7B",
        download_mb: 4_700,
        min_memory_mb: 8_192,
        context_tokens: 32_768,
        notes: "The usual sweet spot for local coding work.",
    },
    LocalModelOption {
        id: "deepseek-coder-v2:16b",
        name: "DeepSeek Coder V2 16B",
        parameters: "16B (MoE)",
        download_mb: 8_900,
        min_memory_mb: 12_288,
        context_tokens: 32_768,
        notes: "Mixture-of-experts: stronger than its memory cost suggests.",
    },
    LocalModelOption {
        id: "qwen2.5-coder:32b",
        name: "Qwen2.5 Coder 32B",
        parameters: "32B",
        download_mb: 19_900,
        min_memory_mb: 24_576,
        context_tokens: 32_768,
        notes: "Closest local models get to a hosted frontier model on code.",
    },
    LocalModelOption {
        id: "llama3.1:70b",
        name: "Llama 3.1 70B",
        parameters: "70B",
        download_mb: 39_600,
        min_memory_mb: 45_056,
        context_tokens: 131_072,
        notes: "Workstation-class. Needs a large GPU or a lot of RAM.",
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct RecommendedModel {
    #[serde(flatten)]
    pub option: LocalModelOption,
    pub fit: Fit,
    /// True when this is the largest model that still fits comfortably — the
    /// one to preselect.
    pub recommended: bool,
}

/// Classify every catalogue entry against real measured capacity.
pub fn recommendations(system: &SystemCapability) -> Vec<RecommendedModel> {
    let budget = system.usable_model_memory_mb();

    let mut out: Vec<RecommendedModel> = LOCAL_MODELS
        .iter()
        .map(|option| {
            let fit = if budget >= option.min_memory_mb * 3 / 2 {
                Fit::Comfortable
            } else if budget >= option.min_memory_mb {
                Fit::Tight
            } else {
                Fit::TooLarge
            };
            RecommendedModel {
                option: option.clone(),
                fit,
                recommended: false,
            }
        })
        .collect();

    // Largest comfortable option wins; if nothing is comfortable, the largest
    // that merely fits. Never recommend something that cannot load.
    let pick = out
        .iter()
        .rposition(|m| m.fit == Fit::Comfortable)
        .or_else(|| out.iter().rposition(|m| m.fit == Fit::Tight));
    if let Some(i) = pick {
        out[i].recommended = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(ram_mb: u64, vram_mb: Option<u64>) -> SystemCapability {
        SystemCapability {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu_cores: 8,
            total_ram_mb: ram_mb,
            available_ram_mb: ram_mb / 2,
            gpus: vec![],
            total_vram_mb: vram_mb,
        }
    }

    #[test]
    fn a_small_laptop_is_not_offered_a_seventy_billion_parameter_model() {
        let recs = recommendations(&machine(8 * 1024, None));
        let big = recs.iter().find(|r| r.option.id == "llama3.1:70b").unwrap();
        assert_eq!(big.fit, Fit::TooLarge);
        assert!(!big.recommended);
    }

    /// "Comfortable" means 1.5x the floor, so a 64 GB machine (60 GB budget)
    /// deliberately does *not* get the 70B — that one needs ~44 GB and would be
    /// merely Tight. Recommending it would be the exact failure this sizing
    /// exists to prevent.
    #[test]
    fn the_recommendation_is_the_largest_model_that_fits_comfortably() {
        let recs = recommendations(&machine(64 * 1024, None));
        let picked = recs.iter().find(|r| r.recommended).unwrap();
        assert_eq!(picked.option.id, "qwen2.5-coder:32b");
        assert_eq!(picked.fit, Fit::Comfortable);

        let seventy = recs.iter().find(|r| r.option.id == "llama3.1:70b").unwrap();
        assert_eq!(seventy.fit, Fit::Tight);
        assert!(!seventy.recommended);
    }

    /// With nothing comfortable, the largest that merely fits is still better
    /// than recommending nothing at all.
    #[test]
    fn a_machine_with_only_tight_options_still_gets_a_recommendation() {
        // 6 GB budget: the 1.5B is comfortable, the 7B needs 8 GB — too large.
        let recs = recommendations(&machine(10 * 1024, None));
        let picked = recs.iter().find(|r| r.recommended).unwrap();
        assert_eq!(picked.option.id, "qwen2.5-coder:1.5b");
    }

    #[test]
    fn exactly_one_model_is_ever_recommended() {
        for ram in [4, 8, 16, 32, 64, 128] {
            let recs = recommendations(&machine(ram * 1024, None));
            let n = recs.iter().filter(|r| r.recommended).count();
            assert!(n <= 1, "{ram}GB machine recommended {n} models");
        }
    }

    #[test]
    fn a_big_gpu_unlocks_models_the_system_ram_alone_would_not() {
        let cpu_only = recommendations(&machine(16 * 1024, None));
        let with_gpu = recommendations(&machine(16 * 1024, Some(24 * 1024)));

        let id = "qwen2.5-coder:32b";
        let before = cpu_only.iter().find(|r| r.option.id == id).unwrap();
        let after = with_gpu.iter().find(|r| r.option.id == id).unwrap();
        assert_eq!(before.fit, Fit::TooLarge);
        assert_ne!(after.fit, Fit::TooLarge, "24GB of VRAM should fit a 32B");
    }

    /// A machine too small for anything must say so rather than recommending
    /// something that cannot load.
    #[test]
    fn a_machine_below_every_floor_recommends_nothing() {
        let recs = recommendations(&machine(2 * 1024, None));
        assert!(recs.iter().all(|r| !r.recommended));
        assert!(recs.iter().all(|r| r.fit == Fit::TooLarge));
    }
}
