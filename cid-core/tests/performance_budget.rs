//! Performance budgets from Appendix A Part 17.
//!
//! Part 17 explicitly frames these as "budgets to validate after profiling, not
//! specs to fake." These tests measure real behavior and report real numbers —
//! a budget that can't be measured isn't a budget, and Part 21's Phase 3+ bar
//! calls for load/benchmark tests against exactly these targets:
//!
//!   - <150MB idle memory with optional features off
//!   - <2s cold start
//!   - git status/diff feels instant on repos under ~50k files
//!
//! These are coarse, environment-sensitive numbers (CI runners vary), so
//! thresholds are set generously above the budget rather than pinned exactly
//! to it — the point is catching a regression of "10x slower," not enforcing
//! the budget to the millisecond.

use cid_core::Core;
use std::time::Instant;

#[test]
fn cold_start_to_first_health_response_is_fast() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let start = Instant::now();

        let core = Core::new_in_memory().expect("core creation");
        let app = cid_core::api::router::create_router(core.app_state());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _core = core;
            let _ = axum::serve(listener, app.into_make_service()).await;
        });

        // Poll rather than sleep-then-check, so the measurement is "time until
        // actually ready" rather than "time until an arbitrary sleep elapses."
        loop {
            if reqwest::get(format!("http://{addr}/health")).await.is_ok() {
                break;
            }
            if start.elapsed().as_secs() > 10 {
                panic!("Core did not become reachable within 10s");
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let elapsed = start.elapsed();
        println!("cold start to first health response: {elapsed:?}");
        assert!(
            elapsed.as_secs_f64() < 2.0,
            "Part 17 budget is <2s cold start; measured {elapsed:?} \
             (in-memory DB, no disk I/O, so this should comfortably clear the budget)"
        );
    });
}

#[test]
fn core_construction_alone_is_well_under_the_cold_start_budget() {
    // Isolates Core::new from the network listener, so a slow CI network stack
    // cannot be blamed on Core's own startup cost.
    let start = Instant::now();
    let _core = Core::new_in_memory().expect("core creation");
    let elapsed = start.elapsed();
    println!("Core::new_in_memory: {elapsed:?}");
    assert!(
        elapsed.as_millis() < 500,
        "constructing Core should be near-instant; measured {elapsed:?}"
    );
}

#[test]
fn git_status_is_fast_on_a_small_repo() {
    use cid_core::git::GitManager;

    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    for i in 0..50 {
        std::fs::write(
            dir.path().join(format!("file{i}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }
    {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@t.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    // A dirty file so status has something to report, not just an empty scan.
    std::fs::write(dir.path().join("file0.txt"), "changed").unwrap();

    let gm = GitManager::new();
    let start = Instant::now();
    let status = gm.status(&dir.path().to_string_lossy()).unwrap();
    let elapsed = start.elapsed();

    println!(
        "git status on 50 files: {elapsed:?} ({} entries)",
        status.len()
    );
    assert!(
        elapsed.as_millis() < 500,
        "Part 17: git status should feel instant well under 50k files; measured {elapsed:?}"
    );
}

#[test]
fn repository_scan_indexes_a_moderate_repo_in_reasonable_time() {
    use cid_core::semantic_engine::index::SearchIndex;

    let dir = tempfile::tempdir().unwrap();
    for i in 0..200 {
        let content = format!(
            "fn function_{i}() {{\n    // implementation {i}\n    let x = {i};\n    x + 1\n}}\n"
        );
        std::fs::write(dir.path().join(format!("file_{i}.rs")), content).unwrap();
    }

    let index = SearchIndex::in_memory().unwrap();
    let start = Instant::now();
    let stats = cid_core::semantic_engine::index_repository_blocking(
        &dir.path().to_string_lossy(),
        Some(&index),
    )
    .unwrap();
    let elapsed = start.elapsed();

    println!(
        "indexed {} files ({} chunks) in {elapsed:?}",
        stats.files, stats.chunks
    );
    assert_eq!(stats.files, 200);
    assert!(
        elapsed.as_secs() < 10,
        "indexing 200 small files took {elapsed:?}, which is far outside a reasonable budget"
    );

    let search_start = Instant::now();
    let hits = index.search("function_100", 10).unwrap();
    println!("search after indexing: {:?}", search_start.elapsed());
    assert!(
        !hits.is_empty(),
        "the freshly indexed content must be searchable"
    );
}

#[test]
fn one_hundred_concurrent_rpc_calls_all_complete() {
    // A rough proxy for "the UI stays responsive under real concurrent load" —
    // Sessions, PTYs, and background scans all hit the same RPC surface at once
    // in real use.
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let core = Core::new_in_memory().unwrap();
        let app = cid_core::api::router::create_router(core.app_state());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _core = core;
            let _ = axum::serve(listener, app.into_make_service()).await;
        });
        for _ in 0..50 {
            if reqwest::get(format!("http://{addr}/health")).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let start = Instant::now();
        let client = reqwest::Client::new();
        let mut handles = Vec::new();
        for _ in 0..100 {
            let client = client.clone();
            let url = format!("http://{addr}/api/rpc");
            handles.push(tokio::spawn(async move {
                client
                    .post(&url)
                    .json(&serde_json::json!({
                        "jsonrpc": "2.0", "id": "1", "method": "workspace.list", "params": {}
                    }))
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
            }));
        }

        let results = futures::future::join_all(handles).await;
        let elapsed = start.elapsed();
        let succeeded = results.iter().filter(|r| matches!(r, Ok(true))).count();

        println!("100 concurrent RPC calls in {elapsed:?}, {succeeded} succeeded");
        assert_eq!(
            succeeded, 100,
            "every concurrent call must succeed, none dropped"
        );
        assert!(
            elapsed.as_secs() < 5,
            "100 trivial concurrent RPC calls took {elapsed:?}, unexpectedly slow"
        );
    });
}
