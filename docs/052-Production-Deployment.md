# 052 — Production Deployment

## Scope and why this doc exists

Written from a 2026-08 production-readiness review that found the manual steps to
actually run CID for a real team were scattered across `SECURITY.md`, `main.rs --help`,
and a couple of `docs/0NN-*.md` files that each cover one slice — with several
load-bearing steps (a TLS example, a backup procedure, running it as a persistent
service, an upgrade procedure, containerization) having **no documentation anywhere**
rather than merely being hard to find. This doc is the single place that starts from
"I built `cid-core` locally" and ends at "a small team can reach it, it survives a
restart, and I can back it up and upgrade it." It doesn't restate what those other docs
already cover well — it points to them and fills the gaps.

Unlike `docs/000`–`051`'s architecture-spec template, this is an operational runbook —
different shape on purpose, because "how do I run this" isn't a design decision with
tradeoffs to record, it's a sequence of steps to get right.

**Reminder of what CID is, deployment-wise** (per `CLAUDE.md`'s Website section): CID has
no multi-tenant hosted backend. "Production" here means *your own* `cid-core` instance,
self-hosted, reachable by your own team — not a SaaS CID is operating on your behalf.
`docs/029-Cloud.md` and `docs/030-Enterprise.md` correctly disclaim anything beyond that.

---

## 1. Build

```bash
cargo build --release -p cid-core
# binary at target/release/cid-core (cid-core.exe on Windows)
```

No installer or pre-built server binary is published today (`.github/workflows/release.yml`
only builds signed **desktop** installers — see CONTRIBUTING.md's "Release Signing
Setup"). Building from source, or the container image below, are the two real options.

### Container image

A `Dockerfile` and `docker-compose.yml` now exist at the repo root (previously: neither
existed, and there was no packaged deployment artifact of any kind for a team wanting to
run `cid-core` outside a desktop install).

**Build-verified 2026-08-19** (it was not, for the three sessions before that — no Docker
daemon was available on the machine that wrote it). What was actually run, so you know
what this claim covers:

- `docker build` from a **`git archive` of `HEAD`**, not the working tree — the artifact a
  fresh clone gets, which is what Coolify/CI builds from. (`Cargo.lock` being gitignored
  and therefore absent from that archive is exactly how the release-day build break in
  `docs/054` was missed; verifying against the working tree would not have caught it.)
  Result: clean build, ~8m20s for the `cargo build --release -p cid-core` layer, **185 MB**
  final image.
- `docker run` with `-e CID_AUTH_TOKEN` and a named volume: Core starts, binds `0.0.0.0`,
  reports `auth_required=true`, and refreshes its model catalog live from `models.dev`
  inside the container.
- `/health` → `200`. `/api/rpc` → `401` with no token **and** with a wrong token, `200`
  with the right one. `/ws` → `401` without the `cid.bearer.<base64url>` subprotocol and
  with a wrong one, **`101 Switching Protocols`** with the right one — the browser-auth
  path from `SECURITY.md` §2, confirmed against a real container rather than a loopback
  dev Core.
- `/home/cid/data/cid.db` is created and owned by `cid:cid`, confirming the `VOLUME`
  ownership fix (`docs/053` §4) works rather than failing on first write as root.

The one thing this does *not* cover: `docker-compose.yml`'s Caddy pairing and TLS
termination were not exercised, only the image itself.

```bash
docker build -t cid-core .
docker run -d --name cid-core \
  -e CID_AUTH_TOKEN="$(docker run --rm cid-core --generate-token)" \
  -v cid-data:/home/cid/data \
  -p 127.0.0.1:5919:5919 \
  cid-core
```

Or use `docker-compose.yml`, which pairs it with a Caddy reverse proxy for TLS
(see §4). The image builds `cid-core` only — the Tauri desktop shell isn't (and can't
usefully be) containerized; the web/mobile frontends are static assets, built separately
(`npm run build`) and served by any static host or folded into the same proxy (commented
in `docker-compose.yml`).

---

## 2. Configuration knobs

All of these are real `cid-core` CLI flags (`cid-core --help`) or environment variables —
none of this is aspirational:

| Knob | Flag / env | Default | Notes |
|---|---|---|---|
| Port | `--port` / `-p` | `5919` | |
| Bind address | `--host` | `127.0.0.1` | Anything non-loopback **requires** an auth token — Core refuses to start otherwise (`AccessPolicy::new`, `SECURITY.md` §2). |
| Database path | `--db` | OS data dir/`cid/cid.db` | SQLite, WAL mode. See §5 for backup. |
| Auth token | `--auth-token` or `CID_AUTH_TOKEN` | none | Required for non-loopback binds. Generate with `cid-core --generate-token`. |
| CORS origins | `--allow-origin` (repeatable) | local desktop/web shell origins | Add your web shell's real origin if hosting it elsewhere. |
| Log verbosity | `RUST_LOG` (e.g. `RUST_LOG=info`, `RUST_LOG=cid_core=debug`) | `info` | `tracing`/`tracing-subscriber`, `EnvFilter` syntax. |

There is currently no config-file option — all configuration is flags/env. If that
becomes unwieldy for your deployment, a systemd unit (§3) or a `.env` +
`docker-compose.yml` (already provided) are the practical equivalents today.

---

## 3. Running as a persistent service

Nothing in Core restarts itself after a crash (`docs/033-Observability.md`'s Failure
Modes — a crash is captured and logged, not auto-recovered). Something outside the
process needs to supervise it.

- **Linux (systemd)**: `deploy/cid-core.service` — a ready-to-copy unit with
  `Restart=on-failure` and basic filesystem hardening (`ProtectSystem=strict`, scoped
  `ReadWritePaths`). Install steps are in the file's own header comment.
- **Docker / docker-compose**: `restart: unless-stopped` is already set in the provided
  `docker-compose.yml`.
- **Windows**: no native CID-specific service wrapper exists. Use a generic process
  supervisor — [NSSM](https://nssm.cc/) (`nssm install CidCore
  C:\path\to\cid-core.exe --host 127.0.0.1 --port 5919`) is the common, well-tested
  choice; Windows' own `sc.exe create` works for a plain executable but doesn't restart
  on crash without additional `sc failure` configuration.

---

## 4. TLS and network exposure

`SECURITY.md` §2 states the requirement ("put a TLS-terminating reverse proxy in front
if the network path isn't trusted") but previously gave no example config. Two real,
ready-to-adapt ones now exist:

- `deploy/Caddyfile` — Caddy, automatic Let's Encrypt certificates, minimal config
  (handles the `/ws` WebSocket upgrade automatically).
- `deploy/nginx.conf.example` — nginx, assumes a certbot-obtained certificate, spells out
  the `Upgrade`/`Connection` headers nginx needs explicitly for `/ws` to work (unlike
  Caddy, nginx silently proxies plain HTTP fine while leaving WebSocket connections
  broken if these are missing — an easy, non-obvious failure mode to hit).

Either way: cid-core itself still binds plain HTTP (`SECURITY.md`'s stated limitation —
"traffic is plain HTTP" — is still accurate; TLS is the proxy's job, not Core's).

**Network allow-list proxy** (Autonomous-mode sandboxed commands' outbound git/npm/pip
access, `SECURITY.md` "Network access — an allow-list, not a block"): this is a separate,
*internal* proxy `SandboxManager` starts for sandboxed subprocess environments — it is
not the reverse proxy above and does not need to be, or ever be, internet-facing. No
additional firewall configuration is needed for it beyond the host's own outbound rules;
just don't confuse it with the TLS-terminating reverse proxy when reasoning about what's
exposed to the network.

---

## 5. Database backup and restore

`docs/021-Storage.md` states plainly that no backup/restore mechanism exists in Core
itself. `deploy/backup-cid-db.sh` is the operational answer: it uses `sqlite3 <db>
".backup"`, which takes a consistent snapshot through SQLite's own backup API — safe to
run against a live database in WAL mode, unlike a plain file copy, which can miss data
still sitting in the `-wal` file next to the main `.db` file.

```bash
./deploy/backup-cid-db.sh /path/to/cid.db /path/to/backup/dir
# keeps the last 14 backups by default; adjust retention in the script
```

**Restore**: stop `cid-core`, copy the chosen backup over the real `--db` path, delete
any stale `cid.db-wal`/`cid.db-shm` sitting next to it, then start Core again.

Run this on a schedule (`cron`, a systemd timer, or your platform's scheduled-task
equivalent) — none is wired up by default; this doc intentionally doesn't prescribe one
schedule since retention needs vary by team.

---

## 6. Logs

Logs go to stdout/stderr only (`RUST_LOG`-controlled) — Core does not write its own log
file or rotate anything. For a systemd-run instance, `journald` already captures and
rotates this for you (`journalctl -u cid-core`). For Docker, the standard `docker logs`
/ your container runtime's own log-driver rotation applies. If running as a bare
foreground process, redirect to a file and rotate with `logrotate` (Linux) or a
scheduled task (Windows) — no CID-specific tooling is needed or provided here since this
is a generic process-output-rotation problem, not something specific to Core's behavior.

---

## 7. Monitoring

- **`GET /health`** — unauthenticated, minimal (reachability, version, connected client
  count, whether auth is required). Suitable for a load balancer / container health
  check (`docker-compose.yml`'s image doesn't define a `HEALTHCHECK` directive by
  default — add one pointed at this endpoint if your orchestrator doesn't already probe
  it another way).
- **`GET /metrics`** — real Prometheus text-exposition format
  (`docs/033-Observability.md`, corrected in this same pass — it previously claimed this
  didn't exist). `deploy/prometheus-scrape-example.yml` is a ready-to-paste scrape-config
  block for your own Prometheus server; CID does not bundle or run one.
- **Crash log**: `observability.crashes.list` RPC surfaces the last 200 captured panics
  (redacted message, `file:line:col`, thread). There is no push-based alerting on this —
  poll it, or watch the log file passed to the crash logger, if you want alerting; wiring
  that to a real notification channel (PagerDuty, Slack, email) is left to the operator,
  same as metrics.

---

## 8. Upgrading

`docs/021-Storage.md` states the migration approach is additive-only and would not
gracefully handle a column rename or type change — a risk disclosure, not an upgrade
runbook. Until CID ships a documented migration-compatibility policy, the safe upgrade
procedure is:

1. Take a real backup first (§5) — non-negotiable given the above.
2. Stop `cid-core`.
3. Replace the binary (or pull a new container image / rebuild from a newer source
   checkout).
4. Start it and check `/health` and the logs before considering the upgrade complete.
5. If anything looks wrong, restore the backup from step 1 and roll back the binary.

There is currently no automated rollback or blue/green deployment story — for a single
self-hosted instance this manual sequence is the realistic scope; a team running
multiple environments should build their own promotion pipeline around it rather than
expecting one from CID.

---

## 9. What's still genuinely missing (honest gaps, not silently dropped)

- **No config file** — flags/env only (§2). Fine at current scope; would need real design
  work (precedence rules, hot-reload or not) before adding one, not a quick patch.
- ~~**No auth token rotation without a restart**~~ — **closed 2026-08-19.**
  `access.token.rotate` swaps the token on a running Core and drops live WebSocket
  sessions so the old credential stops working immediately; `SECURITY.md` §2 has the
  rules and refusals. Rotate with:

  ```bash
  curl -s -X POST https://your-core/api/rpc \
    -H "Authorization: Bearer $CURRENT_TOKEN" -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"access.token.rotate","params":{}}'
  ```

  The response carries the token now in force. **It is in memory only** — the config-file
  gap above is exactly why: update `CID_AUTH_TOKEN` (or the `--auth-token` flag) in your
  service definition as well, or the next restart reverts to the old value. Every client
  must be given the new token; browsers will be prompted for it automatically once their
  socket closes.
- **No multi-instance/HA story** — one `cid-core` process, one SQLite file. Horizontal
  scaling was never a design goal (self-hosted, single-team scale); documented here as a
  boundary, not a bug.
- **No automated migration-compatibility testing across versions** — see §8.
- **Windows service wrapper** — a third-party tool (NSSM) is the answer, not a CID-native
  one; see §3.

If you close one of these, update this section in the same change — this is the file
that goes stale first if a real production gap is fixed without a doc update to match
(see `docs/033-Observability.md`'s own corrected staleness in this same pass for exactly
that failure mode).
