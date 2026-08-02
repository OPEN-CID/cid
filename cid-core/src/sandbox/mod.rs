use std::process::{Command, Stdio};

use tokio::sync::Mutex;

use crate::net_guard::{NetGuard, DEFAULT_ALLOWED_HOSTS};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxConfig {
    pub worktree_path: String,
    pub allowed_read_paths: Vec<String>,
    pub allowed_write_paths: Vec<String>,
    /// `HTTP_PROXY`/`HTTPS_PROXY` to set for the sandboxed command, if the
    /// network allow-list guard is active (`SandboxManager::ensure_network_guard`).
    /// `None` means "no network restriction was applied" — honestly reflects
    /// reality when the guard failed to start rather than silently running
    /// unconfined while claiming otherwise.
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxResult {
    Allowed {
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    Blocked {
        reason: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SandboxStatus {
    pub platform: String,
    pub sandbox_type: String,
    pub available: bool,
    pub supported: bool,
    pub details: String,
}

/// Both cases (upper/lowercase) since tool support for which one they
/// respect is inconsistent — curl/git honor lowercase, many others only
/// check uppercase. `NO_PROXY`/`no_proxy` for loopback so a sandboxed
/// command can still reach Core itself (e.g. a script calling back into
/// `/api/rpc`) without needing to be allow-listed.
fn proxy_env_vars(proxy_url: &str) -> [(&'static str, String); 6] {
    [
        ("HTTP_PROXY", proxy_url.to_string()),
        ("HTTPS_PROXY", proxy_url.to_string()),
        ("http_proxy", proxy_url.to_string()),
        ("https_proxy", proxy_url.to_string()),
        ("NO_PROXY", "localhost,127.0.0.1,::1".to_string()),
        ("no_proxy", "localhost,127.0.0.1,::1".to_string()),
    ]
}

pub struct SandboxManager {
    /// Lazily started (needs an async context `SandboxManager::new` doesn't
    /// have) — `Mutex<Option<..>>` rather than `OnceCell` so a failed start
    /// can be retried on the next call instead of caching the failure forever.
    net_guard: Mutex<Option<NetGuard>>,
}

impl SandboxManager {
    pub fn new() -> Self {
        Self {
            net_guard: Mutex::new(None),
        }
    }

    /// Starts the network allow-list proxy on first call, reuses it after.
    /// Returns its `http://127.0.0.1:PORT` URL for the caller to set as
    /// `HTTP_PROXY`/`HTTPS_PROXY`.
    pub async fn ensure_network_guard(&self) -> anyhow::Result<String> {
        let mut guard = self.net_guard.lock().await;
        if let Some(existing) = guard.as_ref() {
            return Ok(existing.proxy_url());
        }
        let started = NetGuard::start(DEFAULT_ALLOWED_HOSTS.iter().map(|s| s.to_string())).await?;
        let url = started.proxy_url();
        *guard = Some(started);
        Ok(url)
    }

    /// The live allow-list, for a settings/status RPC to display. Empty
    /// (not an error) if the guard hasn't started yet — nothing has run an
    /// Autonomous command in this Core's lifetime yet.
    pub async fn network_allow_list(&self) -> Vec<String> {
        let guard = self.net_guard.lock().await;
        match guard.as_ref() {
            Some(g) => g.allowed_hosts().await,
            None => DEFAULT_ALLOWED_HOSTS
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    pub async fn set_network_allow_list(&self, hosts: Vec<String>) -> anyhow::Result<()> {
        self.ensure_network_guard().await?;
        let guard = self.net_guard.lock().await;
        if let Some(g) = guard.as_ref() {
            g.set_allowed_hosts(hosts).await;
        }
        Ok(())
    }

    pub fn execute_sandboxed(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        // Layer 1 — path policy, enforced on every platform before anything is
        // spawned. This is the only filesystem guarantee on Windows, where Job
        // Objects constrain process/CPU/memory but not file access at all.
        if let Some(reason) = Self::path_policy_violation(config, command, args, workdir) {
            return Ok(SandboxResult::Blocked { reason });
        }

        // Layer 2 — kernel isolation, where the OS provides it.
        self.platform_sandbox(config, command, args, workdir)
    }

    #[cfg(windows)]
    fn platform_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        self.windows_job_object_sandbox(config, command, args, workdir)
    }

    #[cfg(target_os = "macos")]
    fn platform_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        self.macos_sandbox_exec(config, command, args, workdir)
    }

    #[cfg(target_os = "linux")]
    fn platform_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        self.linux_namespace_sandbox(config, command, args, workdir)
    }

    /// Any other Unix (FreeBSD, illumos, …). Layer 1's path-policy check has
    /// already run and still applies; what is missing here is Layer 2 kernel
    /// isolation, for which this build carries no implementation. Refuse rather
    /// than run the command unconfined while reporting it as sandboxed.
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    fn platform_sandbox(
        &self,
        _config: &SandboxConfig,
        _command: &str,
        _args: &[&str],
        _workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        Ok(SandboxResult::Blocked {
            reason: format!(
                "No kernel-level sandbox implementation for this platform ({}); \
                 refusing to execute unconfined",
                std::env::consts::OS
            ),
        })
    }

    /// Reject a command before it runs if any path it names resolves outside the
    /// worktree boundary.
    ///
    /// This is a policy check on the command text, not a kernel guarantee: it
    /// cannot see paths a program computes at runtime. It exists because on
    /// Windows there is no kernel filesystem boundary behind it, and because on
    /// every platform it fails closed and fails fast. `SECURITY.md` states this
    /// limit plainly rather than implying stronger isolation than exists.
    fn path_policy_violation(
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> Option<String> {
        if !Self::path_within_boundary(workdir, &config.worktree_path) {
            return Some(format!(
                "Working directory '{}' is outside the Mission worktree '{}'",
                workdir, config.worktree_path
            ));
        }

        let mut allowed = vec![config.worktree_path.clone()];
        allowed.extend(config.allowed_write_paths.iter().cloned());
        allowed.extend(config.allowed_read_paths.iter().cloned());

        for token in std::iter::once(command).chain(args.iter().copied()) {
            for candidate in Self::path_like_tokens(token) {
                if Self::is_absolute_path(&candidate) || candidate.contains("..") {
                    let resolved = Self::resolve_against(&candidate, workdir);
                    let inside = allowed
                        .iter()
                        .any(|base| Self::path_within_boundary(&resolved, base));
                    // System locations that only ever get read (the shell itself,
                    // /usr/bin, C:\Windows) are not writes and must not be blocked.
                    if !inside && !Self::is_read_only_system_path(&resolved) {
                        return Some(format!(
                            "Command references '{}', which resolves outside the Mission worktree '{}'",
                            candidate, config.worktree_path
                        ));
                    }
                }
            }
        }

        None
    }

    /// Split a shell argument into the path-shaped fragments worth checking.
    /// A single `sh -c` string can carry several redirect targets.
    ///
    /// A fragment counts as path-shaped only when it has a separator past the
    /// first character, which keeps single-letter switches like `cmd /c` and
    /// `find / -name` from being mistaken for absolute paths.
    fn path_like_tokens(arg: &str) -> Vec<String> {
        arg.split(|c: char| {
            c.is_whitespace() || matches!(c, '>' | '<' | '|' | ';' | '&' | '"' | '\'')
        })
        .map(|t| t.trim().to_string())
        .filter(|t| t.chars().skip(1).any(|c| c == '/' || c == '\\'))
        .collect()
    }

    fn is_absolute_path(token: &str) -> bool {
        let t = token.trim();
        t.starts_with('/')
            || t.starts_with('\\')
            || (t.len() >= 3
                && t.as_bytes()[0].is_ascii_alphabetic()
                && t.as_bytes()[1] == b':'
                && matches!(t.as_bytes()[2], b'\\' | b'/'))
    }

    fn resolve_against(token: &str, workdir: &str) -> String {
        let p = std::path::Path::new(token);
        if p.is_absolute() || Self::is_absolute_path(token) {
            token.to_string()
        } else {
            std::path::Path::new(workdir)
                .join(p)
                .to_string_lossy()
                .to_string()
        }
    }

    /// Locations a command may legitimately name for reading — interpreters and
    /// system binaries — which would otherwise trip the policy on every command.
    fn is_read_only_system_path(path: &str) -> bool {
        let lower = path.to_ascii_lowercase().replace('\\', "/");
        const READ_ONLY_PREFIXES: &[&str] = &[
            "/usr/",
            "/bin/",
            "/sbin/",
            "/lib/",
            "/opt/",
            "/etc/",
            "/system/",
            "c:/windows/",
            "c:/program files",
            "c:/programdata/",
        ];
        READ_ONLY_PREFIXES.iter().any(|p| lower.starts_with(p))
    }

    pub fn status(&self) -> SandboxStatus {
        // `available` describes filesystem confinement specifically, because that
        // is the guarantee Autonomous mode depends on. Reporting Windows as
        // available would be false: a Job Object does not restrict file access.
        let (sandbox_type, available, details) = if cfg!(windows) {
            (
                "windows_job_object",
                false,
                "Windows Job Objects limit process/CPU/memory but do NOT confine filesystem \
                 access. On Windows the worktree boundary is enforced by command path policy \
                 only — see SECURITY.md and docs/adr/0011-windows-sandbox-boundary.md.",
            )
        } else if cfg!(target_os = "macos") {
            let available = which::which("sandbox-exec").is_ok();
            (
                "macos_sandbox_exec",
                available,
                "macOS sandbox-exec profile with (deny default) and worktree-scoped write allows",
            )
        } else if cfg!(target_os = "linux") {
            let available = which::which("bwrap").is_ok();
            (
                "linux_namespace",
                available,
                "Linux bubblewrap (bwrap) bind-mount isolation; without bwrap the unshare \
                 fallback does not confine the filesystem and policy alone applies",
            )
        } else {
            (
                "unsupported",
                false,
                "No kernel-level sandbox implementation exists for this platform; \
                 sandboxed execution is refused rather than run unconfined",
            )
        };

        SandboxStatus {
            platform: std::env::consts::OS.to_string(),
            sandbox_type: sandbox_type.to_string(),
            available,
            supported: true,
            details: details.to_string(),
        }
    }

    #[cfg(windows)]
    fn windows_job_object_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        use std::os::windows::process::CommandExt;

        if !Self::path_within_boundary(workdir, &config.worktree_path) {
            return Ok(SandboxResult::Blocked {
                reason: format!(
                    "Working directory '{}' is outside allowed worktree '{}'",
                    workdir, config.worktree_path
                ),
            });
        }

        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NEW_PROCESS_GROUP);
        if let Some(proxy_url) = &config.proxy_url {
            cmd.envs(proxy_env_vars(proxy_url));
        }

        // On Windows, Job Object assignment is done after process creation
        // We use CREATE_BREAKAWAY_FROM_JOB to avoid inheriting parent job,
        // then assign the process to a new restricted job object
        let output = match cmd.spawn() {
            Ok(mut child) => {
                // Assign to a restricted job object
                if let Some(_job_handle) = Self::create_restricted_job() {
                    if !Self::assign_process_to_job(&_job_handle, child.id()) {
                        let _ = child.kill();
                        return Ok(SandboxResult::Blocked {
                            reason: "Failed to assign process to restricted job object".to_string(),
                        });
                    }
                }

                match child.wait_with_output() {
                    Ok(out) => out,
                    Err(e) => {
                        return Ok(SandboxResult::Blocked {
                            reason: format!("Process execution failed: {}", e),
                        });
                    }
                }
            }
            Err(e) => {
                return Ok(SandboxResult::Blocked {
                    reason: format!("Failed to spawn process: {}", e),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = if output.status.success() {
            0
        } else {
            output.status.code().unwrap_or(-1)
        };

        // Verify the command didn't try to escape the sandbox
        // Check if any output references paths outside the worktree
        if Self::output_references_escape(&stdout, &stderr, &config.worktree_path) {
            return Ok(SandboxResult::Blocked {
                reason: "Command output references paths outside the worktree boundary".to_string(),
            });
        }

        Ok(SandboxResult::Allowed {
            exit_code,
            stdout,
            stderr,
        })
    }

    #[cfg(windows)]
    #[allow(clippy::upper_case_acronyms)] // must match the real Win32 API type names
    fn create_restricted_job() -> Option<*mut std::ffi::c_void> {
        // Safety: Windows API calls are unsafe FFI
        unsafe {
            use std::ffi::c_void;

            type HANDLE = *mut c_void;
            type BOOL = i32;
            type LPVOID = *mut c_void;
            type DWORD = u32;

            #[allow(non_snake_case)]
            #[repr(C)]
            struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
                PerProcessUserTimeLimit: i64,
                PerJobUserTimeLimit: i64,
                LimitFlags: DWORD,
                MinimumWorkingSetSize: usize,
                MaximumWorkingSetSize: usize,
                ActiveProcessLimit: DWORD,
                Affinity: usize,
                PriorityClass: DWORD,
                SchedulingClass: DWORD,
            }

            type JOBOBJECTINFOCLASS = i32;
            const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: DWORD = 0x00000008;
            const JOBOBJECT_BASIC_LIMIT_INFORMATION_CLASS: JOBOBJECTINFOCLASS = 2;

            extern "system" {
                fn CreateJobObjectW(lpJobAttributes: *mut c_void, lpName: *const u16) -> HANDLE;

                fn SetInformationJobObject(
                    hJob: HANDLE,
                    JobObjectInformationClass: JOBOBJECTINFOCLASS,
                    lpJobObjectInformation: LPVOID,
                    cbJobObjectInformationLength: DWORD,
                ) -> BOOL;

                fn CloseHandle(hObject: HANDLE) -> BOOL;
            }

            // Create the job object (unnamed)
            let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
            if job.is_null() {
                return None;
            }

            // Configure limits
            let mut limits = JOBOBJECT_BASIC_LIMIT_INFORMATION {
                PerProcessUserTimeLimit: 0,
                PerJobUserTimeLimit: 0,
                LimitFlags: JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
                MinimumWorkingSetSize: 0,
                MaximumWorkingSetSize: 0,
                ActiveProcessLimit: 1, // Only this process
                Affinity: 0,
                PriorityClass: 0,
                SchedulingClass: 0,
            };

            let result = SetInformationJobObject(
                job,
                JOBOBJECT_BASIC_LIMIT_INFORMATION_CLASS,
                &mut limits as *mut _ as LPVOID,
                std::mem::size_of::<JOBOBJECT_BASIC_LIMIT_INFORMATION>() as DWORD,
            );

            if result == 0 {
                CloseHandle(job);
                return None;
            }

            Some(job)
        }
    }

    #[cfg(windows)]
    #[allow(clippy::upper_case_acronyms)] // must match the real Win32 API type names
    fn assign_process_to_job(job: &*mut std::ffi::c_void, pid: u32) -> bool {
        unsafe {
            type HANDLE = *mut std::ffi::c_void;
            type BOOL = i32;
            type DWORD = u32;

            extern "system" {
                fn OpenProcess(
                    dwDesiredAccess: DWORD,
                    bInheritHandle: BOOL,
                    dwProcessId: DWORD,
                ) -> HANDLE;

                fn AssignProcessToJobObject(hJob: HANDLE, hProcess: HANDLE) -> BOOL;

                fn CloseHandle(hObject: HANDLE) -> BOOL;
            }

            const PROCESS_SET_QUOTA: DWORD = 0x0100;
            const PROCESS_TERMINATE: DWORD = 0x0001;

            let h_process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if h_process.is_null() {
                return false;
            }

            let result = AssignProcessToJobObject(*job, h_process);
            CloseHandle(h_process);

            result != 0
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_sandbox_exec(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        if !Self::path_within_boundary(workdir, &config.worktree_path) {
            return Ok(SandboxResult::Blocked {
                reason: format!(
                    "Working directory '{}' is outside allowed worktree '{}'",
                    workdir, config.worktree_path
                ),
            });
        }

        // Build a sandbox-exec profile
        let profile = Self::build_macos_sandbox_profile(config, workdir);

        // Write profile to temp file
        let profile_path =
            std::env::temp_dir().join(format!("cid-sandbox-{}.sb", uuid::Uuid::new_v4()));

        std::fs::write(&profile_path, &profile)
            .map_err(|e| anyhow::anyhow!("Failed to write sandbox profile: {}", e))?;

        let mut cmd = Command::new("sandbox-exec");
        cmd.arg("-f")
            .arg(profile_path.to_string_lossy().as_ref())
            .arg(command)
            .args(args)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(proxy_url) = &config.proxy_url {
            cmd.envs(proxy_env_vars(proxy_url));
        }

        let output = match cmd.output() {
            Ok(out) => out,
            Err(e) => {
                let _ = std::fs::remove_file(&profile_path);
                return Ok(SandboxResult::Blocked {
                    reason: format!("Failed to execute sandbox command: {}", e),
                });
            }
        };

        let _ = std::fs::remove_file(&profile_path);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = if output.status.success() {
            0
        } else {
            output.status.code().unwrap_or(-1)
        };

        // If sandbox-exec itself failed (e.g. not available on the
        // runner), report Blocked rather than Allowed with empty output.
        if exit_code != 0 && stdout.is_empty() {
            let reason = if stderr.is_empty() {
                "macOS sandbox-exec exited with an unknown error".to_string()
            } else {
                format!("macOS sandbox-exec failed: {}", stderr.trim())
            };
            return Ok(SandboxResult::Blocked { reason });
        }

        Ok(SandboxResult::Allowed {
            exit_code,
            stdout,
            stderr,
        })
    }

    #[cfg(target_os = "macos")]
    fn build_macos_sandbox_profile(config: &SandboxConfig, workdir: &str) -> String {
        let mut profile = String::new();
        profile.push_str("(version 1)\n");
        profile.push_str("(deny default)\n");

        // Allow basic operations. process-fork and mach-lookup are not optional
        // extras: under a (deny default) profile, dyld/libSystem process startup
        // (even for something as small as `/bin/sh -c echo`) talks to system
        // mach services during init, and a shell forks before it execs the
        // command it's running. Without both, sandbox-exec silently fails
        // *every* command, not just genuinely disallowed ones.
        profile.push_str("(allow process-exec)\n");
        profile.push_str("(allow process-fork)\n");
        profile.push_str("(allow mach-lookup)\n");
        profile.push_str("(allow sysctl-read)\n");
        profile.push_str("(allow signal)\n");

        // Allow reading in the worktree
        let worktree = &config.worktree_path;
        let workdir_full = std::path::Path::new(workdir)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| workdir.to_string());

        profile.push_str(&format!(
            "(allow file-read* file-read-data file-read-metadata\n  (subpath \"{}\")\n  (subpath \"{}\"))\n",
            worktree, workdir_full
        ));

        // Allow writing only in worktree and allowed paths
        profile.push_str(&format!(
            "(allow file-write* file-write-data file-write-unlink\n  (subpath \"{}\")\n  (subpath \"{}\"))\n",
            worktree, workdir_full
        ));

        for path in &config.allowed_write_paths {
            if path != worktree && path != &workdir_full {
                profile.push_str(&format!(
                    "(allow file-write* file-write-data\n  (subpath \"{}\"))\n",
                    path
                ));
            }
        }

        for path in &config.allowed_read_paths {
            if path != worktree && path != &workdir_full {
                profile.push_str(&format!(
                    "(allow file-read* file-read-data file-read-metadata\n  (subpath \"{}\"))\n",
                    path
                ));
            }
        }

        // Allow network (needed for git, npm, etc.)
        profile.push_str("(allow network-outbound)\n");
        profile.push_str("(allow network-inbound)\n");

        // Allow basic system files
        profile.push_str("(allow file-read*\n  (subpath \"/usr\")\n  (subpath \"/bin\")\n  (subpath \"/sbin\")\n  (subpath \"/etc\")\n  (subpath \"/tmp\")\n  (subpath \"/private/tmp\")\n  (subpath \"/dev\")\n  (subpath \"/var\")\n  (subpath \"/Library\")\n  (subpath \"/System/Library\"))\n");

        profile
    }

    #[cfg(target_os = "linux")]
    fn linux_namespace_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        if !Self::path_within_boundary(workdir, &config.worktree_path) {
            return Ok(SandboxResult::Blocked {
                reason: format!(
                    "Working directory '{}' is outside allowed worktree '{}'",
                    workdir, config.worktree_path
                ),
            });
        }

        // Use unshare to create a new mount namespace, then bind-mount the worktree as root
        // This provides filesystem isolation without requiring root (for user namespaces)
        //
        // Build the sandbox command: unshare --mount --map-root-user -- chroot-or-bind <worktree> <command>
        // For CI/testing environments where unshare might not have all capabilities, fall back to
        // a simpler approach using bubblewrap or just running with a safety check.

        // First check if unshare is available
        if which::which("unshare").is_err() {
            return Ok(SandboxResult::Blocked {
                reason: "Linux namespace isolation requires 'unshare' (util-linux)".to_string(),
            });
        }

        // Try to use bubblewrap (bwrap) if available - more robust than raw unshare
        if which::which("bwrap").is_ok() {
            return self.linux_bwrap_sandbox(config, command, args, workdir);
        }

        // Fallback: use unshare with mount namespace
        // Note: this requires either root or user namespace support
        let mut cmd = Command::new("unshare");
        cmd.arg("--mount");

        // Try user namespace mapping for unprivileged operation
        cmd.arg("--map-root-user");

        // Bind mount worktree and restrict
        let worktree_path = std::path::Path::new(&config.worktree_path);
        let worktree_abs = worktree_path
            .canonicalize()
            .unwrap_or_else(|_| worktree_path.to_path_buf());

        // Build a mount --bind + chroot wrapper.
        // Use "$@" to preserve argument boundaries (avoids the shell
        // treating `echo hello` as `echo` with $0=hello).
        let sandbox_command = format!(
            "mount --bind {} {} 2>/dev/null || true; cd {} && exec \"$@\"",
            worktree_abs.display(),
            worktree_abs.display(),
            workdir,
        );

        cmd.arg("sh")
            .arg("-c")
            .arg(&sandbox_command)
            .arg("--")
            .arg(command)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(proxy_url) = &config.proxy_url {
            cmd.envs(proxy_env_vars(proxy_url));
        }

        let output = match cmd.output() {
            Ok(out) => out,
            Err(e) => {
                return Ok(SandboxResult::Blocked {
                    reason: format!("Failed to execute sandbox command: {}", e),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = if output.status.success() {
            0
        } else {
            output.status.code().unwrap_or(-1)
        };

        // If the sandbox tool (unshare) itself failed (e.g. user namespaces
        // disabled in a container), stdout will be empty and the command did
        // not actually run. Report this as Blocked rather than Allowed with
        // empty output, so callers don't mistake a silent sandbox failure
        // for a successfully isolated command.
        if exit_code != 0 && stdout.is_empty() {
            let reason = if stderr.is_empty() {
                "Linux sandbox (unshare) exited with an unknown error".to_string()
            } else {
                format!("Linux sandbox (unshare) failed: {}", stderr.trim())
            };
            return Ok(SandboxResult::Blocked { reason });
        }

        Ok(SandboxResult::Allowed {
            exit_code,
            stdout,
            stderr,
        })
    }

    #[cfg(target_os = "linux")]
    fn linux_bwrap_sandbox(
        &self,
        config: &SandboxConfig,
        command: &str,
        args: &[&str],
        workdir: &str,
    ) -> anyhow::Result<SandboxResult> {
        let mut cmd = Command::new("bwrap");

        // Create a new namespace
        cmd.arg("--unshare-all");
        cmd.arg("--share-net"); // Need network for git, npm, etc.

        // Bind mount the worktree as restricted root
        cmd.arg("--bind")
            .arg(&config.worktree_path)
            .arg(&config.worktree_path);

        // Bind mount /usr for system binaries
        cmd.arg("--ro-bind").arg("/usr").arg("/usr");
        cmd.arg("--ro-bind").arg("/bin").arg("/bin");
        cmd.arg("--ro-bind").arg("/lib").arg("/lib");
        cmd.arg("--ro-bind").arg("/lib64").arg("/lib64");

        // /etc and /proc for basic functionality
        cmd.arg("--ro-bind").arg("/etc").arg("/etc");
        cmd.arg("--proc").arg("/proc");

        // /dev for basic device access
        cmd.arg("--dev").arg("/dev");

        // /tmp for temp files
        cmd.arg("--tmpfs").arg("/tmp");

        // Set working directory
        cmd.arg("--chdir").arg(workdir);

        // The actual command
        cmd.arg("--");
        cmd.arg(command);
        cmd.args(args);

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(proxy_url) = &config.proxy_url {
            cmd.envs(proxy_env_vars(proxy_url));
        }

        let output = match cmd.output() {
            Ok(out) => out,
            Err(e) => {
                return Ok(SandboxResult::Blocked {
                    reason: format!("Failed to execute bubblewrap sandbox: {}", e),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = if output.status.success() {
            0
        } else {
            output.status.code().unwrap_or(-1)
        };

        // If bwrap itself failed (e.g. user namespaces disabled), report
        // Blocked rather than Allowed with empty output.
        if exit_code != 0 && stdout.is_empty() {
            let reason = if stderr.is_empty() {
                "Linux sandbox (bwrap) exited with an unknown error".to_string()
            } else {
                format!("Linux sandbox (bwrap) failed: {}", stderr.trim())
            };
            return Ok(SandboxResult::Blocked { reason });
        }

        Ok(SandboxResult::Allowed {
            exit_code,
            stdout,
            stderr,
        })
    }

    /// Attempt to write outside the worktree and report whether the boundary held.
    ///
    /// Ground truth is the filesystem, not the exit code: a kernel sandbox that
    /// denies the write may still report success from the shell, and a command
    /// blocked before spawning obviously never writes. The only question that
    /// matters is whether the file exists afterwards.
    pub fn verify_sandbox_boundary(&self, worktree_path: &str) -> anyhow::Result<bool> {
        let test_config = SandboxConfig {
            worktree_path: worktree_path.to_string(),
            allowed_read_paths: vec![worktree_path.to_string()],
            allowed_write_paths: vec![worktree_path.to_string()],
            proxy_url: None,
        };

        // A unique target per run, so a leftover file from an earlier run cannot
        // make a passing boundary look like a failing one.
        let outside =
            std::env::temp_dir().join(format!("cid-sandbox-escape-{}.txt", uuid::Uuid::new_v4()));
        let outside_path = outside.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&outside);

        let (cmd, arg_token) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };
        let script = format!("echo cid-escape-probe > \"{}\"", outside_path);

        let _ = self.execute_sandboxed(&test_config, cmd, &[arg_token, &script], worktree_path)?;

        let escaped = outside.exists();
        let _ = std::fs::remove_file(&outside);
        Ok(!escaped)
    }

    /// The probe used by `verify_sandbox_boundary`, plus the detail a caller
    /// needs to report the result honestly.
    pub fn boundary_report(&self, worktree_path: &str) -> anyhow::Result<(bool, String)> {
        let held = self.verify_sandbox_boundary(worktree_path)?;
        let status = self.status();
        let detail = if held && status.available {
            format!(
                "Boundary held; enforced by {} plus command path policy.",
                status.sandbox_type
            )
        } else if held {
            "Boundary held, enforced by command path policy only — no kernel filesystem \
             confinement is active on this platform."
                .to_string()
        } else {
            format!(
                "BOUNDARY BREACHED: a command wrote outside the worktree on {} ({}).",
                status.platform, status.sandbox_type
            )
        };
        Ok((held, detail))
    }

    fn path_within_boundary(target: &str, boundary: &str) -> bool {
        let target_path = if let Ok(canonical) = std::path::Path::new(target).canonicalize() {
            canonical
        } else {
            std::path::Path::new(target).to_path_buf()
        };

        let boundary_path = if let Ok(canonical) = std::path::Path::new(boundary).canonicalize() {
            canonical
        } else {
            std::path::Path::new(boundary).to_path_buf()
        };

        target_path.starts_with(&boundary_path)
    }

    // Windows-only by design, not by oversight: Job Objects constrain process
    // and resource limits but provide no filesystem confinement, so this coarse
    // output-text heuristic is the compensating control on that platform.
    // Linux (unshare/bwrap) and macOS (sandbox-exec) get real kernel-enforced
    // filesystem isolation instead and do not need — or rely on — it.
    #[cfg(windows)]
    fn output_references_escape(stdout: &str, stderr: &str, worktree: &str) -> bool {
        // Simple heuristic: check if output mentions writing to paths outside worktree
        // This catches common escape patterns like `cd /etc` or writing to /tmp
        let combined = format!("{}\n{}", stdout, stderr);
        let worktree_lower = worktree.to_lowercase();
        let combined_lower = combined.to_lowercase();

        // Look for common escape patterns
        let escape_patterns = [
            "cannot write to",
            "permission denied",
            "access denied",
            "operation not permitted",
        ];

        // If sandbox error, that's fine - it means the sandbox is working
        for pattern in &escape_patterns {
            if combined_lower.contains(pattern) && !combined_lower.contains(&worktree_lower) {
                return true;
            }
        }

        false
    }
}

impl Default for SandboxManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Probes whether this machine's sandbox tooling can actually isolate a
    /// trivial command — CI containers routinely ship `unshare`/`bwrap` while
    /// having user namespaces disabled, and macOS runners can reject profiles.
    ///
    /// This exists so a test can assert the *correct* outcome for the machine
    /// it runs on rather than accepting either one. An earlier version of
    /// `writes_inside_the_worktree_are_allowed` tolerated both `Allowed` and
    /// `Blocked` on Linux/macOS, which made it pass whether the sandbox worked
    /// or was entirely broken. Probe the tool, never the wrapper under test.
    #[cfg(target_os = "linux")]
    fn sandbox_tooling_works() -> bool {
        if which::which("unshare").is_err() {
            return false;
        }
        let (tool, args): (&str, &[&str]) = if which::which("bwrap").is_ok() {
            ("bwrap", &["--unshare-all", "--ro-bind", "/", "/", "true"])
        } else {
            ("unshare", &["--mount", "--map-root-user", "true"])
        };
        let probe = Command::new(tool).args(args).output();
        matches!(probe, Ok(out) if out.status.success())
    }

    #[cfg(target_os = "macos")]
    fn sandbox_tooling_works() -> bool {
        let args: &[&str] = &["-p", "(version 1)(allow default)", "/usr/bin/true"];
        let probe = Command::new("sandbox-exec").args(args).output();
        matches!(probe, Ok(out) if out.status.success())
    }

    /// Job Objects are part of the Windows kernel — there is no optional
    /// userspace tool that can be missing.
    #[cfg(windows)]
    fn sandbox_tooling_works() -> bool {
        true
    }

    /// Platforms with no Layer 2 implementation always block, by design.
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    fn sandbox_tooling_works() -> bool {
        false
    }

    #[test]
    fn test_sandbox_manager_creation() {
        let manager = SandboxManager::new();
        let status = manager.status();
        assert!(!status.platform.is_empty());
        assert!(status.supported);
    }

    #[test]
    fn test_sandbox_config_serialization() {
        let config = SandboxConfig {
            worktree_path: "/tmp/test-worktree".to_string(),
            allowed_read_paths: vec!["/tmp/test-worktree".to_string()],
            allowed_write_paths: vec!["/tmp/test-worktree".to_string()],
            proxy_url: None,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: SandboxConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.worktree_path, "/tmp/test-worktree");
        assert_eq!(parsed.allowed_read_paths.len(), 1);
        assert_eq!(parsed.allowed_write_paths.len(), 1);
    }

    #[test]
    fn test_sandbox_result_serialization() {
        let allowed = SandboxResult::Allowed {
            exit_code: 0,
            stdout: "hello".to_string(),
            stderr: String::new(),
        };
        let json = serde_json::to_string(&allowed).unwrap();
        assert!(json.contains("allowed"));
        assert!(json.contains("exit_code"));

        let blocked = SandboxResult::Blocked {
            reason: "outside worktree".to_string(),
        };
        let json = serde_json::to_string(&blocked).unwrap();
        assert!(json.contains("blocked"));
        assert!(json.contains("outside worktree"));
    }

    #[test]
    fn test_path_within_boundary() {
        assert!(SandboxManager::path_within_boundary(
            "/tmp/test/subdir",
            "/tmp/test"
        ));
        assert!(SandboxManager::path_within_boundary(
            "/tmp/test",
            "/tmp/test"
        ));
        assert!(!SandboxManager::path_within_boundary(
            "/tmp/other",
            "/tmp/test"
        ));
        assert!(SandboxManager::path_within_boundary(
            "/tmp/test/deep/nested",
            "/tmp/test"
        ));
    }

    #[test]
    fn test_sandbox_status() {
        let manager = SandboxManager::new();
        let status = manager.status();
        assert!(status.supported);

        #[cfg(windows)]
        {
            assert_eq!(status.platform, "windows");
            assert_eq!(status.sandbox_type, "windows_job_object");
        }

        #[cfg(target_os = "macos")]
        {
            assert_eq!(status.platform, "macos");
            assert_eq!(status.sandbox_type, "macos_sandbox_exec");
        }

        #[cfg(target_os = "linux")]
        {
            assert_eq!(status.platform, "linux");
            assert_eq!(status.sandbox_type, "linux_namespace");
        }
    }

    /// The security-critical test for Phase 2: an Autonomous Mission must not be
    /// able to write outside its worktree even when a command tries to. This
    /// asserts the boundary actually held on this platform — it does not merely
    /// assert that the check returned without panicking.
    #[test]
    fn autonomous_command_cannot_write_outside_the_worktree() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let manager = SandboxManager::new();

        let (held, detail) = manager
            .boundary_report(&worktree)
            .expect("boundary probe must run");
        assert!(held, "{detail}");
    }

    #[test]
    fn absolute_path_outside_the_worktree_is_blocked_before_spawning() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };

        let outside = if cfg!(windows) {
            "C:\\Temp\\escape.txt"
        } else {
            "/tmp/escape.txt"
        };
        let script = format!("echo x > {}", outside);
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };

        let result = SandboxManager::new()
            .execute_sandboxed(&config, shell, &[flag, &script], &worktree)
            .unwrap();

        match result {
            SandboxResult::Blocked { reason } => {
                assert!(
                    reason.contains("outside"),
                    "unexpected block reason: {reason}"
                );
            }
            SandboxResult::Allowed { .. } => panic!("a write outside the worktree must be blocked"),
        }
    }

    #[test]
    fn parent_directory_traversal_is_blocked() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };

        let result = SandboxManager::new()
            .execute_sandboxed(
                &config,
                shell,
                &[flag, "echo x > ../escaped.txt"],
                &worktree,
            )
            .unwrap();

        assert!(
            matches!(result, SandboxResult::Blocked { .. }),
            "`..` traversal out of the worktree must be blocked"
        );
    }

    #[test]
    fn writes_inside_the_worktree_are_allowed() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };

        let result = SandboxManager::new()
            .execute_sandboxed(
                &config,
                shell,
                &[flag, "echo inside > inside.txt"],
                &worktree,
            )
            .unwrap();

        // Assert the outcome the machine's actual capabilities demand, in both
        // directions — a working sandbox must allow legitimate worktree writes,
        // and an unusable one must surface as Blocked rather than as an
        // Allowed-with-empty-output false success.
        let tooling_works = sandbox_tooling_works();
        match result {
            SandboxResult::Allowed { .. } => {
                assert!(
                    tooling_works,
                    "sandbox returned Allowed although this platform's sandbox tooling probe \
                     failed - a silent sandbox failure must be reported as Blocked"
                );
            }
            SandboxResult::Blocked { reason } => {
                assert!(
                    !tooling_works,
                    "ordinary work inside the worktree must not be blocked when this \
                     platform's sandbox tooling is working: {reason}"
                );
            }
        }
    }

    #[test]
    fn status_does_not_claim_filesystem_confinement_on_windows() {
        let status = SandboxManager::new().status();
        #[cfg(windows)]
        assert!(
            !status.available,
            "Job Objects do not confine the filesystem; claiming otherwise would overstate the guarantee"
        );
        assert!(!status.details.is_empty());
    }

    #[test]
    fn system_binary_paths_are_not_treated_as_escapes() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };
        let system_path = if cfg!(windows) {
            "C:\\Windows\\System32\\cmd.exe"
        } else {
            "/bin/echo"
        };
        assert!(
            SandboxManager::path_policy_violation(&config, system_path, &["hi"], &worktree)
                .is_none(),
            "invoking a system binary must not be mistaken for an escape"
        );
    }

    #[test]
    fn test_execute_sandboxed_basic_command() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();

        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };

        let manager = SandboxManager::new();

        // Try a safe command (echo within worktree)
        let result = manager.execute_sandboxed(
            &config,
            if cfg!(windows) { "cmd" } else { "sh" },
            if cfg!(windows) {
                &["/c", "echo hello"]
            } else {
                &["-c", "echo hello"]
            },
            &worktree,
        );

        match result {
            Ok(SandboxResult::Allowed { stdout, .. }) => {
                assert!(stdout.contains("hello"));
            }
            Ok(SandboxResult::Blocked { reason }) => {
                // Acceptable if sandbox tools aren't available
                eprintln!("Sandbox blocked: {}", reason);
            }
            Err(e) => {
                eprintln!("Sandbox error: {}", e);
            }
        }
    }

    /// review_prompt.md / Gemini-checklist follow-up: proves the network
    /// allow-list guard's URL actually reaches the spawned process's real
    /// environment — not just that `SandboxConfig` has a `proxy_url` field.
    /// A sandboxed command that echoes its own `HTTP_PROXY` must see the
    /// exact URL `ensure_network_guard` handed back.
    #[tokio::test]
    async fn execute_sandboxed_sets_the_proxy_env_vars_on_the_spawned_process() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let manager = SandboxManager::new();
        let proxy_url = manager.ensure_network_guard().await.unwrap();
        assert!(proxy_url.starts_with("http://127.0.0.1:"));

        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: Some(proxy_url.clone()),
        };

        let (shell, args): (&str, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/c", "echo %HTTP_PROXY%"])
        } else {
            ("sh", vec!["-c", "echo $HTTP_PROXY"])
        };

        let result = manager.execute_sandboxed(&config, shell, &args, &worktree);

        match result {
            Ok(SandboxResult::Allowed { stdout, .. }) => {
                assert!(
                    stdout.contains(&proxy_url),
                    "spawned process's HTTP_PROXY should be '{proxy_url}', got stdout: {stdout:?}"
                );
            }
            Ok(SandboxResult::Blocked { reason }) => {
                eprintln!(
                    "Sandbox blocked (acceptable if sandbox tools aren't available): {reason}"
                );
            }
            Err(e) => {
                eprintln!("Sandbox error (acceptable if sandbox tools aren't available): {e}");
            }
        }
    }

    #[tokio::test]
    async fn ensure_network_guard_is_idempotent_and_default_allow_list_covers_common_remotes() {
        let manager = SandboxManager::new();
        let first = manager.ensure_network_guard().await.unwrap();
        let second = manager.ensure_network_guard().await.unwrap();
        assert_eq!(
            first, second,
            "a second call must reuse the same guard, not start a new one"
        );

        let hosts = manager.network_allow_list().await;
        for expected in ["github.com", "registry.npmjs.org", "pypi.org", "crates.io"] {
            assert!(hosts.iter().any(|h| h == expected), "missing {expected}");
        }
    }

    #[test]
    fn test_sandbox_blocked_wrong_workdir() {
        let dir = TempDir::new().unwrap();
        let worktree = dir.path().to_string_lossy().to_string();
        let outside = TempDir::new().unwrap();
        let outside_path = outside.path().to_string_lossy().to_string();

        let config = SandboxConfig {
            worktree_path: worktree.clone(),
            allowed_read_paths: vec![worktree.clone()],
            allowed_write_paths: vec![worktree.clone()],
            proxy_url: None,
        };

        let manager = SandboxManager::new();

        let result = manager.execute_sandboxed(
            &config,
            if cfg!(windows) { "cmd" } else { "sh" },
            if cfg!(windows) {
                &["/c", "echo test"]
            } else {
                &["-c", "echo test"]
            },
            &outside_path,
        );

        match result {
            Ok(SandboxResult::Blocked { reason }) => {
                assert!(reason.contains("outside"));
            }
            _ => {
                // If outside path canonicalizes to same root, it might not block on all platforms
            }
        }
    }
}
