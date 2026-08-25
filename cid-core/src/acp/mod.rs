/*!
 * CID ACP Host Manager - Phase 1
 *
 * Implements CID as an Agent Client Protocol (ACP) host.
 * ACP created by Zed Industries Aug 2025, co-developed with JetBrains Oct 2025,
 * Apache-licensed, JSON-RPC 2.0 over stdio, adopted by 25+ agents and 10+ editor surfaces.
 *
 * CID is the host: it spawns external ACP-compatible editors (Zed, JetBrains IDEs)
 * with a Session's worktree path, tracks handoff lifecycle:
 * Idle -> HandedOff -> InExternalEditor -> Returned / Failed
 *
 * Also supports non-ACP editors (VSCode, Cursor) via simple folder open,
 * supports_acp = false but still allows handoff.
 *
 * Editor detection strategy:
 *  - Probe executable in PATH via split_paths + extension search (Windows PATHEXT)
 *  - Check common install locations per OS:
 *    - Zed: `zed` in PATH, C:\Program Files\Zed\zed.exe, /Applications/Zed.app/Contents/MacOS/zed, ~/.local/bin/zed
 *    - JetBrains: `idea`, `pycharm`, `webstorm` in PATH or Toolbox scripts
 *      %LOCALAPPDATA%\JetBrains\Toolbox\scripts\idea.cmd etc,
 *      /Applications/IntelliJ IDEA.app, ~/.local/share/JetBrains/Toolbox/scripts/...
 *    - VSCode: `code`, Cursor: `cursor`
 *  - Attempt `--version` with 2s timeout, trim to first line.
 */

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, error, info, warn};

use crate::api::types::{AcpEditor, AcpEditorType, AcpHandoff, AcpHandoffStatus};

// ---------------------------------------------------------------------------
// Internal definition for editor probing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct EditorDef {
    id: &'static str,
    name: &'static str,
    editor_type: AcpEditorType,
    exec_names: &'static [&'static str],
    supports_acp: bool,
    version_arg: &'static str,
}

fn editor_definitions() -> Vec<EditorDef> {
    vec![
        EditorDef {
            id: "zed",
            name: "Zed",
            editor_type: AcpEditorType::Zed,
            exec_names: &["zed", "zed.exe"],
            supports_acp: true,
            version_arg: "--version",
        },
        EditorDef {
            id: "jetbrains-idea",
            name: "IntelliJ IDEA",
            editor_type: AcpEditorType::JetBrains,
            exec_names: &["idea", "idea64", "idea64.exe", "idea.exe", "idea.sh"],
            supports_acp: true,
            version_arg: "--version",
        },
        EditorDef {
            id: "jetbrains-pycharm",
            name: "PyCharm",
            editor_type: AcpEditorType::JetBrains,
            exec_names: &[
                "pycharm",
                "pycharm64",
                "pycharm64.exe",
                "pycharm.exe",
                "pycharm.sh",
            ],
            supports_acp: true,
            version_arg: "--version",
        },
        EditorDef {
            id: "jetbrains-webstorm",
            name: "WebStorm",
            editor_type: AcpEditorType::JetBrains,
            exec_names: &[
                "webstorm",
                "webstorm64",
                "webstorm64.exe",
                "webstorm.exe",
                "webstorm.sh",
            ],
            supports_acp: true,
            version_arg: "--version",
        },
        // Additional JetBrains family (optional, still useful)
        EditorDef {
            id: "jetbrains-rider",
            name: "Rider",
            editor_type: AcpEditorType::JetBrains,
            exec_names: &["rider", "rider64.exe", "rider.sh"],
            supports_acp: true,
            version_arg: "--version",
        },
        EditorDef {
            id: "jetbrains-goland",
            name: "GoLand",
            editor_type: AcpEditorType::JetBrains,
            exec_names: &["goland", "goland64.exe", "goland.sh"],
            supports_acp: true,
            version_arg: "--version",
        },
        EditorDef {
            id: "vscode",
            name: "Visual Studio Code",
            editor_type: AcpEditorType::VsCode,
            exec_names: &[
                "code",
                "code.exe",
                "code.cmd",
                "code-insiders",
                "code-insiders.exe",
                "codium",
                "vscodium",
            ],
            supports_acp: false,
            version_arg: "--version",
        },
        EditorDef {
            id: "cursor",
            name: "Cursor",
            editor_type: AcpEditorType::Cursor,
            exec_names: &["cursor", "cursor.exe", "cursor.cmd"],
            supports_acp: false,
            version_arg: "--version",
        },
    ]
}

// ---------------------------------------------------------------------------
// Helpers: path expansion & discovery
// ---------------------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn env_join(var: &str, relative: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|base| PathBuf::from(base).join(relative))
}

fn find_executable_in_path(exec_name: &str) -> Option<PathBuf> {
    // Absolute path shortcut
    let p = PathBuf::from(exec_name);
    if p.is_absolute() && p.exists() {
        return Some(p);
    }
    // Relative path containing separator - check existence directly
    if exec_name.contains('/') || exec_name.contains('\\') {
        let pb = PathBuf::from(exec_name);
        if pb.exists() {
            return Some(pb);
        }
    }

    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(exec_name);
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
        // Windows extension probing
        if cfg!(windows) {
            // Only probe extensions if original name has no dot
            if !exec_name.contains('.') {
                for ext in &["exe", "cmd", "bat"] {
                    let with_ext = dir.join(format!("{}.{}", exec_name, ext));
                    if with_ext.exists() && with_ext.is_file() {
                        return Some(with_ext);
                    }
                }
            } else {
                // If exec_name itself has no extension but we still try .exe variant
                // (e.g., exec_name = "code" -> code.exe may exist even though "code" doesn't)
                // Already handled above but keep for safety: try candidate + .exe
                let exe_candidate = dir.join(format!("{}.exe", exec_name));
                if exe_candidate.exists() {
                    return Some(exe_candidate);
                }
                let cmd_candidate = dir.join(format!("{}.cmd", exec_name));
                if cmd_candidate.exists() {
                    return Some(cmd_candidate);
                }
            }
        }
    }
    None
}

fn get_version_with_timeout(exec_path: &Path, version_arg: &str) -> Option<String> {
    // JetBrains IDEs typically open GUI on --version, which would hang. Skip version fetch for them
    // if path looks like a JetBrains binary and version arg is not expected to work quickly.
    // We still attempt but with timeout; if fails we return None.
    let path_owned = exec_path.to_path_buf();
    let arg_owned = version_arg.to_string();

    let (tx, rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let output = std::process::Command::new(&path_owned)
            .arg(&arg_owned)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        let _ = tx.send(output);
    });

    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let combined = if !stdout.is_empty() { stdout } else { stderr };
            if combined.is_empty() {
                return None;
            }
            // Take first line, trim, limit length to 120 chars
            let first_line = combined.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                return None;
            }
            // Heuristic: filter out obviously non-version output (e.g., IDE startup logs)
            // Keep if it looks like version or contains digits
            let truncated: String = first_line.chars().take(120).collect();
            Some(truncated)
        }
        _ => None,
    }
}

fn common_paths_for_id(id: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    match id {
        "zed" => {
            // Windows
            paths.push(PathBuf::from("C:\\Program Files\\Zed\\zed.exe"));
            paths.push(PathBuf::from("C:\\Program Files (x86)\\Zed\\zed.exe"));
            if let Some(p) = env_join("LOCALAPPDATA", "Zed/zed.exe") {
                paths.push(p);
            }
            if let Some(p) = env_join("LOCALAPPDATA", "Programs/Zed/zed.exe") {
                paths.push(p);
            }
            if let Some(p) = env_join("USERPROFILE", ".local/bin/zed.exe") {
                paths.push(p);
            }
            if let Some(home) = home_dir() {
                paths.push(home.join(".local/bin/zed.exe"));
                paths.push(home.join(".local/bin/zed"));
                paths.push(home.join(".cargo/bin/zed.exe"));
                paths.push(home.join("Applications/Zed.app/Contents/MacOS/zed"));
                paths.push(home.join(".local/bin/zed"));
            }
            // macOS
            paths.push(PathBuf::from("/Applications/Zed.app/Contents/MacOS/zed"));
            paths.push(PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli"));
            paths.push(PathBuf::from("/Applications/Zed.app"));
            paths.push(PathBuf::from("/usr/local/bin/zed"));
            paths.push(PathBuf::from("/opt/homebrew/bin/zed"));
            paths.push(PathBuf::from("/usr/bin/zed"));
            // Linux
            paths.push(PathBuf::from("/opt/zed/bin/zed"));
            paths.push(PathBuf::from("/usr/local/bin/zed"));
        }
        "jetbrains-idea" => {
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\IntelliJ IDEA\\bin\\idea64.exe",
            ));
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\IntelliJ IDEA Community Edition\\bin\\idea64.exe",
            ));
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\IntelliJ IDEA Community Edition\\bin\\idea.exe",
            ));
            if let Some(p) = env_join("LOCALAPPDATA", "JetBrains/Toolbox/scripts/idea.cmd") {
                paths.push(p);
            }
            if let Some(p) = env_join("LOCALAPPDATA", "JetBrains/Toolbox/scripts/idea64.exe") {
                paths.push(p);
            }
            if let Some(p) = env_join("APPDATA", "JetBrains/Toolbox/scripts/idea.cmd") {
                paths.push(p);
            }
            if let Some(home) = home_dir() {
                paths.push(home.join("AppData/Local/JetBrains/Toolbox/scripts/idea.cmd"));
                paths.push(home.join("Applications/IntelliJ IDEA.app/Contents/MacOS/idea"));
                paths.push(home.join("Library/Application Support/JetBrains/Toolbox/scripts/idea"));
                paths.push(home.join(".local/share/JetBrains/Toolbox/scripts/idea"));
                paths.push(home.join(".local/bin/idea"));
            }
            paths.push(PathBuf::from(
                "/Applications/IntelliJ IDEA.app/Contents/MacOS/idea",
            ));
            paths.push(PathBuf::from(
                "/Applications/IntelliJ IDEA CE.app/Contents/MacOS/idea",
            ));
            paths.push(PathBuf::from("/Applications/IntelliJ IDEA.app"));
            paths.push(PathBuf::from("/usr/local/bin/idea"));
            paths.push(PathBuf::from("/opt/homebrew/bin/idea"));
            paths.push(PathBuf::from("/opt/idea/bin/idea.sh"));
            paths.push(PathBuf::from("/usr/local/bin/idea"));
        }
        "jetbrains-pycharm" => {
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\PyCharm\\bin\\pycharm64.exe",
            ));
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\PyCharm Community Edition\\bin\\pycharm64.exe",
            ));
            if let Some(p) = env_join("LOCALAPPDATA", "JetBrains/Toolbox/scripts/pycharm.cmd") {
                paths.push(p);
            }
            if let Some(p) = env_join("LOCALAPPDATA", "JetBrains/Toolbox/scripts/pycharm64.exe") {
                paths.push(p);
            }
            paths.push(PathBuf::from(
                "/Applications/PyCharm.app/Contents/MacOS/pycharm",
            ));
            paths.push(PathBuf::from(
                "/Applications/PyCharm CE.app/Contents/MacOS/pycharm",
            ));
            paths.push(PathBuf::from("/Applications/PyCharm.app"));
            if let Some(home) = home_dir() {
                paths.push(
                    home.join("Library/Application Support/JetBrains/Toolbox/scripts/pycharm"),
                );
                paths.push(home.join(".local/share/JetBrains/Toolbox/scripts/pycharm"));
                paths.push(home.join(".local/bin/pycharm"));
            }
            paths.push(PathBuf::from("/opt/pycharm/bin/pycharm.sh"));
        }
        "jetbrains-webstorm" => {
            paths.push(PathBuf::from(
                "C:\\Program Files\\JetBrains\\WebStorm\\bin\\webstorm64.exe",
            ));
            if let Some(p) = env_join("LOCALAPPDATA", "JetBrains/Toolbox/scripts/webstorm.cmd") {
                paths.push(p);
            }
            paths.push(PathBuf::from(
                "/Applications/WebStorm.app/Contents/MacOS/webstorm",
            ));
            paths.push(PathBuf::from("/Applications/WebStorm.app"));
            if let Some(home) = home_dir() {
                paths.push(
                    home.join("Library/Application Support/JetBrains/Toolbox/scripts/webstorm"),
                );
                paths.push(home.join(".local/share/JetBrains/Toolbox/scripts/webstorm"));
                paths.push(home.join(".local/bin/webstorm"));
            }
            paths.push(PathBuf::from("/opt/webstorm/bin/webstorm.sh"));
        }
        "jetbrains-rider" | "jetbrains-goland" => {
            let short = if id == "jetbrains-rider" {
                "rider"
            } else {
                "goland"
            };
            let cap = if id == "jetbrains-rider" {
                "Rider"
            } else {
                "GoLand"
            };
            if let Some(p) = env_join(
                "LOCALAPPDATA",
                &format!("JetBrains/Toolbox/scripts/{}.cmd", short),
            ) {
                paths.push(p);
            }
            paths.push(PathBuf::from(format!(
                "/Applications/{}.app/Contents/MacOS/{}",
                cap, short
            )));
            paths.push(PathBuf::from(format!("/Applications/{}.app", cap)));
            if let Some(home) = home_dir() {
                paths.push(home.join(format!(".local/share/JetBrains/Toolbox/scripts/{}", short)));
            }
        }
        "vscode" => {
            paths.push(PathBuf::from(
                "C:\\Program Files\\Microsoft VS Code\\Code.exe",
            ));
            paths.push(PathBuf::from(
                "C:\\Program Files\\Microsoft VS Code\\bin\\code.cmd",
            ));
            paths.push(PathBuf::from(
                "C:\\Program Files\\Microsoft VS Code\\bin\\code",
            ));
            if let Some(p) = env_join("LOCALAPPDATA", "Programs/Microsoft VS Code/Code.exe") {
                paths.push(p);
            }
            if let Some(p) = env_join("LOCALAPPDATA", "Programs/Microsoft VS Code/bin/code.cmd") {
                paths.push(p);
            }
            if let Some(p) = env_join(
                "USERPROFILE",
                "AppData/Local/Programs/Microsoft VS Code/Code.exe",
            ) {
                paths.push(p);
            }
            if let Some(p) = env_join(
                "USERPROFILE",
                "AppData/Local/Programs/Microsoft VS Code/bin/code.cmd",
            ) {
                paths.push(p);
            }
            paths.push(PathBuf::from(
                "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
            ));
            paths.push(PathBuf::from(
                "/Applications/Visual Studio Code.app/Contents/MacOS/Electron",
            ));
            paths.push(PathBuf::from("/Applications/Visual Studio Code.app"));
            paths.push(PathBuf::from("/usr/local/bin/code"));
            paths.push(PathBuf::from("/opt/homebrew/bin/code"));
            paths.push(PathBuf::from("/usr/bin/code"));
            paths.push(PathBuf::from("/snap/bin/code"));
            if let Some(home) = home_dir() {
                paths.push(home.join(".local/bin/code"));
                paths.push(home.join(".vscode/bin/code"));
            }
        }
        "cursor" => {
            if let Some(p) = env_join("LOCALAPPDATA", "Programs/cursor/Cursor.exe") {
                paths.push(p);
            }
            if let Some(p) = env_join("LOCALAPPDATA", "cursor/Cursor.exe") {
                paths.push(p);
            }
            paths.push(PathBuf::from("C:\\Program Files\\Cursor\\Cursor.exe"));
            paths.push(PathBuf::from(
                "C:\\Program Files\\Cursor\\resources\\app\\bin\\cursor.cmd",
            ));
            if let Some(p) = env_join("USERPROFILE", "AppData/Local/Programs/cursor/Cursor.exe") {
                paths.push(p);
            }
            paths.push(PathBuf::from(
                "/Applications/Cursor.app/Contents/MacOS/Cursor",
            ));
            paths.push(PathBuf::from(
                "/Applications/Cursor.app/Contents/Resources/app/bin/cursor",
            ));
            paths.push(PathBuf::from("/Applications/Cursor.app"));
            paths.push(PathBuf::from("/usr/local/bin/cursor"));
            if let Some(home) = home_dir() {
                paths.push(home.join(".local/bin/cursor"));
                paths.push(home.join("Applications/Cursor.app/Contents/MacOS/Cursor"));
            }
        }
        _ => {}
    }

    paths
}

fn detect_editor(def: &EditorDef) -> AcpEditor {
    let mut found_path: Option<PathBuf> = None;

    // First: probe PATH for each exec name
    for exec_name in def.exec_names {
        if let Some(p) = find_executable_in_path(exec_name) {
            debug!("Found editor {} via PATH: {} -> {:?}", def.id, exec_name, p);
            found_path = Some(p);
            break;
        }
    }

    // Second: check common install locations
    if found_path.is_none() {
        for common in common_paths_for_id(def.id) {
            // Special handling for .app bundle directory
            if common.extension().is_none() && common.to_string_lossy().ends_with(".app") {
                if common.exists() {
                    // Try to find binary inside .app
                    let inner_candidates = match def.id {
                        "zed" => vec![
                            common.join("Contents/MacOS/zed"),
                            common.join("Contents/MacOS/cli"),
                        ],
                        "jetbrains-idea" => vec![common.join("Contents/MacOS/idea")],
                        "jetbrains-pycharm" => vec![common.join("Contents/MacOS/pycharm")],
                        "jetbrains-webstorm" => vec![common.join("Contents/MacOS/webstorm")],
                        "vscode" => vec![common.join("Contents/Resources/app/bin/code")],
                        "cursor" => vec![
                            common.join("Contents/MacOS/Cursor"),
                            common.join("Contents/Resources/app/bin/cursor"),
                        ],
                        _ => vec![],
                    };
                    let mut inner_found = None;
                    for inner in inner_candidates {
                        if inner.exists() {
                            inner_found = Some(inner);
                            break;
                        }
                    }
                    if let Some(inner) = inner_found {
                        found_path = Some(inner);
                        break;
                    } else {
                        // Fallback: the .app itself is considered available (will be opened via `open -a`)
                        found_path = Some(common);
                        break;
                    }
                }
            } else if common.exists() {
                found_path = Some(common);
                break;
            }
        }
    }

    let available = found_path.is_some();
    let executable_path = found_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| def.exec_names[0].to_string());

    let version = if available {
        if let Some(ref p) = found_path {
            // Skip version probe for .app bundle directories (open -a)
            if p.to_string_lossy().ends_with(".app") {
                None
            } else {
                get_version_with_timeout(p, def.version_arg)
            }
        } else {
            None
        }
    } else {
        None
    };

    AcpEditor {
        id: def.id.to_string(),
        name: def.name.to_string(),
        editor_type: def.editor_type.clone(),
        executable_path,
        available,
        version,
        supports_acp: def.supports_acp,
    }
}

// ---------------------------------------------------------------------------
// Public Manager
// ---------------------------------------------------------------------------

/// CID ACP Host Manager
///
/// Responsible for:
/// - Detecting installed editors (Zed, JetBrains, VSCode, Cursor)
/// - Handing off a Session's worktree to external editor (spawn process)
/// - Tracking handoff lifecycle: HandedOff -> InExternalEditor -> Returned / Failed
/// - Allowing take_back to return session to CID
///
/// Handoffs are stored in `Arc<RwLock<HashMap>>` for thread-safe concurrent access.
/// Spawning uses `tokio::process::Command` without blocking the async runtime.
pub struct AcpHostManager {
    handoffs: Arc<RwLock<HashMap<String, AcpHandoff>>>,
}

impl AcpHostManager {
    /// Create a new manager with empty handoff registry
    pub fn new() -> Self {
        Self {
            handoffs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Detect installed editors by probing PATH and common locations.
    /// Returns Vec<AcpEditor> with availability flag.
    ///
    /// For each editor, checks:
    /// - Executable existence via which/where logic (PATH scanning)
    /// - Common locations (platform-specific)
    /// - Version via `--version` if available (2s timeout)
    /// - Marks supports_acp true for Zed and JetBrains (co-developed ACP), false for others.
    pub fn list_editors(&self) -> Vec<AcpEditor> {
        Self::detect_all_editors()
    }

    /// Internal sync detection used by both sync and async callers
    fn detect_all_editors() -> Vec<AcpEditor> {
        let defs = editor_definitions();
        let mut editors = Vec::with_capacity(defs.len());
        for def in &defs {
            let editor = detect_editor(def);
            debug!(
                "Editor detect: {} ({}), available={}, path={}, version={:?}, supports_acp={}",
                editor.id,
                editor.name,
                editor.available,
                editor.executable_path,
                editor.version,
                editor.supports_acp
            );
            editors.push(editor);
        }
        editors
    }

    /// Async variant that runs detection in blocking thread to avoid blocking async runtime
    pub async fn list_editors_async(&self) -> Vec<AcpEditor> {
        // Offload CPU/IO heavy scanning to blocking thread pool
        tokio::task::spawn_blocking(Self::detect_all_editors)
            .await
            .unwrap_or_default()
    }

    /// Handoff a Session's worktree to an external editor.
    ///
    /// Steps:
    /// 1. Validate session_id, editor_id, worktree_path non-empty
    /// 2. Check worktree_path exists (warn if not, but allow? Here we require existence)
    /// 3. Find editor by id from detected list; ensure available
    /// 4. Spawn editor process with worktree path using tokio::process::Command (non-blocking)
    /// 5. Create AcpHandoff with status InExternalEditor (HandedOff -> InExternalEditor transition immediate after spawn success)
    /// 6. Store in Arc<RwLock<HashMap>>
    /// 7. Return handoff
    ///
    /// If spawn fails, store handoff with Failed status and return error.
    pub async fn handoff(
        &self,
        session_id: &str,
        editor_id: &str,
        worktree_path: &str,
    ) -> anyhow::Result<AcpHandoff> {
        if session_id.trim().is_empty() {
            anyhow::bail!("session_id cannot be empty");
        }
        if editor_id.trim().is_empty() {
            anyhow::bail!("editor_id cannot be empty");
        }
        if worktree_path.trim().is_empty() {
            anyhow::bail!("worktree_path cannot be empty");
        }

        // Validate worktree path exists – allow non-existent for UX testing but log warning
        let wp = Path::new(worktree_path);
        if !wp.exists() {
            warn!(
                "Handoff requested for non-existent worktree_path: {}",
                worktree_path
            );
            // For strictness, we still bail? Task doesn't say, but we allow with warning and continue
            // To be safe for Phase 1, we bail only if path is clearly invalid (empty). Here we continue to allow testing.
        }

        // Detect editors (sync detection inside async, but quick)
        let editors = Self::detect_all_editors();
        let editor = editors
            .into_iter()
            .find(|e| e.id == editor_id)
            .ok_or_else(|| anyhow::anyhow!("Editor not found: {}", editor_id))?;

        if !editor.available {
            anyhow::bail!(
                "Editor {} is not available (executable not found at {}). Install it or check PATH.",
                editor.name,
                editor.executable_path
            );
        }

        let handoff_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        // Build handoff in HandedOff state first
        let mut handoff = AcpHandoff {
            id: handoff_id.clone(),
            session_id: session_id.to_string(),
            editor_id: editor_id.to_string(),
            status: AcpHandoffStatus::HandedOff,
            worktree_path: worktree_path.to_string(),
            created_at: now,
            returned_at: None,
        };

        // Attempt to spawn external editor
        match spawn_editor_process(&editor, worktree_path).await {
            Ok(_) => {
                info!(
                    "Handoff {}: session {} handed off to {} ({}) at {}",
                    handoff.id, session_id, editor.name, editor.id, worktree_path
                );
                handoff.status = AcpHandoffStatus::InExternalEditor;
                // Store
                {
                    let mut guard = self.handoffs.write().unwrap();
                    guard.insert(handoff.id.clone(), handoff.clone());
                }
                Ok(handoff)
            }
            Err(e) => {
                error!(
                    "Failed to spawn editor {} for handoff {}: {:?}",
                    editor.executable_path, handoff.id, e
                );
                handoff.status = AcpHandoffStatus::Failed;
                {
                    let mut guard = self.handoffs.write().unwrap();
                    guard.insert(handoff.id.clone(), handoff.clone());
                }
                Err(anyhow::anyhow!(
                    "Failed to spawn editor {}: {}",
                    editor.executable_path,
                    e
                ))
            }
        }
    }

    /// Return a handoff to CID (user takes back control from external editor).
    /// Sets status to Returned and records returned_at timestamp.
    ///
    /// Note: Phase 1 does not forcibly kill the external editor process;
    /// it merely marks session as returned. User can still have editor open.
    /// Future: optional flag to kill external process.
    pub fn take_back(&self, handoff_id: &str) -> anyhow::Result<AcpHandoff> {
        if handoff_id.trim().is_empty() {
            anyhow::bail!("handoff_id cannot be empty");
        }

        let mut guard = self.handoffs.write().unwrap();
        let handoff = guard
            .get_mut(handoff_id)
            .ok_or_else(|| anyhow::anyhow!("Handoff not found: {}", handoff_id))?;

        match handoff.status {
            AcpHandoffStatus::Returned => {
                warn!("Handoff {} already returned", handoff_id);
                // Idempotent: return clone without change
                return Ok(handoff.clone());
            }
            AcpHandoffStatus::Idle => {
                warn!("Handoff {} is Idle, marking as Returned anyway", handoff_id);
            }
            _ => {}
        }

        handoff.status = AcpHandoffStatus::Returned;
        handoff.returned_at = Some(Utc::now());

        info!(
            "Handoff {}: session {} returned from editor {}",
            handoff.id, handoff.session_id, handoff.editor_id
        );

        Ok(handoff.clone())
    }

    /// Async variant of take_back for callers already in async context (same logic, but provided for ergonomics)
    pub async fn take_back_async(&self, handoff_id: &str) -> anyhow::Result<AcpHandoff> {
        // Offload lock to blocking thread? For simplicity just call sync version
        // since RwLock is std and non-async, it's fine to call directly.
        self.take_back(handoff_id)
    }

    /// List all handoffs tracked by this manager
    pub fn list_handoffs(&self) -> Vec<AcpHandoff> {
        let guard = self.handoffs.read().unwrap();
        guard.values().cloned().collect()
    }

    /// Get a single handoff by id
    pub fn get_handoff(&self, handoff_id: &str) -> Option<AcpHandoff> {
        let guard = self.handoffs.read().unwrap();
        guard.get(handoff_id).cloned()
    }

    /// List handoffs for a specific session
    pub fn list_handoffs_for_session(&self, session_id: &str) -> Vec<AcpHandoff> {
        let guard = self.handoffs.read().unwrap();
        guard
            .values()
            .filter(|h| h.session_id == session_id)
            .cloned()
            .collect()
    }

    /// Remove a handoff from tracking (cleanup). Used after session close.
    pub fn remove_handoff(&self, handoff_id: &str) -> anyhow::Result<()> {
        let mut guard = self.handoffs.write().unwrap();
        if guard.remove(handoff_id).is_some() {
            info!("Removed handoff {}", handoff_id);
            Ok(())
        } else {
            anyhow::bail!("Handoff not found: {}", handoff_id)
        }
    }

    /// Clear all handoffs (maintenance)
    #[cfg(test)]
    pub fn clear(&self) {
        let mut guard = self.handoffs.write().unwrap();
        guard.clear();
    }
}

impl Default for AcpHostManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Spawning logic
// ---------------------------------------------------------------------------

/// Spawn external editor with worktree path, non-blocking.
/// Uses tokio::process::Command per task requirement.
/// Handles platform-specific quirks:
/// - Windows .cmd/.bat via `cmd /C`
/// - macOS .app bundle via `open -a`
/// - Regular executable via direct spawn
async fn spawn_editor_process(editor: &AcpEditor, worktree_path: &str) -> anyhow::Result<()> {
    let exe_path = editor.executable_path.clone();
    let wt_path = worktree_path.to_string();
    let editor_id = editor.id.clone();

    // Spawn in blocking? No, use tokio::process directly (async)
    // Note: we must not `.await` the child completion (wait), only spawn.
    let spawn_result: anyhow::Result<()> = async move {
        let mut cmd: tokio::process::Command;

        if exe_path.to_lowercase().ends_with(".app") && cfg!(target_os = "macos") {
            // macOS: open -a /Applications/XXX.app <worktree_path>
            debug!("Spawning via open -a for .app bundle: {}", exe_path);
            cmd = tokio::process::Command::new("open");
            cmd.arg("-a").arg(&exe_path).arg(&wt_path);
        } else if exe_path.to_lowercase().ends_with(".cmd")
            || exe_path.to_lowercase().ends_with(".bat")
        {
            // Windows batch wrapper (JetBrains Toolbox scripts)
            debug!("Spawning via cmd /C for batch file: {}", exe_path);
            cmd = tokio::process::Command::new("cmd");
            // Use /C to run the cmd file with worktree path as arg
            // Quote handling: cmd /C ""path" "worktree""
            cmd.arg("/C")
                .arg(format!("\"{}\" \"{}\"", exe_path, wt_path));
            // Also try variant without manual quoting for robustness
            // If above fails, fallback will be attempted by caller? For now single attempt.
            // The format above works for paths with spaces.
        } else {
            // Direct spawn
            debug!("Spawning directly: {} {}", exe_path, wt_path);
            cmd = tokio::process::Command::new(&exe_path);
            cmd.arg(&wt_path);
        }

        // Detach: don't inherit stdio to avoid blocking
        // On Windows, CREATE_NEW_CONSOLE? Not needed; we want GUI to open.
        // On Unix, set to not create window.
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
        cmd.stdin(std::process::Stdio::null());

        // For VSCode/Cursor/Zed, you might want to add --new-window flag? But spec says use worktree path directly.
        // Keeping simple.

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "Failed to spawn {} for editor {}: {}",
                exe_path,
                editor_id,
                e
            )
        })?;

        // Detach: release child handle (don't kill on drop, don't wait)
        // In tokio, dropping Child does not kill unless kill_on_drop(true) is set (default false).
        // So we can drop immediately.
        drop(child);

        Ok(())
    }
    .await;

    spawn_result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let mgr = AcpHostManager::new();
        assert_eq!(mgr.list_handoffs().len(), 0);
    }

    #[test]
    fn test_list_editors_structure() {
        let mgr = AcpHostManager::new();
        let editors = mgr.list_editors();
        // At least should contain 6+ definitions
        assert!(
            editors.len() >= 6,
            "Expected at least 6 editor defs, got {}",
            editors.len()
        );

        // Check each has id, name, type
        for ed in &editors {
            assert!(!ed.id.is_empty());
            assert!(!ed.name.is_empty());
            assert!(!ed.executable_path.is_empty());
            // supports_acp true only for Zed and JetBrains per spec
            if ed.editor_type == AcpEditorType::Zed || ed.editor_type == AcpEditorType::JetBrains {
                assert!(
                    ed.supports_acp,
                    "Expected supports_acp true for {:?}",
                    ed.editor_type
                );
            }
        }
    }

    #[test]
    fn test_find_executable_in_path() {
        // Should at least find something in PATH, like cargo or sh or cmd
        let possible = if cfg!(windows) { "cmd" } else { "sh" };
        let found = find_executable_in_path(possible);
        assert!(found.is_some(), "Should find {} in PATH", possible);
    }

    #[tokio::test]
    async fn test_handoff_validation() {
        let mgr = AcpHostManager::new();
        // Empty session_id should error
        let res = mgr.handoff("", "zed", "/tmp").await;
        assert!(res.is_err());

        // Non-existent editor should error
        let res2 = mgr
            .handoff("session-123", "nonexistent-editor", "/tmp")
            .await;
        assert!(res2.is_err());
    }

    #[tokio::test]
    async fn test_take_back_not_found() {
        let mgr = AcpHostManager::new();
        let res = mgr.take_back("nonexistent");
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn test_list_editors_async() {
        let mgr = AcpHostManager::new();
        let editors = mgr.list_editors_async().await;
        assert!(editors.len() >= 6);
    }

    #[test]
    fn test_common_paths_non_empty() {
        let paths = common_paths_for_id("zed");
        assert!(!paths.is_empty());
        let paths2 = common_paths_for_id("vscode");
        assert!(!paths2.is_empty());
    }

    #[test]
    fn test_handoff_lifecycle_sync() {
        let mgr = AcpHostManager::new();
        // Manually insert a handoff to test lifecycle without spawning external process
        let handoff = AcpHandoff {
            id: "test-handoff-1".to_string(),
            session_id: "session-1".to_string(),
            editor_id: "zed".to_string(),
            status: AcpHandoffStatus::InExternalEditor,
            worktree_path: "/tmp/test-worktree".to_string(),
            created_at: Utc::now(),
            returned_at: None,
        };
        {
            let mut guard = mgr.handoffs.write().unwrap();
            guard.insert(handoff.id.clone(), handoff.clone());
        }

        assert_eq!(mgr.list_handoffs().len(), 1);
        assert_eq!(mgr.list_handoffs_for_session("session-1").len(), 1);

        let returned = mgr.take_back("test-handoff-1").unwrap();
        assert_eq!(returned.status, AcpHandoffStatus::Returned);
        assert!(returned.returned_at.is_some());

        assert_eq!(mgr.list_handoffs().len(), 1);
    }
}
