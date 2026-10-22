# 055 — One-Stop Platform Master Plan

**Written:** 2026-10-17, from a full pass over every part (36 RPC groups,
14 UI tabs, 4 shells). **Audience:** whoever picks the next work, human or
agent. **Method:** each claim below names the file or RPC that proves it —
no aspirational entries; see `041-Roadmap.md` §Failure Modes for why.

**Status, end of day:** W1–W3 are BUILT (see below), verified by the full
gates. Remaining: W4 (needs a design first), W5 (needs usage evidence),
native mobile builds (needs SDKs), cloud integrations (permanently out).

**Standing boundaries (not re-litigated here; see 041 §v1.0 scope):**
deploy-provider orchestration ("Deploy to X"), hosted CID Cloud, native
rendering engine, enterprise/air-gapped hardening. Anything below that
collides with those is marked REJECT with the reason, not silently dropped.

## Part-by-part state

| Part | Backend | UI surface | Verdict |
|---|---|---|---|
| Chat thread, plans, approvals | Complete (incl. provenance, compaction) | Complete | MAINTAIN |
| Model catalog + routing | Complete (auto modes, recommend) | Mode selector + badge + preview | MAINTAIN |
| Session model override | Complete | Creation picker + thread switcher | MAINTAIN |
| Workflows | Complete (defs, runs, retry, crash recovery, from-plan) | Panel + plan bridge | EXTEND: schedules, see backlog |
| Reviews (PRs + releases) | `github.pr.*`, `forge.change_request.*`, `deployment.*` complete | List/status/release log | EXTEND: open-PR form (backlog W2) |
| Agents (subagents, bg tasks) | Complete; `get`/`cancel` client shape fixed | Live view + sandbox | MAINTAIN |
| Terminal | PTY complete + reconnect affordance | Complete | MAINTAIN |
| Editor | Tabs, dirty tracking, binary guard | Complete | EXTEND: small items, see backlog |
| Diff | Per-hunk accept/reject real | Complete | MAINTAIN |
| MCP client | stdio transport real; sampling/elicitation absent (SEP-2322, low priority) | Add/remove/test; custom UI | EXTEND: @mention invocation (backlog W1) |
| Skills (incl. AGENTS.md gate) | Complete | Bundle CRUD + approval card | MAINTAIN |
| Search (ripgrep engine) | Complete | Results via editor | MAINTAIN |
| Semantic engine | Complete, opt-in per repo | Health panel tabs | MAINTAIN |
| Context compaction + usage | Complete | Indicator + `/compact` | MAINTAIN |
| Checkpoints | Complete | Cards + rewind | MAINTAIN |
| Governance (caps, roles, allow-lists) | `policy.set` requires admin session; **no UI supplies one** | Allow-lists only | **GAP — backlog W3** |
| Auth (multi-user) | Complete (Argon2id, roles, rotation) | Token box only; **no login/register UI** | **GAP — backlog W3** |
| Local models | Detection, fit classes, manager | Start/stop/pull, catalog | MAINTAIN |
| Health/observability | `/metrics`, crash log | Server tab | MAINTAIN |
| Trackers (Jira/Linear) | Link + credential verify | Via issues panels? partial | EXTEND: link UI check (backlog) |
| Slack/Teams bridges | Complete | Configure panels | MAINTAIN |
| Mobile shell | Approval/monitoring + PWA | `?mobile=1` | MAINTAIN (native = separate project) |
| TUI | Chat/status/approvals/diff | Complete | MAINTAIN |
| Desktop (Tauri) | CI check per OS | Installers via release workflow | MAINTAIN |

## Backlog (ordered; each item is build→test→verify, no stubs)

### W1 — @mention MCP tools in chat (advertised, unimplemented)
The composer placeholder promises "@mention an MCP tool". Nothing parses
`@`. Build: `@`-triggered autocomplete over `mcp.server.list` +
`mcp.tools.list`; on send, resolve each `@server.tool` (+ optional trailing
JSON args) via `mcp.tool.call` client-side and append results to the outgoing
message. Pure UI + existing RPCs. Tests: dropdown filters, extraction unit
tests, arg passing, failure transparency.

### W2 — Open-PR affordance (golden path says "open a PR", no button)
`github.pr.create` has no caller. Build: form in Reviews tab (title, head,
base from the session's branch) → create → link. Tests: RPC wired with
correct shape, validation errors surfaced.

### W3 — Login + governance wiring (admin RPCs unreachable from any UI)
`auth.register/login/logout/status` have zero clients; `governance.policy.set`
requires an admin `session_token` nothing supplies. Build: sign-in dialog
(stores session token, shows current user/role, logout); wire the token into a
Governance section (autonomous enable + repos, spend caps) added to the
Autonomy area. Tests: login/logout flow, token attached to policy.set,
refusal without token surfaced. Deliberately NOT a permissions overhaul —
just the missing bridge.

### W4 — Workflow schedules
`setInterval`-style recurrence ("nightly checks") needs a persisted schedule
+ a Core-side timer + missed-run semantics. Real but new machinery; do after
W1–W3 prove the panel patterns. Design first, then build.

### W5 — Editor small wins (only if real use demands)
Find/replace bar, minimap toggle, word wrap toggle. Each is a day, not a
project — but only with evidence anyone misses them (no speculative UI).

### Explicitly not building
- **Third-party CLI control plane** (drive `claude`/`opencode`/etc. from CID):
  shelling out to another agent from inside an agent loop is a loop, not an
  integration; credential exfiltration surface. What IS built instead: CLI
  *detection* + endpoint prefill (never secret import) — separate small item.
- **Vercel/AWS/GCP/Azure orchestration or EC2 management**: 041 permanent
  non-goal. The deployment *record* stays the interface; `deployment.webhook`
  stays the CI ingress.
- **Native mobile apps**: separate project (SDKs, signing, stores). PWA +
  mobile shell is the shipped answer.

## Acceptance bar (applies to every item above)
Full gates green (`cargo test`, fmt, clippy `-D warnings`, tsc, lint,
vitest), no orphan RPCs either direction (`comm` check both ways, including
`api.ts` call shapes), no new `blocked_on_approval`-style dead ends without
a visible path forward, and every behavior verified against a running Core —
never "should work".
