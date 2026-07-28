//! Integration tests for the headless Core JSON-RPC API surface.
//!
//! Phase 1 testing bar (Appendix A, Part 21): the Web and Mobile shells depend on
//! this surface, so it is exercised over real HTTP against a running Core rather
//! than by calling handlers directly.

use cid_core::Core;
use serde_json::{json, Value};
use std::net::SocketAddr;

/// Start a Core on an ephemeral port with in-memory persistence.
/// Returns the base URL once the server is accepting connections.
async fn start_core() -> String {
    let core = Core::new_in_memory().expect("core creation");
    let app = cid_core::api::router::create_router(core.app_state());
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        // Core is moved in so its managers outlive the server task.
        let _core = core;
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    let base = format!("http://{}", addr);
    for _ in 0..50 {
        if reqwest::get(format!("{}/health", base)).await.is_ok() {
            return base;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("core did not become reachable");
}

async fn rpc(base: &str, method: &str, params: Value) -> Value {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/rpc", base))
        .json(&json!({ "jsonrpc": "2.0", "id": "1", "method": method, "params": params }))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{} request failed: {e}", method));
    resp.json().await.expect("json response")
}

/// Unwrap a successful JSON-RPC result, panicking with the error payload otherwise.
async fn rpc_ok(base: &str, method: &str, params: Value) -> Value {
    let body = rpc(base, method, params).await;
    match body.get("result") {
        Some(r) => r.clone(),
        None => panic!("{} returned error: {}", method, body),
    }
}

/// Create a real git repo with one commit, connect it, and open a Mission.
/// Returns (base_url, tempdir guard, mission_id).
async fn mission_fixture(autonomy: &str) -> (String, tempfile::TempDir, String) {
    let base = start_core().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("git init");
    std::fs::write(dir.path().join("README.md"), "# fixture\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    let repo_path = dir.path().to_string_lossy().to_string();
    let channel = rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;
    let channel_id = channel["id"].as_str().expect("channel id").to_string();

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel_id,
            "title": "Fixture mission",
            "task": "Add a greeting to README",
            "session_mode": "shared",
            "autonomy_level": autonomy,
        }),
    )
    .await;
    let mission_id = mission["id"].as_str().expect("mission id").to_string();
    (base, dir, mission_id)
}

#[tokio::test]
async fn co_pilot_mission_is_gated_until_a_plan_is_approved() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;

    // A Mission with no approved plan reports the Implementer as blocked.
    let gate = rpc_ok(
        &base,
        "mission.plan.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(
        gate["implementer_blocked_reason"].is_string(),
        "a Mission without an approved plan must block the Implementer: {gate}"
    );

    // Sending a message returns blocked rather than silently starting work.
    let sent = rpc_ok(
        &base,
        "mission.send_message",
        json!({ "mission_id": mission_id, "content": "go ahead" }),
    )
    .await;
    assert_eq!(
        sent["blocked"], true,
        "Implementer must not run behind the gate: {sent}"
    );

    // Write a plan by hand, then approve it.
    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Requirements\n- greet\n## Approach\nedit README\n## Steps\n1. Edit README.md" }),
    )
    .await;
    let approved = rpc_ok(
        &base,
        "mission.plan.approve",
        json!({ "mission_id": mission_id, "approved_by": "tester" }),
    )
    .await;
    assert_eq!(approved["status"], "approved");
    assert_eq!(approved["approved_by"], "tester");

    // The gate is now open.
    let gate = rpc_ok(
        &base,
        "mission.plan.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(
        gate["implementer_blocked_reason"].is_null(),
        "an approved plan must open the gate: {gate}"
    );
}

#[tokio::test]
async fn editing_an_approved_plan_revokes_the_approval() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;

    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. original" }),
    )
    .await;
    rpc_ok(
        &base,
        "mission.plan.approve",
        json!({ "mission_id": mission_id }),
    )
    .await;

    let edited = rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. something entirely different" }),
    )
    .await;
    assert_eq!(
        edited["status"], "draft",
        "the approval applied to the old text and must not carry over: {edited}"
    );
    assert!(edited["approved_by"].is_null());
}

#[tokio::test]
async fn manual_autonomy_has_no_plan_gate() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;
    let gate = rpc_ok(
        &base,
        "mission.plan.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(
        gate["implementer_blocked_reason"].is_null(),
        "Manual autonomy means the human is driving; no plan gate applies: {gate}"
    );
}

#[tokio::test]
async fn rejecting_a_plan_keeps_the_gate_closed() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;

    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. do a thing" }),
    )
    .await;
    let rejected = rpc_ok(
        &base,
        "mission.plan.reject",
        json!({ "mission_id": mission_id, "reason": "too broad" }),
    )
    .await;
    assert_eq!(rejected["status"], "rejected");

    let gate = rpc_ok(
        &base,
        "mission.plan.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(gate["implementer_blocked_reason"].is_string());
}

#[tokio::test]
async fn an_empty_plan_cannot_be_written_or_approved() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;

    let body = rpc(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "   " }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "empty plan text must be rejected"
    );

    let body = rpc(
        &base,
        "mission.plan.approve",
        json!({ "mission_id": "no-such-mission" }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "cannot approve a plan that does not exist"
    );
}

#[tokio::test]
async fn review_of_a_clean_worktree_reports_no_findings() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;

    let review = rpc_ok(
        &base,
        "mission.review.run",
        json!({ "mission_id": mission_id, "diff": "" }),
    )
    .await;
    assert_eq!(review["verdict"], "clean");
    assert_eq!(review["findings"].as_array().map(|f| f.len()), Some(0));

    let latest = rpc_ok(
        &base,
        "mission.review.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert_eq!(latest["id"], review["id"], "the review must be persisted");
}

#[tokio::test]
async fn reviews_accumulate_and_list_newest_first() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;

    rpc_ok(
        &base,
        "mission.review.run",
        json!({ "mission_id": mission_id, "diff": "" }),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    rpc_ok(
        &base,
        "mission.review.run",
        json!({ "mission_id": mission_id, "diff": "" }),
    )
    .await;

    let all = rpc_ok(
        &base,
        "mission.review.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    let reviews = all.as_array().expect("array");
    assert_eq!(reviews.len(), 2, "each run is recorded: {reviews:?}");
    assert!(reviews[0]["created_at"].as_str() >= reviews[1]["created_at"].as_str());
}

// ---- Phase 3: accounts, roles, governance ----

/// Register an Owner and sign in, returning their session token.
async fn owner_session(base: &str) -> String {
    rpc_ok(
        base,
        "auth.register",
        json!({ "username": "owner", "password": "correct-horse-battery" }),
    )
    .await;
    let session = rpc_ok(
        base,
        "auth.login",
        json!({ "username": "owner", "password": "correct-horse-battery" }),
    )
    .await;
    session["token"]
        .as_str()
        .expect("session token")
        .to_string()
}

#[tokio::test]
async fn auth_reports_when_no_account_exists_yet() {
    let base = start_core().await;
    let status = rpc_ok(&base, "auth.status", json!({})).await;
    assert_eq!(status["bootstrapped"], false);
}

#[tokio::test]
async fn the_first_registration_needs_no_session_and_yields_an_owner() {
    let base = start_core().await;
    let user = rpc_ok(
        &base,
        "auth.register",
        json!({ "username": "owner", "password": "correct-horse-battery" }),
    )
    .await;
    assert_eq!(user["role"], "owner");
    assert_eq!(
        rpc_ok(&base, "auth.status", json!({})).await["bootstrapped"],
        true
    );
}

#[tokio::test]
async fn later_registrations_require_an_admin_session() {
    let base = start_core().await;
    let token = owner_session(&base).await;

    let body = rpc(
        &base,
        "auth.register",
        json!({ "username": "bob", "password": "another-long-password" }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "creating an account once bootstrapped is an administrative act: {body}"
    );

    let created = rpc_ok(
        &base,
        "auth.register",
        json!({
            "username": "bob",
            "password": "another-long-password",
            "role": "developer",
            "session_token": token,
        }),
    )
    .await;
    assert_eq!(created["role"], "developer");
}

#[tokio::test]
async fn login_returns_a_session_that_resolves_and_can_be_revoked() {
    let base = start_core().await;
    let token = owner_session(&base).await;

    let resolved = rpc_ok(&base, "auth.session", json!({ "session_token": token })).await;
    assert_eq!(resolved["username"], "owner");

    rpc_ok(&base, "auth.logout", json!({ "session_token": token })).await;
    let after = rpc_ok(&base, "auth.session", json!({ "session_token": token })).await;
    assert!(
        after.is_null(),
        "a logged-out session must not resolve: {after}"
    );
}

#[tokio::test]
async fn listing_users_requires_an_admin_session() {
    let base = start_core().await;
    let owner = owner_session(&base).await;
    rpc_ok(
        &base,
        "auth.register",
        json!({
            "username": "viewer", "password": "another-long-password",
            "role": "viewer", "session_token": owner,
        }),
    )
    .await;
    let viewer = rpc_ok(
        &base,
        "auth.login",
        json!({ "username": "viewer", "password": "another-long-password" }),
    )
    .await;
    let viewer_token = viewer["token"].as_str().unwrap().to_string();

    let denied = rpc(
        &base,
        "auth.users.list",
        json!({ "session_token": viewer_token }),
    )
    .await;
    assert!(
        denied.get("result").is_none(),
        "a Viewer must not list accounts"
    );

    let allowed = rpc_ok(&base, "auth.users.list", json!({ "session_token": owner })).await;
    assert_eq!(allowed.as_array().map(|a| a.len()), Some(2));
}

#[tokio::test]
async fn an_expired_or_bogus_session_is_refused_with_a_clear_message() {
    let base = start_core().await;
    owner_session(&base).await;
    let body = rpc(
        &base,
        "auth.users.list",
        json!({ "session_token": "not-a-token" }),
    )
    .await;
    let msg = body["error"]["message"].as_str().unwrap_or_default();
    assert!(msg.contains("expired") || msg.contains("invalid"), "{msg}");
}

#[tokio::test]
async fn governance_policy_defaults_to_autonomous_disabled() {
    let base = start_core().await;
    let policy = rpc_ok(&base, "governance.policy.get", json!({})).await;
    assert_eq!(policy["autonomous_enabled"], false);
    assert_eq!(policy["min_role_for_autonomous"], "admin");
}

#[tokio::test]
async fn only_an_admin_can_change_governance_policy() {
    let base = start_core().await;
    let owner = owner_session(&base).await;
    rpc_ok(
        &base,
        "auth.register",
        json!({
            "username": "dev", "password": "another-long-password",
            "role": "developer", "session_token": owner,
        }),
    )
    .await;
    let dev = rpc_ok(
        &base,
        "auth.login",
        json!({ "username": "dev", "password": "another-long-password" }),
    )
    .await;

    let mut policy = rpc_ok(&base, "governance.policy.get", json!({})).await;
    policy["autonomous_enabled"] = json!(true);

    let denied = rpc(
        &base,
        "governance.policy.set",
        json!({ "session_token": dev["token"], "policy": policy }),
    )
    .await;
    assert!(
        denied.get("result").is_none(),
        "a Developer must not change policy"
    );

    let after = rpc_ok(&base, "governance.policy.get", json!({})).await;
    assert_eq!(
        after["autonomous_enabled"], false,
        "policy must be unchanged"
    );
}

#[tokio::test]
async fn creating_an_autonomous_mission_is_refused_by_default_policy() {
    let base = start_core().await;
    let token = owner_session(&base).await;

    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let channel = rpc_ok(
        &base,
        "repo.connect",
        json!({ "path": dir.path().to_string_lossy() }),
    )
    .await;

    let body = rpc(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel["id"],
            "title": "Autonomous run",
            "task": "do it all",
            "session_mode": "shared",
            "autonomy_level": "autonomous",
            "session_token": token,
        }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "Autonomous mode is off by Workspace policy and must be refused: {body}"
    );
    let msg = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("Autonomous"),
        "the refusal should explain itself: {msg}"
    );
}

#[tokio::test]
async fn an_autonomous_mission_is_allowed_once_policy_permits_the_repo() {
    let base = start_core().await;
    let token = owner_session(&base).await;

    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();
    let channel = rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;

    let mut policy = rpc_ok(&base, "governance.policy.get", json!({})).await;
    policy["autonomous_enabled"] = json!(true);
    policy["autonomous_repos"] = json!([repo_path]);
    rpc_ok(
        &base,
        "governance.policy.set",
        json!({ "session_token": token, "policy": policy }),
    )
    .await;

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel["id"],
            "title": "Autonomous run",
            "task": "do it all",
            "session_mode": "shared",
            "autonomy_level": "autonomous",
            "session_token": token,
        }),
    )
    .await;
    assert_eq!(mission["autonomy_level"], "autonomous");
}

#[tokio::test]
async fn plan_approval_records_the_approving_user() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;
    let token = owner_session(&base).await;

    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. do the thing" }),
    )
    .await;
    let approved = rpc_ok(
        &base,
        "mission.plan.approve",
        json!({ "mission_id": mission_id, "session_token": token }),
    )
    .await;
    assert_eq!(approved["status"], "approved");
    assert_eq!(
        approved["approved_by"], "owner",
        "the audit trail needs the real user, not a free-text string"
    );
}

#[tokio::test]
async fn a_viewer_cannot_approve_a_plan() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;
    let owner = owner_session(&base).await;
    rpc_ok(
        &base,
        "auth.register",
        json!({
            "username": "viewer", "password": "another-long-password",
            "role": "viewer", "session_token": owner,
        }),
    )
    .await;
    let viewer = rpc_ok(
        &base,
        "auth.login",
        json!({ "username": "viewer", "password": "another-long-password" }),
    )
    .await;

    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. do the thing" }),
    )
    .await;
    let denied = rpc(
        &base,
        "mission.plan.approve",
        json!({ "mission_id": mission_id, "session_token": viewer["token"] }),
    )
    .await;
    assert!(
        denied.get("result").is_none(),
        "a Viewer must not approve plans"
    );
}

#[tokio::test]
async fn spend_caps_are_enforced_before_the_spend() {
    let base = start_core().await;
    let token = owner_session(&base).await;

    let mut policy = rpc_ok(&base, "governance.policy.get", json!({})).await;
    policy["mission_spend_cap_usd"] = json!(5.0);
    rpc_ok(
        &base,
        "governance.policy.set",
        json!({ "session_token": token, "policy": policy }),
    )
    .await;

    rpc_ok(
        &base,
        "governance.spend.record",
        json!({ "mission_id": "m1", "usd": 4.0, "note": "planner" }),
    )
    .await;

    let ok = rpc_ok(
        &base,
        "governance.spend.check",
        json!({ "mission_id": "m1", "usd": 0.5 }),
    )
    .await;
    assert_eq!(ok["decision"], "allow");

    let denied = rpc_ok(
        &base,
        "governance.spend.check",
        json!({ "mission_id": "m1", "usd": 3.0 }),
    )
    .await;
    assert_eq!(denied["decision"], "deny");
    assert!(denied["reason"].as_str().unwrap().contains("spend cap"));

    let summary = rpc_ok(
        &base,
        "governance.spend.summary",
        json!({ "mission_id": "m1" }),
    )
    .await;
    assert!((summary["mission_spend_usd"].as_f64().unwrap() - 4.0).abs() < 1e-9);
}

#[tokio::test]
async fn decisions_list_reads_real_adrs_from_the_repo() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let adr_dir = dir.path().join("docs").join("adr");
    std::fs::create_dir_all(&adr_dir).unwrap();
    std::fs::write(
        adr_dir.join("0001-pick-a-database.md"),
        "# ADR 0001 — Pick a database\n\n**Status:** Accepted\n",
    )
    .unwrap();

    let adrs = rpc_ok(
        &base,
        "decisions.list",
        json!({ "repo_path": dir.path().to_string_lossy() }),
    )
    .await;
    let list = adrs.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["number"], "0001");
    assert_eq!(list[0]["status"], "Accepted");
}

#[tokio::test]
async fn decisions_for_mission_finds_adrs_referenced_in_the_task() {
    let (base, dir, mission_id) = mission_fixture("manual").await;
    let adr_dir = dir.path().join("docs").join("adr");
    std::fs::create_dir_all(&adr_dir).unwrap();
    std::fs::write(
        adr_dir.join("0042-relevant-decision.md"),
        "# ADR 0042 — Relevant\n",
    )
    .unwrap();

    // mission_fixture's task description doesn't mention the ADR, so the
    // real behavior (no match) is exercised first...
    let none = rpc_ok(
        &base,
        "decisions.for_mission",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert_eq!(none.as_array().map(|a| a.len()), Some(0));

    // ...then approve a plan that does reference it, and confirm it surfaces.
    rpc_ok(
        &base,
        "mission.plan.update",
        json!({ "mission_id": mission_id, "content": "## Steps\n1. Follow ADR 0042 exactly." }),
    )
    .await;
    let found = rpc_ok(
        &base,
        "decisions.for_mission",
        json!({ "mission_id": mission_id }),
    )
    .await;
    let list = found.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["number"], "0042");
}

#[tokio::test]
async fn deployment_record_and_webhook_are_tagged_with_their_real_source() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;

    let manual = rpc_ok(
        &base,
        "deployment.record",
        json!({ "mission_id": mission_id, "environment": "staging", "commit_or_tag": "abc123" }),
    )
    .await;
    assert_eq!(manual["source"], "manual");

    let webhook = rpc_ok(
        &base,
        "deployment.webhook",
        json!({ "mission_id": mission_id, "environment": "production", "commit_or_tag": "def456" }),
    )
    .await;
    assert_eq!(webhook["source"], "ci_webhook");

    let all = rpc_ok(
        &base,
        "deployment.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert_eq!(all.as_array().map(|a| a.len()), Some(2));
}

#[tokio::test]
async fn deployment_record_cannot_orchestrate_anything_it_can_only_log() {
    // There is deliberately no RPC method that deploys anything — this test
    // documents that boundary by asserting the only two entry points are the
    // ones tested above, both of which just persist a record.
    let (base, _dir, mission_id) = mission_fixture("manual").await;
    let body = rpc(
        &base,
        "deployment.record",
        json!({ "mission_id": mission_id, "environment": "", "commit_or_tag": "" }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "an empty record must be rejected, not silently accepted"
    );
}

#[tokio::test]
async fn test_impact_and_doc_graphs_populate_after_enabling_the_semantic_engine() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();

    std::fs::write(
        dir.path().join("math.rs"),
        "pub fn add_numbers(a: i32, b: i32) -> i32 { a + b }",
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("tests").join("math_test.rs"),
        "fn it_adds() { add_numbers(1, 2); }",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("README.md"),
        "Call `add_numbers` to sum two integers. Also see `long_removed_fn`.",
    )
    .unwrap();

    rpc_ok(
        &base,
        "semantic_engine.enable",
        json!({ "repo_path": repo_path }),
    )
    .await;

    // The scan runs in the background; poll status until indexing completes.
    for _ in 0..100 {
        let status = rpc_ok(
            &base,
            "semantic_engine.status",
            json!({ "repo_path": repo_path }),
        )
        .await;
        if status["indexing"] == false && status["indexed_files"].as_u64().unwrap_or(0) > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    let impact = rpc_ok(
        &base,
        "semantic_engine.test_impact.for_symbol",
        json!({ "repo_path": repo_path, "symbol": "add_numbers" }),
    )
    .await;
    let tests = impact["covering_tests"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        tests
            .iter()
            .any(|t| t.as_str().unwrap_or_default().contains("math_test.rs")),
        "the test-impact graph should find the covering test: {impact}"
    );

    let docs = rpc_ok(
        &base,
        "semantic_engine.docs.for_symbol",
        json!({ "repo_path": repo_path, "symbol": "add_numbers" }),
    )
    .await;
    let doc_list = docs["docs"].as_array().cloned().unwrap_or_default();
    assert!(
        doc_list
            .iter()
            .any(|d| d.as_str().unwrap_or_default().contains("README.md")),
        "the doc graph should find README.md referencing add_numbers: {docs}"
    );

    let stale = rpc_ok(
        &base,
        "semantic_engine.docs.stale",
        json!({ "repo_path": repo_path }),
    )
    .await;
    let stale_list = stale.as_array().cloned().unwrap_or_default();
    assert!(
        stale_list.iter().any(|s| s["missing_symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "long_removed_fn")),
        "README.md references a symbol that doesn't exist and should be reported stale: {stale}"
    );
}

#[tokio::test]
async fn test_impact_and_docs_are_empty_before_the_engine_is_enabled() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();

    let impact = rpc_ok(
        &base,
        "semantic_engine.test_impact.for_symbol",
        json!({ "repo_path": repo_path, "symbol": "anything" }),
    )
    .await;
    assert_eq!(
        impact["covering_tests"].as_array().map(|a| a.len()),
        Some(0)
    );
}

#[tokio::test]
async fn confidence_score_is_computed_and_logged_to_the_mission() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;

    let card = rpc_ok(
        &base,
        "confidence.score",
        json!({
            "mission_id": mission_id,
            "target_file": "src/new.rs",
            "new_content": "pub fn tidy_function(a: i32, b: i32) -> i32 { a + b }",
        }),
    )
    .await;

    assert_eq!(card["signals"].as_array().map(|a| a.len()), Some(9));
    assert!(card["overall"].as_f64().unwrap() > 0.0);

    let history = rpc_ok(
        &base,
        "confidence.history",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert_eq!(history.as_array().map(|a| a.len()), Some(1));

    let messages = rpc_ok(&base, "message.list", json!({ "mission_id": mission_id })).await;
    let has_summary = messages.as_array().unwrap().iter().any(|m| {
        m["content"]
            .as_str()
            .unwrap_or_default()
            .contains("Confidence")
    });
    assert!(
        has_summary,
        "a confidence summary must appear in the thread: {messages}"
    );
}

#[tokio::test]
async fn confidence_score_reads_the_worktree_file_when_no_content_is_supplied() {
    let (base, dir, mission_id) = mission_fixture("manual").await;
    std::fs::write(dir.path().join("existing.rs"), "pub fn already_here() {}").unwrap();

    let card = rpc_ok(
        &base,
        "confidence.score",
        json!({ "mission_id": mission_id, "target_file": "existing.rs" }),
    )
    .await;
    assert_eq!(card["signals"].as_array().map(|a| a.len()), Some(9));
}

#[tokio::test]
async fn confidence_score_without_content_or_an_existing_file_fails_clearly() {
    let (base, _dir, mission_id) = mission_fixture("manual").await;
    let body = rpc(
        &base,
        "confidence.score",
        json!({ "mission_id": mission_id, "target_file": "does_not_exist.rs" }),
    )
    .await;
    assert!(body.get("result").is_none());
}

#[tokio::test]
async fn vibe_preset_mission_starts_with_an_already_approved_plan() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let channel = rpc_ok(
        &base,
        "repo.connect",
        json!({ "path": dir.path().to_string_lossy() }),
    )
    .await;

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel["id"],
            "title": "Quick typo fix",
            "task": "Fix a typo in the README",
            "session_mode": "shared",
            "autonomy_level": "co_pilot",
            "vibe": true,
        }),
    )
    .await;
    let mission_id = mission["id"].as_str().unwrap().to_string();

    // The plan is already approved by the time mission.create returns — no
    // background wait, no mission.plan.changed race, unlike the normal path.
    let gate = rpc_ok(
        &base,
        "mission.plan.get",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(
        gate["implementer_blocked_reason"].is_null(),
        "a vibe Mission's gate must already be open: {gate}"
    );
    assert_eq!(gate["plan"]["status"], "approved");
    assert_eq!(gate["plan"]["approved_by"], "vibe-preset");
}

#[tokio::test]
async fn vibe_preset_does_not_bypass_tool_call_approval() {
    // The preset shortens planning, not review — Co-Pilot's per-tool-call
    // approval must still apply to a vibe Mission exactly as it does to a
    // normally-planned one. This is asserted indirectly: sending a message
    // to a vibe Mission must NOT come back `blocked` (the gate is open), but
    // the Mission's autonomy level itself is unchanged from what was
    // requested (co_pilot), which is what drives per-tool approval in the
    // model loop.
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let channel = rpc_ok(
        &base,
        "repo.connect",
        json!({ "path": dir.path().to_string_lossy() }),
    )
    .await;

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel["id"],
            "title": "Quick fix",
            "task": "small change",
            "session_mode": "shared",
            "autonomy_level": "co_pilot",
            "vibe": true,
        }),
    )
    .await;
    assert_eq!(
        mission["autonomy_level"], "co_pilot",
        "vibe mode changes planning ceremony, not the requested autonomy level"
    );
}

#[tokio::test]
async fn non_vibe_mission_still_uses_the_full_planner() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let channel = rpc_ok(
        &base,
        "repo.connect",
        json!({ "path": dir.path().to_string_lossy() }),
    )
    .await;

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel["id"],
            "title": "Real feature",
            "task": "Build something substantial",
            "session_mode": "shared",
            "autonomy_level": "co_pilot",
        }),
    )
    .await;
    let mission_id = mission["id"].as_str().unwrap().to_string();

    // Without vibe:true, the ordinary Planner path applies — the plan is not
    // pre-approved with the vibe-preset marker.
    for _ in 0..50 {
        let gate = rpc_ok(
            &base,
            "mission.plan.get",
            json!({ "mission_id": mission_id }),
        )
        .await;
        if !gate["plan"].is_null() {
            assert_ne!(gate["plan"]["approved_by"], "vibe-preset");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("expected a plan to be generated by the background Planner");
}

#[tokio::test]
async fn repo_health_scan_reports_untested_and_duplicate_tests() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        "pub fn risky() { todo!() }\n\
         #[test]\nfn checks_one() { assert_eq!(1 + 1, 2); }\n\
         #[test]\nfn also_checks_one() { assert_eq!(1 + 1, 2); }\n",
    )
    .unwrap();

    let report = rpc_ok(
        &base,
        "repo_health.scan",
        json!({ "path": dir.path().to_string_lossy() }),
    )
    .await;

    assert!(report["total_fns"].as_u64().unwrap() >= 3);
    assert!(report["duplicate_test_groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|g| g["tests"].as_array().unwrap().len() == 2));
}

#[tokio::test]
async fn observability_crashes_list_starts_empty() {
    let base = start_core().await;
    let crashes = rpc_ok(&base, "observability.crashes.list", json!({})).await;
    assert_eq!(crashes.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn metrics_endpoint_reports_prometheus_text_and_counts_rpc_calls() {
    let base = start_core().await;
    // Any RPC call at all should bump the counter this test then checks for.
    rpc_ok(&base, "workspace.list", json!({})).await;

    let resp = reqwest::get(format!("{}/metrics", base)).await.unwrap();
    assert!(resp.status().is_success());
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(content_type.contains("text/plain"));
    let body = resp.text().await.unwrap();
    assert!(body.contains("cid_rpc_requests_total"));
    assert!(body.contains("cid_rpc_requests_by_method_total"));
}

#[tokio::test]
async fn health_endpoint_reports_ok() {
    let base = start_core().await;
    let body: Value = reqwest::get(format!("{}/health", base))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "cid-core");
    assert_eq!(
        body["auth_required"], false,
        "loopback dev Core needs no token"
    );
    assert!(body["connected_clients"].is_number());
}

/// Start a Core whose access policy demands a bearer token.
async fn start_authenticated_core(token: &str) -> String {
    let mut core = Core::new_in_memory().expect("core creation");
    core.set_access_policy(
        cid_core::access::AccessPolicy::new(
            "0.0.0.0".parse().unwrap(),
            Some(token.to_string()),
            vec![],
        )
        .expect("policy"),
    );
    let app = cid_core::api::router::create_router(core.app_state());
    let listener = tokio::net::TcpListener::bind::<SocketAddr>("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _core = core;
        let _ = axum::serve(listener, app.into_make_service()).await;
    });

    let base = format!("http://{}", addr);
    for _ in 0..50 {
        if reqwest::get(format!("{}/health", base)).await.is_ok() {
            return base;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("authenticated core did not become reachable");
}

#[tokio::test]
async fn protected_core_rejects_rpc_without_a_token() {
    let base = start_authenticated_core("test-token-value").await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/rpc", base))
        .json(&json!({ "jsonrpc": "2.0", "id": "1", "method": "workspace.list", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        401,
        "an unauthenticated RPC call must be refused"
    );
}

#[tokio::test]
async fn protected_core_rejects_a_wrong_token() {
    let base = start_authenticated_core("correct-token").await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/rpc", base))
        .header("Authorization", "Bearer wrong-token")
        .json(&json!({ "jsonrpc": "2.0", "id": "1", "method": "workspace.list", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn protected_core_accepts_the_right_token() {
    let base = start_authenticated_core("correct-token").await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/rpc", base))
        .header("Authorization", "Bearer correct-token")
        .json(&json!({ "jsonrpc": "2.0", "id": "1", "method": "workspace.list", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body.get("result").is_some(),
        "authorized call should succeed: {body}"
    );
}

#[tokio::test]
async fn health_stays_reachable_on_a_protected_core() {
    let base = start_authenticated_core("tok").await;
    let body: Value = reqwest::get(format!("{}/health", base))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["status"], "ok");
    assert_eq!(body["auth_required"], true);
    assert_eq!(body["loopback_only"], false);
}

#[tokio::test]
async fn sandbox_test_rpc_reports_the_boundary_held() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().to_string_lossy().to_string();

    let result = rpc_ok(&base, "sandbox.test", json!({ "worktree_path": worktree })).await;
    assert_eq!(
        result["passed"], true,
        "an Autonomous Mission must not be able to write outside its worktree: {result}"
    );
    assert!(result["reason"].as_str().unwrap_or_default().len() > 10);
}

#[tokio::test]
async fn sandbox_status_describes_what_it_actually_enforces() {
    let base = start_core().await;
    let status = rpc_ok(&base, "sandbox.status", json!({})).await;
    assert!(status["supported"].as_bool().unwrap_or(false));
    let details = status["details"].as_str().unwrap_or_default();
    assert!(!details.is_empty(), "status must describe the guarantee");
}

#[tokio::test]
async fn sandbox_network_allowlist_defaults_cover_common_remotes_and_are_editable() {
    let base = start_core().await;

    let initial = rpc_ok(&base, "sandbox.network_allowlist.get", json!({})).await;
    let hosts = initial["allowed_hosts"]
        .as_array()
        .expect("allowed_hosts array");
    let host_strs: Vec<&str> = hosts.iter().filter_map(|h| h.as_str()).collect();
    for expected in ["github.com", "registry.npmjs.org", "pypi.org", "crates.io"] {
        assert!(
            host_strs.contains(&expected),
            "default allow-list missing {expected}: {host_strs:?}"
        );
    }

    let updated = rpc_ok(
        &base,
        "sandbox.network_allowlist.set",
        json!({ "allowed_hosts": ["example.com"] }),
    )
    .await;
    let updated_hosts: Vec<&str> = updated["allowed_hosts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|h| h.as_str())
        .collect();
    assert_eq!(updated_hosts, vec!["example.com"]);

    // The change is real, not a fire-and-forget — a follow-up get must see it too.
    let refetched = rpc_ok(&base, "sandbox.network_allowlist.get", json!({})).await;
    assert_eq!(
        refetched["allowed_hosts"].as_array().unwrap().len(),
        1,
        "the edited allow-list must persist across calls, not reset to the default"
    );
}

#[tokio::test]
async fn unknown_method_returns_jsonrpc_error() {
    let base = start_core().await;
    let body = rpc(&base, "does.not.exist", json!({})).await;
    assert!(body.get("result").is_none());
    assert_eq!(body["error"]["code"], -32000);
}

#[tokio::test]
async fn workspace_is_seeded_on_first_start() {
    let base = start_core().await;
    let result = rpc_ok(&base, "workspace.list", json!({})).await;
    let workspaces = result.as_array().expect("array of workspaces");
    assert!(!workspaces.is_empty(), "a default workspace should exist");
}

#[tokio::test]
async fn acp_editors_list_returns_known_editor_ids() {
    let base = start_core().await;
    let result = rpc_ok(&base, "acp.editors.list", json!({})).await;
    let editors = result.as_array().expect("array of editors");
    assert!(
        !editors.is_empty(),
        "editor definitions are always returned"
    );

    let ids: Vec<&str> = editors.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&"zed"), "zed must be a probed editor: {ids:?}");
    assert!(
        ids.contains(&"vscode"),
        "vscode must be a probed editor: {ids:?}"
    );

    // Every editor reports availability and ACP capability regardless of install state.
    for editor in editors {
        assert!(editor["available"].is_boolean(), "available flag missing");
        assert!(
            editor["supports_acp"].is_boolean(),
            "supports_acp flag missing"
        );
    }
    let zed = editors.iter().find(|e| e["id"] == "zed").unwrap();
    assert_eq!(zed["supports_acp"], true, "Zed co-created ACP");
    let vscode = editors.iter().find(|e| e["id"] == "vscode").unwrap();
    assert_eq!(
        vscode["supports_acp"], false,
        "VS Code opens by folder, not ACP"
    );
}

#[tokio::test]
async fn acp_handoff_list_is_empty_before_any_handoff() {
    let base = start_core().await;
    let result = rpc_ok(&base, "acp.handoffs.list", json!({})).await;
    assert_eq!(result.as_array().map(|a| a.len()), Some(0));
}

#[tokio::test]
async fn acp_handoff_rejects_unknown_mission() {
    let base = start_core().await;
    let body = rpc(
        &base,
        "acp.handoff",
        json!({ "mission_id": "no-such-mission", "editor_id": "zed" }),
    )
    .await;
    assert!(
        body.get("result").is_none(),
        "unknown mission must not hand off"
    );
}

#[tokio::test]
async fn acp_take_back_requires_a_handoff_id() {
    let base = start_core().await;
    let body = rpc(&base, "acp.take_back", json!({})).await;
    let msg = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("handoff_id"),
        "error should name the missing param: {msg}"
    );
}

#[tokio::test]
async fn acp_handoff_get_reports_missing_handoff() {
    let base = start_core().await;
    let body = rpc(&base, "acp.handoff.get", json!({ "handoff_id": "nope" })).await;
    assert!(body.get("result").is_none());
}

#[tokio::test]
async fn skills_resolve_returns_a_layered_context_stack() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let repo_path = repo.path().to_string_lossy().to_string();
    std::fs::write(
        repo.path().join("AGENTS.md"),
        "# Repo rules\nAlways run cargo fmt.\n",
    )
    .unwrap();

    let result = rpc_ok(&base, "skills.resolve", json!({ "repo_path": repo_path })).await;
    let resolved = result["resolved"].as_str().expect("resolved text");
    assert!(
        resolved.contains("Always run cargo fmt"),
        "AGENTS.md content must reach the resolved stack: {resolved}"
    );

    let layers = &result["layers"];
    assert!(layers["workspace_skills"].is_array());
    assert!(layers["repo_skills"].is_array());
    assert!(layers["repo_skill_bundles"].is_array());
    assert!(layers["agents_md"].is_string());
}

#[tokio::test]
async fn skills_resolve_puts_mission_context_last() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let repo_path = repo.path().to_string_lossy().to_string();
    std::fs::write(repo.path().join("AGENTS.md"), "REPO_LAYER").unwrap();

    let result = rpc_ok(
        &base,
        "skills.resolve",
        json!({ "repo_path": repo_path, "mission_context": "MISSION_LAYER" }),
    )
    .await;
    let resolved = result["resolved"].as_str().unwrap();
    let repo_at = resolved.find("REPO_LAYER").expect("repo layer present");
    let mission_at = resolved
        .find("MISSION_LAYER")
        .expect("mission layer present");
    assert!(
        mission_at > repo_at,
        "Mission context is the most specific layer and must come last"
    );
}

#[tokio::test]
async fn skills_bundles_list_finds_multi_file_skill_md() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let repo_path = repo.path().to_string_lossy().to_string();
    let bundle_dir = repo.path().join(".cid").join("skills").join("commit-style");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    std::fs::write(
        bundle_dir.join("SKILL.md"),
        "# Commit style\nUse imperative mood.\n",
    )
    .unwrap();

    let result = rpc_ok(
        &base,
        "skills.bundles.list",
        json!({ "scope": "repo", "repo_path": repo_path }),
    )
    .await;
    let bundles = result.as_array().expect("array of bundles");
    assert!(
        bundles.iter().any(|b| b["skill_md_content"]
            .as_str()
            .unwrap_or_default()
            .contains("imperative mood")),
        "SKILL.md bundle should be discovered: {bundles:?}"
    );
}

#[tokio::test]
async fn skills_bundle_write_creates_the_file() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let target = repo.path().join("skills").join("deploy").join("SKILL.md");
    let target_str = target.to_string_lossy().to_string();

    rpc_ok(
        &base,
        "skills.bundle.write",
        json!({ "path": target_str, "content": "# Deploy checklist\n" }),
    )
    .await;

    let written = std::fs::read_to_string(&target).expect("file written");
    assert!(written.contains("Deploy checklist"));
}

#[tokio::test]
async fn skills_bundles_list_requires_repo_path() {
    let base = start_core().await;
    let body = rpc(&base, "skills.bundles.list", json!({ "scope": "repo" })).await;
    let msg = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("repo_path"),
        "error should name the missing param: {msg}"
    );
}

#[tokio::test]
async fn model_list_exposes_all_phase1_providers() {
    let base = start_core().await;
    let result = rpc_ok(&base, "model.list", json!({})).await;
    let text = result.to_string();
    for provider in ["anthropic", "openai", "google"] {
        assert!(
            text.contains(provider),
            "provider {provider} must appear in model.list: {text}"
        );
    }
}

#[tokio::test]
async fn local_runtime_list_returns_known_runtimes() {
    let base = start_core().await;
    let result = rpc_ok(&base, "local.runtime.list", json!({})).await;
    let text = result.to_string().to_lowercase();
    assert!(
        text.contains("ollama") || text.contains("lm_studio") || text.contains("llama"),
        "detection should enumerate the supported runtimes: {text}"
    );
}

fn allowlist_params(scope_id: &str, patterns: &[&str]) -> Value {
    json!({
        "scope_id": scope_id,
        "allowed_commands": patterns
            .iter()
            .map(|p| json!({ "pattern": p, "description": null, "requires_approval": false }))
            .collect::<Vec<_>>(),
        "allowed_paths": [],
        "denied_paths": [],
    })
}

#[tokio::test]
async fn autonomy_allowlist_round_trips() {
    let base = start_core().await;
    let repo = "/tmp/cid-allowlist-test";

    rpc_ok(
        &base,
        "autonomy.allowlist.set",
        allowlist_params(repo, &["cargo test", "cargo fmt"]),
    )
    .await;

    let got = rpc_ok(&base, "autonomy.allowlist.get", json!({ "scope_id": repo })).await;
    let text = got.to_string();
    assert!(
        text.contains("cargo test"),
        "allow-list should persist: {text}"
    );
}

#[tokio::test]
async fn autonomy_denies_by_default_when_no_allowlist_is_configured() {
    let base = start_core().await;
    let result = rpc_ok(
        &base,
        "autonomy.command.check",
        json!({ "repo_path": "/tmp/cid-never-configured", "command": "cargo test" }),
    )
    .await;
    assert_eq!(
        result["allowed"], false,
        "an unconfigured scope must deny, not default-allow: {result}"
    );
    assert_eq!(result["requires_approval"], true);
}

#[tokio::test]
async fn autonomy_denies_a_command_outside_the_allowlist() {
    let base = start_core().await;
    let repo = "/tmp/cid-allowlist-deny";

    rpc_ok(
        &base,
        "autonomy.allowlist.set",
        allowlist_params(repo, &["cargo test"]),
    )
    .await;

    let denied = rpc_ok(
        &base,
        "autonomy.command.check",
        json!({ "repo_path": repo, "command": "rm -rf /" }),
    )
    .await;
    assert_eq!(
        denied["allowed"], false,
        "a command outside the allow-list must be denied: {denied}"
    );

    let allowed = rpc_ok(
        &base,
        "autonomy.command.check",
        json!({ "repo_path": repo, "command": "cargo test" }),
    )
    .await;
    assert_eq!(
        allowed["allowed"], true,
        "allow-listed command should pass: {allowed}"
    );
}

#[tokio::test]
async fn context_engine_is_off_by_default_and_toggles_per_repo() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let repo_path = repo.path().to_string_lossy().to_string();

    let status = rpc_ok(
        &base,
        "context_engine.status",
        json!({ "repo_path": repo_path }),
    )
    .await;
    assert_eq!(
        status["enabled"], false,
        "Part 17: heavy features default off"
    );

    rpc_ok(
        &base,
        "context_engine.enable",
        json!({ "repo_path": repo_path }),
    )
    .await;
    let status = rpc_ok(
        &base,
        "context_engine.status",
        json!({ "repo_path": repo_path }),
    )
    .await;
    assert_eq!(status["enabled"], true);

    rpc_ok(
        &base,
        "context_engine.disable",
        json!({ "repo_path": repo_path }),
    )
    .await;
    let status = rpc_ok(
        &base,
        "context_engine.status",
        json!({ "repo_path": repo_path }),
    )
    .await;
    assert_eq!(status["enabled"], false);
}

#[tokio::test]
async fn semantic_engine_is_off_by_default() {
    let base = start_core().await;
    let repo = tempfile::tempdir().expect("tempdir");
    let repo_path = repo.path().to_string_lossy().to_string();
    let status = rpc_ok(
        &base,
        "semantic_engine.status",
        json!({ "repo_path": repo_path }),
    )
    .await;
    assert_eq!(status["enabled"], false);
}

#[tokio::test]
async fn settings_never_return_a_full_api_key() {
    let base = start_core().await;

    rpc_ok(
        &base,
        "settings.update",
        json!({
            "anthropic_api_key": "sk-ant-real-secret-value-should-never-round-trip",
            "anthropic_model": "claude-3-5-sonnet-20241022",
            "theme": "dark"
        }),
    )
    .await;

    let result = rpc_ok(&base, "settings.get", json!({})).await;
    let key = result["anthropic_api_key"]
        .as_str()
        .expect("settings.get should report a redacted key at the top level, not omit it");
    assert!(
        key.contains("...") || key == "***" || key.is_empty(),
        "settings.get must redact secrets, got {key}"
    );
    assert!(
        !key.contains("real-secret-value"),
        "the plaintext secret leaked through settings.get: {key}"
    );

    // The response must not carry a plaintext copy anywhere else in the payload —
    // this is the exact shape of the bug this test is guarding against: an earlier
    // version nested an unredacted `full_settings` object alongside the redacted one.
    let raw = result.to_string();
    assert!(
        !raw.contains("real-secret-value"),
        "settings.get response contains the plaintext secret somewhere in its payload: {raw}"
    );
}

// ---- review_prompt.md §1.3: governance.check.merge wired into github.pr.create ----
//
// `can_merge` existed and was tested at the GovernanceManager level, but
// nothing in a real call path invoked it — a workspace's merge-role policy
// could never actually block a PR. These tests cover the gate itself, not a
// real GitHub round-trip (no live token in this environment) — a request
// that gets past the gate and then fails at the real GitHub API call is
// treated as a pass here, since that failure is unrelated to governance.

#[tokio::test]
async fn opening_a_pr_without_a_session_is_refused_once_auth_is_bootstrapped() {
    let base = start_core().await;
    let _owner_token = owner_session(&base).await; // bootstraps auth for this workspace

    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();
    rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;

    let body = rpc(
        &base,
        "github.pr.create",
        json!({
            "repo_path": repo_path,
            "title": "Test PR",
            "head_branch": "feature",
            // no session_token
        }),
    )
    .await;

    let error = body
        .get("error")
        .expect("opening a PR with no session must be refused once auth is bootstrapped");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("Session") || message.contains("session"),
        "expected a session-related refusal, got: {message}"
    );
}

#[tokio::test]
async fn opening_a_pr_is_unaffected_when_auth_was_never_bootstrapped() {
    // The default, single-user, no-auth-configured golden path (Flow 1 step
    // 7: "Merge or open PR") must be unaffected by the governance gate —
    // only enforced once a Workspace has actually opted into multi-user auth.
    let base = start_core().await;

    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();
    rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;

    let body = rpc(
        &base,
        "github.pr.create",
        json!({
            "repo_path": repo_path,
            "title": "Test PR",
            "head_branch": "feature",
        }),
    )
    .await;

    // No live GitHub token in this environment, so the call still fails —
    // but it must fail at the GitHub API step, not the governance gate.
    let error = body
        .get("error")
        .expect("no GitHub token is configured, so this call should still fail somewhere");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("Session is invalid"),
        "auth was never bootstrapped, so this must not fail on a session check: {message}"
    );
}

#[tokio::test]
async fn a_developer_or_higher_session_passes_the_merge_gate() {
    let base = start_core().await;
    let token = owner_session(&base).await; // Owner satisfies the default Developer-or-higher requirement

    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();
    rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;

    let body = rpc(
        &base,
        "github.pr.create",
        json!({
            "repo_path": repo_path,
            "title": "Test PR",
            "head_branch": "feature",
            "session_token": token,
        }),
    )
    .await;

    // Still fails (no live GitHub token in this environment) — the assertion
    // is specifically that it does NOT fail on the governance/session check,
    // proving a sufficiently-privileged session passes the gate.
    let error = body
        .get("error")
        .expect("no GitHub token is configured, so this call should still fail somewhere");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        !message.contains("Session is invalid") && !message.contains("cannot merge"),
        "an Owner session should pass the merge gate cleanly: {message}"
    );
}

// ---- review_prompt.md §6: git.hunk.apply reject was whole-file, not per-hunk ----
//
// The original implementation discarded an entire file's changes on any
// hunk reject (`git checkout HEAD -- file`), silently losing every other
// hunk in that file. Fixed with a real `git apply -R` reverse patch, using
// the hunk's own header+content (sent by the client from its last
// `git.diff` response) rather than the meaningless `hunk_id` alone — a
// fresh id is minted on every `git.diff` call, so an id from one response
// can't identify anything in a later request.

#[tokio::test]
async fn rejecting_one_hunk_leaves_other_hunks_in_the_same_file_untouched() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    // Disable Windows' line-ending translation for this repo so the diff and
    // the reverse-applied patch agree on line endings — unrelated to the fix
    // under test, just test-environment hygiene.
    repo.config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();

    // A file with enough unchanged lines between two edit sites that git
    // produces two separate hunks, not one merged one (default 3-line
    // context on each side of a change).
    let original: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    std::fs::write(dir.path().join("multi.txt"), &original).unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("multi.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    // Two edits far apart: line 5 and line 35.
    let mut lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();
    lines[4] = "line 5 CHANGED".to_string();
    lines[34] = "line 35 CHANGED".to_string();
    let modified = lines.join("\n") + "\n";
    std::fs::write(dir.path().join("multi.txt"), &modified).unwrap();

    let repo_path = dir.path().to_string_lossy().to_string();
    let diff = rpc_ok(&base, "git.diff", json!({ "repo_path": repo_path })).await;
    let file = diff
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "multi.txt")
        .expect("multi.txt should appear in the diff");
    let hunks = file["hunks"].as_array().unwrap();
    assert_eq!(
        hunks.len(),
        2,
        "two edits 30 lines apart should produce two separate hunks: {hunks:?}"
    );

    // Reject only the first hunk (the "line 5" change).
    let first_hunk = &hunks[0];
    rpc_ok(
        &base,
        "git.hunk.apply",
        json!({
            "repo_path": repo_path,
            "file_path": "multi.txt",
            "hunk_id": first_hunk["id"],
            "action": "reject",
            "header": first_hunk["header"],
            "content": first_hunk["content"],
        }),
    )
    .await;

    let result = std::fs::read_to_string(dir.path().join("multi.txt")).unwrap();
    assert!(
        result.contains("line 5\n") && !result.contains("line 5 CHANGED"),
        "the rejected hunk's change must be reverted:\n{result}"
    );
    assert!(
        result.contains("line 35 CHANGED"),
        "the OTHER hunk must survive a reject of the first one — this is the exact bug: \
         a whole-file checkout would have discarded this too:\n{result}"
    );
}

#[tokio::test]
async fn git_hunk_apply_reject_without_header_and_content_is_refused() {
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    git2::Repository::init(dir.path()).unwrap();
    let repo_path = dir.path().to_string_lossy().to_string();

    let body = rpc(
        &base,
        "git.hunk.apply",
        json!({
            "repo_path": repo_path,
            "file_path": "whatever.txt",
            "hunk_id": "some-uuid-from-an-earlier-diff-call",
            "action": "reject",
            // no header/content
        }),
    )
    .await;

    assert!(
        body.get("error").is_some(),
        "a per-hunk reject with no header/content has no safe way to know what to discard, \
         and must be refused rather than silently falling back to something destructive"
    );
}

#[tokio::test]
async fn git_hunk_apply_reject_file_still_discards_the_whole_file() {
    // The explicit "Reject file" UI action (hunk_id: "all") is a real,
    // intentional whole-file discard — distinct from a per-hunk reject, and
    // must keep working exactly as before.
    let base = start_core().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();

    std::fs::write(dir.path().join("f.txt"), "original\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("f.txt")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    std::fs::write(dir.path().join("f.txt"), "changed\n").unwrap();

    let repo_path = dir.path().to_string_lossy().to_string();
    rpc_ok(
        &base,
        "git.hunk.apply",
        json!({
            "repo_path": repo_path,
            "file_path": "f.txt",
            "hunk_id": "all",
            "action": "reject",
        }),
    )
    .await;

    let result = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(result, "original\n");
}

// ---- review_prompt.md §3.1: context compaction RPC surface ----

#[tokio::test]
async fn mission_context_usage_reports_real_numbers_for_a_fresh_mission() {
    let (base, _dir, mission_id) = mission_fixture("co_pilot").await;

    let usage = rpc_ok(
        &base,
        "mission.context.usage",
        json!({ "mission_id": mission_id }),
    )
    .await;

    assert!(usage["window_tokens"].as_u64().unwrap() > 0);
    assert!(usage["used_tokens"].as_u64().unwrap() > 0);
    assert_eq!(usage["compaction_recommended"], false);
}

#[tokio::test]
async fn mission_context_compact_is_a_real_manual_trigger() {
    // Manual autonomy has no plan-approval gate (Part 5) — mission.send_message
    // still persists a user message synchronously either way, which is all
    // this test depends on; it does not wait on the backgrounded, simulated
    // assistant reply (no API key is configured in this test environment).
    let (base, _dir, mission_id) = mission_fixture("manual").await;

    // Not enough messages yet — nothing to fold away.
    let first = rpc_ok(
        &base,
        "mission.context.compact",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(first["digest"].is_null());

    // mission.send_message persists its user message synchronously before
    // returning (the model call it triggers is backgrounded) — sending more
    // than KEEP_RECENT_MESSAGES of these alone is enough to have something
    // to fold away, without depending on any background task's timing.
    for i in 0..10 {
        rpc_ok(
            &base,
            "mission.send_message",
            json!({ "mission_id": mission_id, "content": format!("message {i}") }),
        )
        .await;
    }

    let before_count = rpc_ok(&base, "message.list", json!({ "mission_id": mission_id }))
        .await
        .as_array()
        .unwrap()
        .len();

    let compacted = rpc_ok(
        &base,
        "mission.context.compact",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert!(
        !compacted["digest"].is_null(),
        "with more than KEEP_RECENT_MESSAGES messages, /compact should find something to fold away: {compacted}"
    );
    assert!(compacted["digest"]["content"]
        .as_str()
        .unwrap()
        .contains("CID context digest"));

    // The digest is additive — nothing already persisted is deleted. Checked
    // as a before/after snapshot around just the compact call itself, so a
    // still-running background model call from the send loop above can't
    // race this assertion.
    let after_count = rpc_ok(&base, "message.list", json!({ "mission_id": mission_id }))
        .await
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after_count, before_count + 1);
}

// ---- review_prompt.md §3.2: checkpoint/rewind on the Mission worktree ----

/// A worktree-mode Mission (unlike `mission_fixture`, which always uses
/// `session_mode: "shared"` — checkpointing is deliberately scoped to
/// worktree Missions only, so these tests need a real one).
async fn worktree_mission_fixture(autonomy: &str) -> (String, tempfile::TempDir, String) {
    let base = start_core().await;
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("git init");
    repo.config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();
    std::fs::write(dir.path().join("README.md"), "# fixture\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }

    let repo_path = dir.path().to_string_lossy().to_string();
    let channel = rpc_ok(&base, "repo.connect", json!({ "path": repo_path })).await;
    let channel_id = channel["id"].as_str().expect("channel id").to_string();

    let mission = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel_id,
            "title": "Worktree fixture mission",
            "task": "test",
            "session_mode": "worktree",
            "autonomy_level": autonomy,
        }),
    )
    .await;
    let mission_id = mission["id"].as_str().expect("mission id").to_string();
    (base, dir, mission_id)
}

#[tokio::test]
async fn sending_a_message_auto_checkpoints_the_worktree() {
    let (base, _dir, mission_id) = worktree_mission_fixture("manual").await;

    let before = rpc_ok(
        &base,
        "mission.checkpoint.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    assert_eq!(before.as_array().unwrap().len(), 0);

    rpc_ok(
        &base,
        "mission.send_message",
        json!({ "mission_id": mission_id, "content": "do something" }),
    )
    .await;

    let after = rpc_ok(
        &base,
        "mission.checkpoint.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    let checkpoints = after.as_array().unwrap();
    assert_eq!(
        checkpoints.len(),
        1,
        "sending a message should auto-checkpoint the worktree before this turn's work: {checkpoints:?}"
    );
    assert!(checkpoints[0]["sha"].as_str().unwrap().len() >= 7);
}

#[tokio::test]
async fn checkpoint_rewind_requires_explicit_confirmation() {
    let (base, _dir, mission_id) = worktree_mission_fixture("manual").await;
    rpc_ok(
        &base,
        "mission.send_message",
        json!({ "mission_id": mission_id, "content": "do something" }),
    )
    .await;
    let checkpoints = rpc_ok(
        &base,
        "mission.checkpoint.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    let checkpoint_id = checkpoints[0]["id"].as_str().unwrap();

    let body = rpc(
        &base,
        "mission.checkpoint.rewind",
        json!({ "mission_id": mission_id, "checkpoint_id": checkpoint_id }),
    )
    .await;
    assert!(
        body.get("error").is_some(),
        "a rewind without confirm: true must be refused, not silently applied"
    );
}

#[tokio::test]
async fn checkpoint_rewind_actually_restores_the_worktree() {
    let (base, dir, mission_id) = worktree_mission_fixture("manual").await;

    // First turn: checkpoints the clean worktree.
    rpc_ok(
        &base,
        "mission.send_message",
        json!({ "mission_id": mission_id, "content": "first turn" }),
    )
    .await;
    let checkpoints = rpc_ok(
        &base,
        "mission.checkpoint.list",
        json!({ "mission_id": mission_id }),
    )
    .await;
    let first_checkpoint_id = checkpoints[0]["id"].as_str().unwrap().to_string();

    // Find the actual worktree path CID created for this Mission.
    let mission = rpc_ok(&base, "mission.get", json!({ "id": mission_id })).await;
    let worktree_path = mission["worktree_path"]
        .as_str()
        .expect("a worktree-mode Mission must have a worktree_path")
        .to_string();

    // Simulate the agent having made a change after the checkpoint.
    std::fs::write(
        std::path::Path::new(&worktree_path).join("README.md"),
        "# fixture\n\nchanged by the agent\n",
    )
    .unwrap();
    assert!(
        std::fs::read_to_string(std::path::Path::new(&worktree_path).join("README.md"))
            .unwrap()
            .contains("changed by the agent")
    );

    let rewound = rpc_ok(
        &base,
        "mission.checkpoint.rewind",
        json!({ "mission_id": mission_id, "checkpoint_id": first_checkpoint_id, "confirm": true }),
    )
    .await;
    assert_eq!(rewound["id"], first_checkpoint_id);

    let restored =
        std::fs::read_to_string(std::path::Path::new(&worktree_path).join("README.md")).unwrap();
    assert_eq!(
        restored, "# fixture\n",
        "rewinding must actually restore the worktree's file content: {restored}"
    );

    let _ = dir; // keep the tempdir guard alive for the whole test
}

#[tokio::test]
async fn checkpoint_rewind_refuses_a_checkpoint_from_a_different_mission() {
    let (base, _dir_a, mission_a) = worktree_mission_fixture("manual").await;
    rpc_ok(
        &base,
        "mission.send_message",
        json!({ "mission_id": mission_a, "content": "turn" }),
    )
    .await;
    let checkpoints_a = rpc_ok(
        &base,
        "mission.checkpoint.list",
        json!({ "mission_id": mission_a }),
    )
    .await;
    let checkpoint_a_id = checkpoints_a[0]["id"].as_str().unwrap().to_string();

    let dir_b = tempfile::tempdir().unwrap();
    let repo_b = git2::Repository::init(dir_b.path()).unwrap();
    repo_b
        .config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();
    std::fs::write(dir_b.path().join("README.md"), "# b\n").unwrap();
    {
        let mut index = repo_b.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree = repo_b.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo_b
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    let channel_b = rpc_ok(
        &base,
        "repo.connect",
        json!({ "path": dir_b.path().to_string_lossy() }),
    )
    .await;
    let mission_b = rpc_ok(
        &base,
        "mission.create",
        json!({
            "repo_channel_id": channel_b["id"],
            "title": "Mission B",
            "task": "test",
            "session_mode": "worktree",
            "autonomy_level": "manual",
        }),
    )
    .await;
    let mission_b_id = mission_b["id"].as_str().unwrap().to_string();

    let body = rpc(
        &base,
        "mission.checkpoint.rewind",
        json!({ "mission_id": mission_b_id, "checkpoint_id": checkpoint_a_id, "confirm": true }),
    )
    .await;
    assert!(
        body.get("error").is_some(),
        "rewinding Mission B to a checkpoint that belongs to Mission A must be refused"
    );
}
