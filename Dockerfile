# Headless cid-core daemon only — the Tauri desktop shell is not (and cannot
# usefully be) containerized; the web/mobile shells are static assets served
# by any web server pointed at `VITE_CID_CORE_HOST` (see
# docs/052-Production-Deployment.md). git2/rusqlite are vendored ("bundled" /
# "vendored-libgit2" / "vendored-openssl" in cid-core/Cargo.toml), so the
# build stage needs a C toolchain but the runtime image needs no external
# libgit2/sqlite/openssl packages.

FROM rust:1-bookworm AS builder
WORKDIR /build

# cmake/perl are needed by vendored-openssl's build script; the rest is the
# usual native-extension toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake perl pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY cid-core ./cid-core
COPY cid-tui ./cid-tui
COPY src-tauri ./src-tauri

# Only cid-core is needed at runtime; building it alone skips the Tauri shell's
# system webview dependencies entirely, matching CI's own
# `--exclude cid`/`-p cid-core` split (see docs/036-CI-CD.md).
RUN cargo build --release -p cid-core

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# `data/` must exist AND be owned by `cid` in the image itself. Docker creates
# a declared VOLUME's mount point as root:root when the path is absent from the
# image — so with the container running as the unprivileged `cid` user, the
# `--db /home/cid/data/cid.db` in CMD below would fail with a permission error
# on the very first start. Creating it here is what makes the default CMD work.
RUN useradd --create-home --shell /usr/sbin/nologin cid \
    && mkdir -p /home/cid/data \
    && chown -R cid:cid /home/cid

# Before USER, so this lands in /usr/local/bin as root rather than depending on
# the unprivileged user having write access there.
COPY --from=builder /build/target/release/cid-core /usr/local/bin/cid-core

USER cid
WORKDIR /home/cid

ENV RUST_LOG=info
VOLUME ["/home/cid/data"]
EXPOSE 5919

# --host 0.0.0.0 is required here (containers are reached across a network
# namespace boundary even for "local" use) — CID_AUTH_TOKEN is therefore
# mandatory, not optional, for a containerized deployment. See
# docs/052-Production-Deployment.md before running this without it.
ENTRYPOINT ["cid-core"]
CMD ["--host", "0.0.0.0", "--port", "5919", "--db", "/home/cid/data/cid.db"]
