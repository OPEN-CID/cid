# ADR 0013 — Minimal local accounts for multi-user Workspaces

**Status:** Accepted
**Context:** Phase 3, Part 3 (Workspace model), Part 22 (multi-user membership/roles), Part 24

## Context

Part 3, Part 22, and Part 24 all reference multi-user Workspaces with membership and
roles, but no part of the founding brief says how a user authenticates. The Phase 3 prompt
identifies this as a genuine gap rather than a contradiction, and Part 0 rule 4 says to
pick the option that keeps the phase smallest, write it down, and move on.

The constraint that matters: Phases 0–2 never introduced a server component beyond the
local Core (Part 24). Adding cloud account infrastructure — email verification, an identity
provider, password reset flows — would be a larger change than the feature it exists to
support, and would contradict the local-first posture in Part 1.

Note that this is a *different* problem from [ADR 0012](0012-core-access-control.md).
That one answers "may this connection talk to Core at all." This one answers "who is this,
and what are they allowed to do inside the Workspace." Both are needed; neither replaces
the other.

## Decision

**Local account records in Core's SQLite, with Argon2id password hashing and opaque
session tokens. No SSO, no OAuth, no email verification, no password reset.**

- `users` table: id, username (unique, case-insensitive), Argon2id PHC-string hash, role,
  created/updated timestamps, `active` flag.
- Argon2id with the crate's default parameters, per-user random salt, PHC-format storage
  so parameters can be raised later without invalidating existing hashes.
- `auth.register` creates an account. **The first account created becomes `Owner`**; every
  subsequent registration defaults to `Developer`. This avoids a bootstrap problem without
  a setup wizard or a magic default password.
- `auth.login` returns an opaque 48-character random session token with an expiry. Sessions
  live in a `sessions` table so they can be revoked; `auth.logout` deletes one.
- Workspace roles: `Owner`, `Admin`, `Developer`, `Reviewer`, `Viewer`, in that order of
  authority. Permissions are derived from the role, not stored per user, so a role change
  takes effect immediately everywhere.
- Failed logins are rate-limited per username with a short lockout, so the local DB is not
  a free offline brute-force target for someone who can reach the port.

## Alternatives considered

- **OS user identity (whoever runs Core).** Simplest, and correct for single-user local
  use — which is exactly what Phases 0–2 already did implicitly. It cannot express
  membership or roles once more than one person shares a Core, which is the entire point
  of this phase.
- **OIDC / SSO.** What an enterprise deployment will eventually want (Part 15 anticipates
  it). It requires an identity provider, redirect URIs, and token validation — a server
  story this phase deliberately does not open. The role model here is designed so an OIDC
  provider can later populate the same `users` and role tables without changing anything
  above them.
- **No auth; rely on the Phase 2 access token.** The access token is shared by everyone who
  can reach that Core. It cannot attribute an action to a person, which makes the
  governance audit trail this phase builds meaningless.

## Consequences

**What this protects.** Distinct identities, per-user roles, an audit trail that names who
approved a plan or enabled Autonomous mode, and enforcement of governance policy at the
Core boundary rather than in the UI.

**What this does not protect.**
- No password reset. An Owner can reset another user's password; a sole Owner who forgets
  theirs must edit the database.
- No email verification, so usernames are not proof of anything about a person.
- No MFA.
- Sessions are bearer tokens over whatever transport Core is using. Without a TLS proxy,
  a token on the wire is a token anyone on the path can take — same caveat as ADR 0012.
- This is appropriate for a small team on a trusted network. It is **not** an
  internet-facing authentication system, and `SECURITY.md` says so.

**Revisit when** a deployment needs SSO, or when Workspaces span organizations. The table
shape was chosen so an external identity provider can be added as a new authentication
path without disturbing membership, roles, or governance.
