/*!
 * Local accounts, sessions, and Workspace roles (Phase 3, ADR 0013).
 *
 * Deliberately minimal: usernames and Argon2id-hashed passwords in Core's own
 * SQLite, opaque session tokens, and a fixed role ladder. No SSO, no email, no
 * password reset — see ADR 0013 for what that does and does not protect.
 *
 * This is a different concern from the transport access token in `access`:
 * that decides whether a connection may talk to Core at all, this decides who
 * the caller is and what they may do inside the Workspace.
 */

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Workspace roles, ordered by authority. Permissions derive from the role, so
/// changing someone's role takes effect everywhere at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Viewer,
    Reviewer,
    Developer,
    Admin,
    Owner,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Reviewer => "reviewer",
            Role::Developer => "developer",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "viewer" => Some(Role::Viewer),
            "reviewer" => Some(Role::Reviewer),
            "developer" => Some(Role::Developer),
            "admin" => Some(Role::Admin),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    /// Whether this role is at least as authoritative as `required`.
    pub fn satisfies(&self, required: Role) -> bool {
        *self >= required
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    pub token: String,
    pub user_id: String,
    pub username: String,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
}

/// How long a session stays valid without re-authenticating.
pub const SESSION_TTL_HOURS: i64 = 12;

const MAX_FAILED_ATTEMPTS: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

/// Per-username failed-login tracking.
///
/// The account database sits behind a port; without this it is a free offline
/// brute-force target for anyone who can reach Core.
#[derive(Default)]
struct Throttle {
    failures: HashMap<String, (u32, Instant)>,
}

impl Throttle {
    fn check(&mut self, username: &str) -> Result<()> {
        if let Some((count, last)) = self.failures.get(username) {
            if *count >= MAX_FAILED_ATTEMPTS {
                let elapsed = last.elapsed();
                if elapsed < LOCKOUT {
                    let remaining = LOCKOUT.as_secs() - elapsed.as_secs();
                    bail!("Too many failed attempts. Try again in {remaining}s.");
                }
            }
        }
        Ok(())
    }

    fn record_failure(&mut self, username: &str) {
        let entry = self
            .failures
            .entry(username.to_string())
            .or_insert((0, Instant::now()));
        // A failure after the lockout window restarts the count rather than
        // leaving the account permanently one attempt from lockout.
        if entry.0 >= MAX_FAILED_ATTEMPTS && entry.1.elapsed() >= LOCKOUT {
            *entry = (1, Instant::now());
        } else {
            entry.0 += 1;
            entry.1 = Instant::now();
        }
    }

    fn clear(&mut self, username: &str) {
        self.failures.remove(username);
    }
}

pub struct AuthManager {
    persistence: std::sync::Arc<crate::persistence::Persistence>,
    throttle: Mutex<Throttle>,
}

impl AuthManager {
    pub fn new(persistence: std::sync::Arc<crate::persistence::Persistence>) -> Self {
        Self {
            persistence,
            throttle: Mutex::new(Throttle::default()),
        }
    }

    /// Whether any account exists. Drives first-run setup in the UI.
    pub fn is_bootstrapped(&self) -> Result<bool> {
        Ok(self.persistence.count_users()? > 0)
    }

    /// Create an account. The first account created is the Owner, which avoids
    /// needing a setup wizard or a default password.
    pub fn register(&self, username: &str, password: &str, role: Option<Role>) -> Result<User> {
        let username = normalize_username(username)?;
        validate_password(password)?;

        if self.persistence.find_user_by_username(&username)?.is_some() {
            bail!("Username '{username}' is already taken");
        }

        let first_account = self.persistence.count_users()? == 0;
        let role = if first_account {
            Role::Owner
        } else {
            role.unwrap_or(Role::Developer)
        };

        let hash = hash_password(password)?;
        self.persistence.create_user(&username, &hash, role)
    }

    pub fn login(&self, username: &str, password: &str) -> Result<AuthSession> {
        let username = normalize_username(username)?;

        {
            let mut throttle = self.throttle.lock().unwrap();
            throttle.check(&username)?;
        }

        let record = self.persistence.find_user_by_username(&username)?;
        let failed = |this: &Self| {
            this.throttle.lock().unwrap().record_failure(&username);
            // One message for both "no such user" and "wrong password", so the
            // response does not reveal which usernames exist.
            anyhow::anyhow!("Invalid username or password")
        };

        let (user, hash) = match record {
            Some(r) => r,
            None => return Err(failed(self)),
        };

        if !user.active {
            return Err(anyhow::anyhow!("This account has been deactivated"));
        }
        if !verify_password(password, &hash)? {
            return Err(failed(self));
        }

        self.throttle.lock().unwrap().clear(&username);

        let token = generate_session_token();
        let expires_at = Utc::now() + chrono::Duration::hours(SESSION_TTL_HOURS);
        self.persistence
            .create_auth_session(&token, &user.id, expires_at)?;

        Ok(AuthSession {
            token,
            user_id: user.id,
            username: user.username,
            role: user.role,
            expires_at,
        })
    }

    /// Resolve a session token. Expired sessions are deleted as they are found.
    pub fn resolve_session(&self, token: &str) -> Result<Option<AuthSession>> {
        let session = match self.persistence.find_auth_session(token)? {
            Some(s) => s,
            None => return Ok(None),
        };
        if session.expires_at <= Utc::now() {
            let _ = self.persistence.delete_auth_session(token);
            return Ok(None);
        }
        Ok(Some(session))
    }

    pub fn logout(&self, token: &str) -> Result<()> {
        self.persistence.delete_auth_session(token)
    }

    pub fn list_users(&self) -> Result<Vec<User>> {
        self.persistence.list_users()
    }

    /// Change a user's role. Refuses to remove the last Owner, which would
    /// leave the Workspace with nobody able to administer it.
    pub fn set_role(&self, actor: &AuthSession, target_user_id: &str, role: Role) -> Result<User> {
        require(actor, Role::Admin, "change roles")?;

        let target = self.persistence.get_user(target_user_id)?;
        if target.role == Role::Owner && role != Role::Owner && self.count_owners()? <= 1 {
            bail!("Cannot demote the last Owner — promote another Owner first");
        }
        if role == Role::Owner && !actor.role.satisfies(Role::Owner) {
            bail!("Only an Owner can grant the Owner role");
        }
        self.persistence.set_user_role(target_user_id, role)
    }

    pub fn set_active(
        &self,
        actor: &AuthSession,
        target_user_id: &str,
        active: bool,
    ) -> Result<User> {
        require(actor, Role::Admin, "activate or deactivate accounts")?;
        let target = self.persistence.get_user(target_user_id)?;
        if !active && target.role == Role::Owner && self.count_owners()? <= 1 {
            bail!("Cannot deactivate the last Owner");
        }
        if !active {
            // Revoke live sessions so deactivation takes effect immediately
            // rather than at the end of the session TTL.
            self.persistence
                .delete_auth_sessions_for_user(target_user_id)?;
        }
        self.persistence.set_user_active(target_user_id, active)
    }

    /// Change a password. Users may change their own; an Admin may reset another's.
    pub fn change_password(
        &self,
        actor: &AuthSession,
        target_user_id: &str,
        new_password: &str,
    ) -> Result<()> {
        if actor.user_id != target_user_id {
            require(actor, Role::Admin, "reset another user's password")?;
        }
        validate_password(new_password)?;
        let hash = hash_password(new_password)?;
        self.persistence.set_user_password(target_user_id, &hash)?;
        // Every existing session for that user stops working, so a reset
        // actually locks out whoever was signed in.
        self.persistence
            .delete_auth_sessions_for_user(target_user_id)?;
        Ok(())
    }

    fn count_owners(&self) -> Result<usize> {
        Ok(self
            .persistence
            .list_users()?
            .into_iter()
            .filter(|u| u.role == Role::Owner && u.active)
            .count())
    }
}

/// Fail with a message naming the action, rather than a bare "forbidden".
pub fn require(session: &AuthSession, required: Role, action: &str) -> Result<()> {
    if session.role.satisfies(required) {
        Ok(())
    } else {
        bail!(
            "Role '{}' cannot {action}; '{}' or higher is required",
            session.role.as_str(),
            required.as_str()
        )
    }
}

fn normalize_username(username: &str) -> Result<String> {
    let trimmed = username.trim().to_ascii_lowercase();
    if trimmed.len() < 2 {
        bail!("Username must be at least 2 characters");
    }
    if trimmed.len() > 64 {
        bail!("Username must be at most 64 characters");
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!("Username may contain only letters, digits, '-', '_' and '.'");
    }
    Ok(trimmed)
}

fn validate_password(password: &str) -> Result<()> {
    if password.len() < 10 {
        bail!("Password must be at least 10 characters");
    }
    if password.len() > 1024 {
        bail!("Password must be at most 1024 characters");
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("Failed to hash password: {e}"))
}

fn verify_password(password: &str, stored_hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(stored_hash)
        .map_err(|e| anyhow::anyhow!("Stored password hash is unreadable: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

fn generate_session_token() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..48)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn manager() -> AuthManager {
        let p = Arc::new(crate::persistence::Persistence::new_in_memory().unwrap());
        AuthManager::new(p)
    }

    fn session_for(role: Role) -> AuthSession {
        AuthSession {
            token: "t".into(),
            user_id: "u".into(),
            username: "u".into(),
            role,
            expires_at: Utc::now() + chrono::Duration::hours(1),
        }
    }

    #[test]
    fn role_ladder_is_ordered() {
        assert!(Role::Owner.satisfies(Role::Admin));
        assert!(Role::Admin.satisfies(Role::Developer));
        assert!(Role::Developer.satisfies(Role::Viewer));
        assert!(!Role::Viewer.satisfies(Role::Developer));
        assert!(Role::Developer.satisfies(Role::Developer));
    }

    #[test]
    fn role_parses_round_trip() {
        for role in [
            Role::Viewer,
            Role::Reviewer,
            Role::Developer,
            Role::Admin,
            Role::Owner,
        ] {
            assert_eq!(Role::parse(role.as_str()), Some(role));
        }
        assert_eq!(Role::parse("nonsense"), None);
        assert_eq!(Role::parse("OWNER"), Some(Role::Owner));
    }

    #[test]
    fn the_first_account_becomes_owner() {
        let auth = manager();
        assert!(!auth.is_bootstrapped().unwrap());

        let first = auth
            .register("alice", "correct-horse-battery", None)
            .unwrap();
        assert_eq!(first.role, Role::Owner);
        assert!(auth.is_bootstrapped().unwrap());

        let second = auth.register("bob", "another-long-password", None).unwrap();
        assert_eq!(
            second.role,
            Role::Developer,
            "later accounts are not Owners"
        );
    }

    #[test]
    fn usernames_are_unique_and_case_insensitive() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();
        let err = auth
            .register("ALICE", "another-long-password", None)
            .unwrap_err();
        assert!(err.to_string().contains("already taken"), "{err}");
    }

    #[test]
    fn weak_passwords_and_bad_usernames_are_rejected() {
        let auth = manager();
        assert!(auth.register("alice", "short", None).is_err());
        assert!(auth.register("a", "correct-horse-battery", None).is_err());
        assert!(auth
            .register("bad name!", "correct-horse-battery", None)
            .is_err());
    }

    #[test]
    fn login_succeeds_with_the_right_password_and_yields_a_session() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();

        let session = auth.login("alice", "correct-horse-battery").unwrap();
        assert_eq!(session.username, "alice");
        assert_eq!(session.role, Role::Owner);
        assert!(session.expires_at > Utc::now());
        assert_eq!(session.token.len(), 48);

        let resolved = auth.resolve_session(&session.token).unwrap().unwrap();
        assert_eq!(resolved.user_id, session.user_id);
    }

    #[test]
    fn login_fails_identically_for_unknown_user_and_wrong_password() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();

        let wrong_password = auth
            .login("alice", "wrong-password-here")
            .unwrap_err()
            .to_string();
        let unknown_user = auth
            .login("nobody", "wrong-password-here")
            .unwrap_err()
            .to_string();
        assert_eq!(
            wrong_password, unknown_user,
            "the error must not reveal whether the username exists"
        );
    }

    #[test]
    fn repeated_failures_lock_the_account_briefly() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();

        for _ in 0..MAX_FAILED_ATTEMPTS {
            let _ = auth.login("alice", "wrong-password-here");
        }
        let err = auth.login("alice", "correct-horse-battery").unwrap_err();
        assert!(
            err.to_string().contains("Too many failed attempts"),
            "{err}"
        );
    }

    #[test]
    fn logout_invalidates_the_session() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();
        let session = auth.login("alice", "correct-horse-battery").unwrap();

        auth.logout(&session.token).unwrap();
        assert!(auth.resolve_session(&session.token).unwrap().is_none());
    }

    #[test]
    fn an_unknown_token_resolves_to_nothing() {
        let auth = manager();
        assert!(auth.resolve_session("not-a-real-token").unwrap().is_none());
    }

    #[test]
    fn admins_can_change_roles_but_developers_cannot() {
        let auth = manager();
        auth.register("owner", "correct-horse-battery", None)
            .unwrap();
        let bob = auth.register("bob", "another-long-password", None).unwrap();

        let dev = session_for(Role::Developer);
        assert!(auth.set_role(&dev, &bob.id, Role::Admin).is_err());

        let admin = session_for(Role::Admin);
        let updated = auth.set_role(&admin, &bob.id, Role::Reviewer).unwrap();
        assert_eq!(updated.role, Role::Reviewer);
    }

    #[test]
    fn only_an_owner_may_grant_the_owner_role() {
        let auth = manager();
        auth.register("owner", "correct-horse-battery", None)
            .unwrap();
        let bob = auth.register("bob", "another-long-password", None).unwrap();

        let admin = session_for(Role::Admin);
        assert!(auth.set_role(&admin, &bob.id, Role::Owner).is_err());

        let owner = session_for(Role::Owner);
        assert_eq!(
            auth.set_role(&owner, &bob.id, Role::Owner).unwrap().role,
            Role::Owner
        );
    }

    #[test]
    fn the_last_owner_cannot_be_demoted_or_deactivated() {
        let auth = manager();
        let owner_user = auth
            .register("owner", "correct-horse-battery", None)
            .unwrap();
        let owner = session_for(Role::Owner);

        let err = auth
            .set_role(&owner, &owner_user.id, Role::Developer)
            .unwrap_err();
        assert!(err.to_string().contains("last Owner"), "{err}");

        let err = auth.set_active(&owner, &owner_user.id, false).unwrap_err();
        assert!(err.to_string().contains("last Owner"), "{err}");
    }

    #[test]
    fn deactivating_a_user_revokes_their_sessions_and_blocks_login() {
        let auth = manager();
        auth.register("owner", "correct-horse-battery", None)
            .unwrap();
        let bob = auth.register("bob", "another-long-password", None).unwrap();
        let bob_session = auth.login("bob", "another-long-password").unwrap();

        let admin = session_for(Role::Admin);
        auth.set_active(&admin, &bob.id, false).unwrap();

        assert!(
            auth.resolve_session(&bob_session.token).unwrap().is_none(),
            "deactivation must take effect immediately, not at session expiry"
        );
        let err = auth.login("bob", "another-long-password").unwrap_err();
        assert!(err.to_string().contains("deactivated"), "{err}");
    }

    #[test]
    fn changing_a_password_revokes_existing_sessions() {
        let auth = manager();
        let alice = auth
            .register("alice", "correct-horse-battery", None)
            .unwrap();
        let session = auth.login("alice", "correct-horse-battery").unwrap();

        auth.change_password(&session, &alice.id, "a-brand-new-password")
            .unwrap();

        assert!(auth.resolve_session(&session.token).unwrap().is_none());
        assert!(auth.login("alice", "correct-horse-battery").is_err());
        assert!(auth.login("alice", "a-brand-new-password").is_ok());
    }

    #[test]
    fn a_developer_cannot_reset_someone_elses_password() {
        let auth = manager();
        auth.register("owner", "correct-horse-battery", None)
            .unwrap();
        let bob = auth.register("bob", "another-long-password", None).unwrap();

        let dev = session_for(Role::Developer);
        assert!(auth
            .change_password(&dev, &bob.id, "hijacked-password")
            .is_err());
    }

    #[test]
    fn passwords_are_never_stored_in_plain_text() {
        let auth = manager();
        auth.register("alice", "correct-horse-battery", None)
            .unwrap();
        let (_, hash) = auth
            .persistence
            .find_user_by_username("alice")
            .unwrap()
            .unwrap();
        assert!(
            hash.starts_with("$argon2"),
            "expected an Argon2 PHC string, got {hash}"
        );
        assert!(!hash.contains("correct-horse-battery"));
    }

    #[test]
    fn hashes_are_salted_so_identical_passwords_differ() {
        let a = hash_password("correct-horse-battery").unwrap();
        let b = hash_password("correct-horse-battery").unwrap();
        assert_ne!(a, b, "each hash must use its own salt");
        assert!(verify_password("correct-horse-battery", &a).unwrap());
        assert!(verify_password("correct-horse-battery", &b).unwrap());
    }

    #[test]
    fn require_names_the_action_it_refused() {
        let viewer = session_for(Role::Viewer);
        let err = require(&viewer, Role::Admin, "delete a Workspace").unwrap_err();
        assert!(err.to_string().contains("delete a Workspace"), "{err}");
        assert!(err.to_string().contains("admin"), "{err}");
    }
}
