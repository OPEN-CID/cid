use anyhow::{Context, Result};
use cid_core::{
    access::{generate_token, AccessPolicy},
    Core,
};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use tracing_subscriber::EnvFilter;

const HELP: &str = "\
CID Core — headless daemon for the CID platform

Usage: cid-core [OPTIONS]

Options:
  -p, --port <PORT>          JSON-RPC port (default: 5919)
      --host <ADDR>          Address to bind (default: 127.0.0.1)
                             Binding to anything other than loopback requires --auth-token.
      --db <PATH>            SQLite database path (default: OS data dir/cid/cid.db)
      --auth-token <TOKEN>   Bearer token required on /api/rpc and /ws.
                             May also be supplied via CID_AUTH_TOKEN.
      --generate-token       Print a fresh token and exit.
      --allow-origin <ORIGIN>  Add a permitted CORS origin (repeatable).
                             Defaults cover the local desktop and web shells.
  -h, --help                 Show this help

Examples:
  cid-core --port 5919
  cid-core --host 0.0.0.0 --auth-token \"$(cid-core --generate-token)\"
";

struct Args {
    port: u16,
    host: IpAddr,
    db_path: Option<PathBuf>,
    token: Option<String>,
    origins: Vec<String>,
}

fn parse_args() -> Result<Option<Args>> {
    let argv: Vec<String> = std::env::args().collect();
    let mut args = Args {
        port: 5919,
        host: "127.0.0.1".parse().unwrap(),
        db_path: None,
        token: std::env::var("CID_AUTH_TOKEN").ok(),
        origins: Vec::new(),
    };

    let mut i = 1;
    while i < argv.len() {
        let next = |i: usize| -> Result<String> {
            argv.get(i + 1)
                .cloned()
                .with_context(|| format!("{} requires a value", argv[i]))
        };
        match argv[i].as_str() {
            "--port" | "-p" => {
                args.port = next(i)?.parse().context("--port must be a number")?;
                i += 1;
            }
            "--host" => {
                args.host = next(i)?.parse().context("--host must be an IP address")?;
                i += 1;
            }
            "--db" => {
                args.db_path = Some(PathBuf::from(next(i)?));
                i += 1;
            }
            "--auth-token" => {
                args.token = Some(next(i)?);
                i += 1;
            }
            "--allow-origin" => {
                args.origins.push(next(i)?);
                i += 1;
            }
            "--generate-token" => {
                println!("{}", generate_token());
                return Ok(None);
            }
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(None);
            }
            other => anyhow::bail!("Unknown argument: {other}\n\n{HELP}"),
        }
        i += 1;
    }
    Ok(Some(args))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args = match parse_args()? {
        Some(a) => a,
        None => return Ok(()),
    };

    // Built before Core so an unsafe bind fails immediately, with an actionable
    // message, rather than after the daemon is already listening.
    let policy = AccessPolicy::new(args.host, args.token, args.origins)?;

    let mut core = Core::new(args.db_path.clone())?;
    core.set_access_policy(policy);
    cid_core::observability::install_panic_hook(core.crash_log.clone());

    // Best-effort, non-blocking: pulls the current model catalog (ids, context
    // windows, real per-token pricing) so the picker and every spend estimate
    // stop depending on a snapshot compiled into the binary. Started here
    // rather than in `Core::new` because it needs a Tokio runtime, and
    // `Core::new` is called from sync contexts and tests. Offline is fine —
    // the cached or bundled catalog stands and a warning is logged.
    cid_core::model::catalog::refresh_in_background();

    let addr = SocketAddr::new(args.host, args.port);
    println!("CID Core v{}", env!("CARGO_PKG_VERSION"));
    println!("  WS:       ws://{addr}/ws  (JSON-RPC 2.0)");
    println!("  HTTP RPC: http://{addr}/api/rpc");
    println!("  Health:   http://{addr}/health");
    match args.db_path {
        Some(ref p) => println!("  DB:       {}", p.display()),
        None => println!("  DB:       default OS data directory"),
    }
    if core.access_policy.requires_auth() {
        println!("  Auth:     bearer token required");
    } else {
        println!("  Auth:     none (loopback only)");
    }

    core.serve(addr).await?;
    Ok(())
}
