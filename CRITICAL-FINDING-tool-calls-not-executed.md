# RESOLVED — real provider tool calls are parsed but never executed

**Found:** while implementing review_prompt.md §3.2 (checkpoint/rewind), not part of the
original review.

**Fixed:** all four provider call sites (`call_anthropic_with_tools`,
`call_openai_with_tools`, `call_openai_compatible_with_tools`,
`call_google_with_tools` in `cid-core/src/model/mod.rs`) now run a real
multi-round tool-execution loop: parse the requested tool call(s) off the
stream, call the already-correct `execute_tool_with_approval` (autonomy gate +
Co-Pilot approval wait, unchanged), persist a `ToolCall` record on the turn's
message, feed the result back as the provider's own follow-up-turn format
(Anthropic `tool_result` blocks, OpenAI `tool` messages, Gemini
`functionResponse` parts), and loop (capped at 25 rounds) until the model
stops requesting tools. Every provider's endpoint is now an injectable
parameter (`chat_url`/`api_base`), which is what made this verifiable without
a live key — `cid-core/src/model/mod.rs`'s `tool_execution_tests` module runs
a real two-round exchange against a local mock server for all four providers
and asserts the tool call actually touched a real file on disk, not a stub.
`cargo test --workspace` (465 tests) and `cargo clippy -D warnings` both pass
clean with this change. The analysis below is kept for the historical record
of how this was found and why it wasn't fixed in the same pass it was
discovered in.

---

**Original status when discovered:** not yet fixed — documented here instead of attempted
as a rushed, unverifiable change, since it touches the primary provider's core streaming
logic and there is no live Anthropic API key in this environment to test a rewrite
against. (This blocker was resolved by making the endpoint injectable — see above — rather
than by obtaining a live key.)

## The bug

`cid-core/src/model/mod.rs`:

- `call_anthropic_with_tools` (~line 1965): accumulates parsed `tool_use` content blocks
  into `tool_calls_buffer: HashMap<String, serde_json::Value>` as they stream in
  (`content_block_start` with `content_block.type == "tool_use"`). After the stream ends,
  **`tool_calls_buffer` is never read again.** The function persists only the text content
  and returns.
- `call_openai_with_tools` (~line 2085): the same pattern with `tool_calls_accum:
  HashMap<usize, String>`, built from `delta.tool_calls[].function.arguments` deltas,
  never read after the loop either.
- `execute_tool_with_approval` (~line 2610, the function that actually runs a tool —
  applies the autonomy gate, requests human approval via `session.tool_call.request` for
  Co-Pilot, and calls `execute_tool_direct_in`) **has no callers anywhere in the
  codebase** (`grep -rn "execute_tool_with_approval" cid-core/src/` returns only its own
  definition).

**Net effect: with a real API key configured, the model can request a tool call (read a
file, edit a file, run a command) and CID silently drops the request.** No approval
prompt appears (Co-Pilot's whole selling point — "every tool call is shown and requires
approval" — never fires for a real model). No file is read or written. No command runs.
The Session just ends with whatever text the model streamed before or around the
tool-call request, and nothing else happens. This was not caught anywhere else in this
project's history because every E2E/manual verification pass this session (and, from the
checkpoint history, prior sessions too) ran without a configured API key, which routes
through the entirely separate "simulated response" branch in `process_message_with_role`
— a scripted string, not a real model call — so the real streaming-and-tool-execution
path has apparently never been exercised end-to-end with genuine credentials.

This is a more fundamental gap than anything in `review_prompt.md`: the agent loop's
actual reason for existing — reading and editing code, running commands, with approval —
does not function against a real provider today.

## Why this wasn't fixed in this pass

1. It requires rewriting the primary provider's (Anthropic) streaming parser to also
   accumulate each tool_use block's incrementally-streamed `input` JSON
   (`input_json_delta` events, keyed by content-block `index` — not currently parsed at
   all) and then implement the actual multi-turn loop: execute each tool via the
   already-correct `execute_tool_with_approval` (which already handles the autonomy gate
   and Co-Pilot human-approval wait — it just needs to be called), build a `tool_result`
   follow-up message, and re-call the API until the model stops requesting tools.
2. `call_anthropic_with_tools`'s endpoint URL is hardcoded
   (`"https://api.anthropic.com/v1/messages"`), so — unlike the OpenAI-compatible route
   used for this session's spend-tracking and MCP tests — it cannot be pointed at a local
   mock server without also changing the function's signature. Verifying a rewrite here
   needs either a live Anthropic key or a deliberate refactor to make the endpoint
   injectable purely for testing.
3. Given the size of the already-completed work in this session and the real risk of
   shipping an unverified rewrite of core model-loop logic, the more responsible choice
   was to document this precisely rather than guess at a fix with no way to confirm it
   actually works against the real API's exact wire format.

## Suggested fix approach for the follow-up that picks this up

1. Extend the Anthropic SSE parser to track tool_use blocks by content-block `index`
   (not just `id`), accumulating `input_json_delta.partial_json` per index, and
   finalizing/parsing each block's JSON at `content_block_stop`.
2. After a turn's stream ends: if any tool_use blocks were parsed, for each one (in
   order), call `self.execute_tool_with_approval(session_id, &name, input,
   app_state.clone()).await` — this already implements the autonomy gate and the
   Co-Pilot approval wait correctly; it just needs to be invoked.
3. Persist the assistant's turn (text + `ToolCall` entries with results) via
   `create_message` with `tool_calls: Vec<ToolCall>` populated — `ChatMessage` and
   `ToolCall` already support this shape.
4. Build the next request's message list: the assistant's own turn (replayed with its
   original content blocks, including `tool_use`) followed by a `user` message
   containing one `tool_result` block per tool call, referencing `tool_use_id`.
5. Loop (with a hard cap — e.g. 25 rounds — to prevent a runaway loop) until a turn
   produces no tool_use blocks, then finalize as today.
6. Apply the same pattern to `call_openai_with_tools` (OpenAI's `tool_calls[].function`
   shape, `finish_reason: "tool_calls"` signals another round is needed) — this one
   *can* be verified with the existing local-mock-server test pattern
   (`stdio_transport_tests`/`spend_tracking_tests` in this codebase already establish it)
   since its `chat_url` is already a parameter, unlike Anthropic's.
7. `call_google_with_tools` and `call_openai_compatible_with_tools` need the same
   treatment; audit each for the same "parsed but never executed" shape before assuming
   either is fine.
8. Write an integration test using a local mock server (Node, per this codebase's
   existing pattern — see `cid-core/src/mcp/mod.rs`'s `stdio_transport_tests`) that
   returns a tool_use/tool_calls response on the first request and a plain-text response
   on the second, then asserts the tool was actually invoked (e.g. a file was actually
   written) and the follow-up request was actually sent — this is the test that would
   have caught the original bug.

## How this was found

While tracing where `process_message_with_role` and its provider-call functions fit
together to add an auto-checkpoint hook (review_prompt.md §3.2), a call site search for
`execute_tool_with_approval` — the function that actually executes a tool and emits the
`session.tool_call.request`/`.complete` notifications the frontend listens for — returned
zero callers. Tracing backward from there to how tool calls are parsed out of each
provider's streaming response confirmed the buffers are built and then discarded in both
providers checked (Anthropic, OpenAI).
