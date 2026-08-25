# ADR 0011 — What the Autonomous-mode sandbox actually guarantees on Windows

**Status:** Accepted
**Context:** Phase 2, Part 14 (Security, Approval & Sandboxing)

## Context

Part 14 specifies that Autonomous-mode terminal commands run "inside a lightweight
sandbox (OS-level: `sandbox-exec` on macOS, a restricted job object on Windows, a
namespaced process on Linux) scoped to the Session's worktree directory, so an
autonomous Session can't touch files outside its own worktree even if a command tries to."

Two of the three named mechanisms deliver that. The Windows one does not.

A **Windows Job Object constrains process lifetime, CPU, memory, and process-creation
limits. It does not restrict filesystem access at all.** A process in a restricted job
object can open, write, and delete any file its token allows — which, for a process
launched by the user running CID, is everything that user can reach. The original
implementation checked only that the *working directory* was inside the worktree, which
a command like `echo x > C:\Users\me\.ssh\authorized_keys` trivially sidesteps.

This was found because the existing boundary test asserted `passed || !passed` — a
tautology that cannot fail — so the gap was invisible in a green test suite.

Mechanisms that *would* give kernel-level filesystem confinement on Windows:

- **AppContainer** — real isolation, but requires `CreateProcessW` with
  `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, capability SIDs, and per-directory ACL
  grants for every path the Session legitimately needs. Substantial unsafe FFI.
- **Write-restricted token** (`CreateRestrictedToken` with `WRITE_RESTRICTED` plus a
  restricting SID ACE on the worktree) — also real, also substantial, and needs
  `CreateProcessAsUser`, whose privilege requirements vary by host configuration.

## Decision

Enforce the boundary in two layers, and describe each one accurately.

**Layer 1 — command path policy, on every platform.** Before any process is spawned,
the command and its arguments are scanned for path-shaped tokens. Any token that is
absolute, or that uses `..` to climb out, is resolved and checked against the Session's
worktree and its configured allowed paths. Anything landing outside is refused before
execution. Read-only system locations (`/usr`, `/bin`, `C:\Windows`, `C:\Program Files`)
are exempt so invoking an interpreter is not mistaken for an escape. A Session's
`run_terminal` working directory is additionally clamped into the Session root, so a
model-supplied `workdir` cannot redirect execution elsewhere.

**Layer 2 — kernel isolation where the OS provides it.** macOS `sandbox-exec` with a
`(deny default)` profile and worktree-scoped write allows; Linux `bubblewrap` with
bind mounts. Both genuinely confine the filesystem. Windows Job Objects are still
applied for process containment, but are **not** counted as a filesystem boundary.

`SandboxManager::status()` therefore reports `available: false` on Windows, where
`available` means *filesystem confinement specifically*, with `details` explaining why.

## Consequences

**What holds.** A command that names a path outside the worktree is blocked on every
platform. On macOS and Linux-with-bwrap, a command that computes such a path at runtime
is also blocked by the kernel. `verify_sandbox_boundary` now checks the filesystem after
the probe rather than trusting an exit code, and the test asserts the boundary actually
held.

**What does not hold.** On Windows, and on Linux without `bwrap` installed, a program
that constructs an out-of-worktree path at runtime — a script that reads a target from
an environment variable, a compiler writing to a configured output directory — is not
stopped. Layer 1 cannot see paths that do not appear in the command text.

**Therefore:** Autonomous mode on Windows is guarded by the command allow-list plus path
policy, not by kernel isolation. `SECURITY.md` states this plainly. A user who needs a
hard boundary on Windows should run Sessions in a VM or container until AppContainer
support lands.

**Revisit when** there is real demand for unattended Autonomous runs on Windows hosts
where the allow-list is not considered sufficient. The AppContainer work is well-defined;
it was deferred for cost, not because it is unclear.
