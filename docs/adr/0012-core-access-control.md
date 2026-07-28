# ADR 0012 — Access control for a remotely-reachable Core

**Status:** Accepted
**Context:** Phase 2, Part 15 (Cross-Platform Architecture), Part 14 (Security)

## Context

Through Phase 1, Core bound to `127.0.0.1` and the OS was the access boundary. The Phase 2
Web Shell makes "Core running somewhere a browser can reach" a supported deployment, and
the Phase 2 prompt asks for "basic access control" if Core is reachable beyond localhost.

The RPC surface is not low-stakes. It can read and write arbitrary files, run terminal
commands, create git worktrees, and reach stored model credentials. An unauthenticated
Core on `0.0.0.0` is a remote shell for anyone on the network.

The Web Shell already had an "Access Control" panel, but it was local React state — the
toggles wrote to nothing and enforced nothing. CORS was `allow_origin(Any)`, so any web
page the user visited could drive the RPC surface from their browser.

## Decision

**A shared bearer token, checked in Core, mandatory for non-loopback binds.**

- `AccessPolicy::new(bind_ip, token, origins)` **fails at startup** if the bind address is
  not loopback and no token was supplied. Core refuses to run rather than starting in an
  unsafe configuration — a startup error is far more visible than a warning in a log.
- The token is supplied by `--auth-token` or `CID_AUTH_TOKEN`. `--generate-token` prints a
  40-character random token.
- `/api/rpc` and the `/ws` upgrade both require `Authorization: Bearer <token>` when a
  token is configured. The WebSocket is authorized *before* the upgrade, since an open
  socket carries the same authority as the HTTP surface.
- Token comparison is constant-time.
- `/health` stays unauthenticated and reports only reachability, version, client count,
  and whether auth is required — nothing not already implied by the port being open. This
  is what lets the Web Shell tell the user their Core is exposed.
- CORS moved from `Any` to an explicit origin allow-list, defaulting to the local desktop
  and web shell origins, extensible with `--allow-origin`.

## Alternatives considered

- **mTLS.** Stronger, but requires certificate distribution for a feature whose main user
  is one developer reaching their own machine. Not proportionate at this phase.
- **OS user accounts / OIDC.** Phase 3 introduces local accounts for *multi-user
  Workspace membership*. That is a different problem — who you are within a Workspace —
  and layers on top of this rather than replacing it. Transport authorization still needs
  to exist first.
- **Leaving it open and documenting "don't expose it".** Rejected. The Web Shell exists
  specifically so Core can be reached from elsewhere; shipping the capability with a
  documentation-only mitigation is how this goes wrong in practice.

## Consequences

- The single token is shared by everyone who can reach that Core. It authenticates the
  *connection*, not a person. Audit entries attribute actions to the Mission, not to a
  human, until Phase 3's account model lands.
- There is no token rotation without a restart.
- Traffic is plain HTTP unless a TLS-terminating reverse proxy is placed in front. The
  token is therefore only as private as the network path. `SECURITY.md` says so.
- Existing local setups are unaffected: loopback with no token behaves exactly as before.
