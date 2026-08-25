# ADR 0009: Multi-Provider Model Routing (Phase 1)

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: Phase 0 implemented Anthropic-only routing in `cid-core/src/model/mod.rs` with simulated fallback. Phase 1 per Build Prompt Part 22 / Appendix A Part 22 requires: OpenAI and Google as foreground providers alongside Anthropic, plus one generic OpenAI-compatible endpoint slot (covers OpenRouter, Groq, Bedrock-compatible proxies, vLLM, self-hosted like Ollama, LM Studio, llama.cpp). Swappable mid-Session (settings re-read each message) and selectable per role (Planner/Implementer/Reviewer can each use different provider/model). Settings already extended in `cid-core/src/api/types.rs` with fields: openai_api_key, openai_model, google_api_key, google_model, openai_compatible_endpoint, openai_compatible_api_key, openai_compatible_model, planner_provider/model, implementer_provider/model, reviewer_provider/model, github_token.

- **Decision**:

  - **Persistence Migration**: `cid-core/src/persistence/mod.rs`
    - Updated `settings` table CREATE to include all new columns (openai_api_key, openai_model, google_api_key, google_model, openai_compatible_endpoint, openai_compatible_api_key, openai_compatible_model, planner_provider/model, implementer_provider/model, reviewer_provider/model, github_token)
    - Added idempotent ALTER TABLE ADD COLUMN migrations for existing DBs (ignore error if column exists)
    - Rewrote `get_settings()` to try full-column SELECT first, fallback to 4-column legacy SELECT, returning Settings with all fields.
    - Rewrote `update_settings()` to update all 18 columns.

  - **ModelManager Refactor**: `cid-core/src/model/mod.rs` completely rewritten (>1500 lines) to support multi-provider:

    - **ResolvedModelConfig** struct: provider (ModelProvider enum), model_id, api_key (Option), endpoint (Option)
    - **Provider Parsing**: `parse_provider_str()` handles aliases: anthropic/claude, openai/gpt, google/gemini/genai, openai_compatible/openrouter/groq/together/vllm, ollama, lmstudio variants, llamacpp variants.

    - **Key & Endpoint Resolution**:
      - `provider_api_key()` checks Settings field then env vars: ANTHROPIC_API_KEY, OPENAI_API_KEY, GOOGLE_API_KEY/GEMINI_API_KEY/GOOGLE_GENERATIVE_AI_API_KEY, OPENAI_COMPATIBLE_API_KEY/OPENROUTER_API_KEY/GROQ_API_KEY
      - `provider_endpoint()` returns fixed base for Anthropic/OpenAI/Google, or settings.openai_compatible_endpoint for compatible slot, with defaults for local: Ollama http://localhost:11434/v1, LM Studio http://localhost:1234/v1, llama.cpp http://localhost:8080/v1
      - `provider_default_model()` picks settings.*_model or hard default (gpt-4o-mini, gemini-1.5-flash, etc.)

    - **Per-Role Selection**:
      - `resolve_for_role(role, settings)` checks planner_provider/model etc, parses provider string, returns ResolvedModelConfig with that role's model or inferred default.
      - `resolve_active_config(settings, preferred_role)` priority: preferred role override -> Implementer role -> Planner -> Reviewer -> first enabled provider by priority [Anthropic, OpenAI, Google, OpenAICompatible] -> fallback Anthropic default (triggers simulated response if no key).
      - `process_message()` now delegates to `process_message_with_role()` with Implementer as default, but exposed `process_message_with_role(session_id, content, role, app_state)` for future Planner/Implementer/Reviewer orchestration.
      - Swappable mid-Session: settings re-read fresh on every message via `persistence.get_settings()`.

    - **Known Models & list_models()**:
      - Constants: ANTHROPIC_MODELS (sonnet, haiku, opus), OPENAI_MODELS (gpt-4o, mini, turbo, o1, o1-mini), GOOGLE_MODELS (gemini 1.5 pro/flash, 2.0 flash exp, 8b), OPENAI_COMPAT_MODELS (llama-3.1-70b, 8b, mixtral, qwen)
      - `list_models()` reads settings, checks `is_provider_enabled()` (key existence or endpoint configured), returns Vec<Value> with fields id, name, provider (snake_case matching ModelProvider::serialize), context_length, default (true if matches settings model), available (bool based on enablement).
      - For OpenAI-compatible: if endpoint configured, includes configured model plus known compatibles with available=true; else returns 2 placeholder unavailable models for discoverability.
      - For local runtimes: if compatible endpoint set, exposes Ollama/LM Studio/llama.cpp entries as provider ollama, lm_studio, llama_cpp with same model.

    - **Provider Clients** (trait-like enum dispatch, not trait to keep simpler):

      - **Anthropic**: POST https://api.anthropic.com/v1/messages, headers x-api-key, anthropic-version 2023-06-01, body with model, max_tokens 8192, system, messages, tools (read_file, write_file, edit_file, list_files, run_terminal, git_*), stream true. SSE parsing: lines `data: {type: content_block_delta, delta: {text}}`. Emits `session.message.delta` per chunk and `session.message.complete` on finish. Persists assistant placeholder message.

      - **OpenAI**: POST https://api.openai.com/v1/chat/completions, Authorization Bearer, body {model, messages (system+history+user), tools (OpenAI function format), tool_choice auto, stream true, stream_options include_usage, max_tokens 8192, temp 0.7}. Headers include HTTP-Referer https://cid.dev and X-Title CID for OpenRouter compatibility. SSE parsing: data JSON with choices[0].delta.content. Emits same delta/complete notifications.

      - **OpenAI-Compatible**: Generic endpoint resolution via `resolve_chat_url(endpoint)`: if endpoint contains chat/completions use as is, else if ends with /v1 append /chat/completions, else append /v1/chat/completions. Covers OpenRouter https://openrouter.ai/api/v1, Groq https://api.groq.com/openai/v1, Ollama http://localhost:11434/v1, LM Studio http://localhost:1234/v1, vLLM, Bedrock-compatible proxies. Same streaming as OpenAI but Authorization optional (local servers may not need key). Same notifications.

      - **Google (Gemini)**: POST https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?key=API_KEY&alt=sse, body {contents (role user/model), systemInstruction {parts: [{text: system_prompt}]}, generationConfig {maxOutputTokens 8192, temp 0.7, topP 0.9}, tools {functionDeclarations}}. alt=sse makes it SSE similar to OpenAI. Parsing: candidates[].content.parts[].text. Emits delta/complete same as others.

    - **Simulated Fallback**: If provider requires key and none found, or compatible endpoint missing, create assistant message with helpful text listing how to enable each provider, include per-role config hints, mention swappable mid-Session, and emit session.message.new. Returns Ok without calling API.

    - **Error Handling**: If API call fails (non-2xx), bail with status+body, then catch in process_message_with_role, persist error as assistant message, emit new notification, set session status Review, return Ok (don't crash loop). Uses anyhow::Context and tracing::info/warn.

    - **Streaming Best Practices**: reqwest Client built with 300s timeout, 10s connect timeout, user-agent cid-core/1.0. Keeps http_client in ModelManager Arc. Uses bytes_stream() + futures::StreamExt, buffers leftover for split lines, handles data: prefix, [DONE], ignores keep-alive.

    - **Notifications**: Helper functions emit_delta and emit_complete construct JsonRpcNotification with method session.message.delta / complete and params {session_id, message_id, delta/content}. Same as Anthropic path required by task.

  - **Settings Router Enhancement**: `cid-core/src/api/router.rs`
    - handle_settings_get now tries keyring for all provider keys (anthropic, openai, google, compatible) with migration path, redacts all keys via redact_key helper, returns has_* flags for each provider.
    - handle_settings_update now handles redacted keys (contains ...) by keeping existing, stores real keys in OS keyring via keyring::Entry com.cid.dev/{provider}_key, preserves existing if incoming None/empty. Supports github_token as well.

  - **Local Runtime Detection** (already existed but fixed): `cid-core/src/local_models/mod.rs` bug where Response::json consumed twice fixed by reading text once and parsing via serde_json::from_str twice. Cargo check now passes.

  - **Cargo.toml**: tree-sitter-go version fixed from 0.24 (non-existent) to 0.23 to align with Cargo.lock 0.23.4; tree-sitter updated to 0.25 to match lock; ensures `cargo check -p cid-core` passes.

- **Alternatives Considered**:

  - Trait-based ProviderClient with async trait streaming: would be cleaner but requires async_trait and boxing, increased complexity for Phase 1; enum dispatch simpler, still allows swappable.
  - Unified OpenAI client for all OpenAI-compatible including Anthropic via proxy: rejected because Anthropic API shape differs significantly (x-api-key header, system separate, tool_use blocks).
  - Caching settings in memory: rejected because requirement says swappable mid-Session, must re-read each call.
  - Only returning enabled providers in list_models: considered but we return all with available flag for discoverability; UI can show disabled with reason.
  - Using `tiktoken` for token counting: deferred to Phase 1 metrics tab, not needed for routing.

- **Consequences**:

  - Phase 1 multi-provider routing complete: Planner/Implementer/Reviewer can each have different provider/model via Settings (planner_provider/model, etc). Changing Settings mid-Session affects next message.
  - One generic OpenAI-compatible slot satisfies OpenRouter, Groq, Bedrock-compatible proxies, vLLM, Ollama (http://localhost:11434/v1), LM Studio (http://localhost:1234/v1), llama.cpp (http://localhost:8080/v1).
  - list_models now returns models from all enabled providers based on API key existence or endpoint configured, with available flag.
  - Streaming for each provider emits same notifications as Anthropic path (delta, complete) so frontend doesn't need provider-specific handling.
  - Uses latest best practices: reqwest streaming, anyhow, tracing, proper error handling, env var fallbacks for CI.
  - `cargo check -p cid-core` passes (verified with msvc toolchain, 11 warnings, 0 errors) and `cargo test -p cid-core` passes 31 tests.
  - Security: keys still stored in keyring via new multi-provider handlers, redacted in GET; plaintext fallback in SQLite remains for dev but documented as Phase 2 improvement (already noted in ADR 0008).
  - Future work: full tool_calls parsing for OpenAI/Google (currently text streaming only, tool definitions sent), per-role orchestration in agent loop (currently defaults to Implementer but with_role API ready), token/cost metrics.

- **References**: Build Prompt Appendix A Part 22 (Phase 1 scope), types.rs ModelProvider enum, model/mod.rs Phase 0 implementation, persistence/mod.rs settings migration, https://platform.openai.com/docs/api-reference/chat, https://docs.anthropic.com/en/api/messages-streaming, https://ai.google.dev/api/generate-content#method:-models.streamGenerateContent, https://openrouter.ai/docs, https://github.com/ollama/ollama/blob/main/docs/openai.md
