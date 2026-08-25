import { useCallback, useEffect, useRef, useState } from "react";
// Side-effect import: pins Monaco to the bundled copy instead of a CDN fetch.
// Must precede the first <Editor> render — see the module for why.
import "@/lib/monaco-setup";
import Editor, { type OnMount } from "@monaco-editor/react";
import { useSessionRepoPath } from "@/hooks/useSessionRepoPath";
import { api } from "@/lib/api";
import { useTheme } from "@/theme/useTheme";
import { useFocusTrap } from "@/lib/useFocusTrap";
import { t as translate } from "@/lib/i18n";
import { ChevronRight, ChevronDown, Loader2, Search, X, FolderOpen, ListTree } from "lucide-react";

// 051-Editor-Excellence-Roadmap.md Wave 4.3: extension -> Monaco language id.
// Not every language Monaco ships, just the common ones a repo actually has;
// anything unlisted falls back to plaintext rather than guessing wrong.
const LANGUAGE_BY_EXT: Record<string, string> = {
  rs: "rust",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  py: "python",
  go: "go",
  json: "json",
  md: "markdown",
  mdx: "markdown",
  yaml: "yaml",
  yml: "yaml",
  toml: "ini",
  css: "css",
  scss: "scss",
  less: "less",
  html: "html",
  htm: "html",
  xml: "xml",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  sql: "sql",
  java: "java",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  cs: "csharp",
  php: "php",
  rb: "ruby",
  kt: "kotlin",
  kts: "kotlin",
  swift: "swift",
  lua: "lua",
  graphql: "graphql",
  gql: "graphql",
  proto: "protobuf",
  ps1: "powershell",
  r: "r",
  pl: "perl",
  scala: "scala",
  ini: "ini",
  env: "shell",
};

function languageForPath(path: string): string {
  const base = path.split(/[/\\]/).pop() || path;
  if (base.toLowerCase() === "dockerfile") return "dockerfile";
  const dot = base.lastIndexOf(".");
  const ext = dot >= 0 ? base.slice(dot + 1).toLowerCase() : "";
  return LANGUAGE_BY_EXT[ext] || "plaintext";
}

// Directories a repo's own file tree should never expand into — mirrors this
// project's own .gitignore conventions rather than parsing .gitignore itself
// (real glob/negation semantics are a much bigger, separate undertaking; this
// covers the overwhelming majority of what an agent's own worktrees produce).
const IGNORED_DIR_NAMES = new Set([
  ".git",
  "node_modules",
  "target",
  "dist",
  "build",
  ".cid",
  "__pycache__",
  ".venv",
  "venv",
  ".next",
  ".turbo",
]);

type FileEntry = { name: string; path: string; is_dir: boolean; is_file: boolean; size: number };

function visibleEntries(entries: FileEntry[]): FileEntry[] {
  return entries
    // Not `is_dir && ignored`: inside a Session's linked worktree `.git` is a
    // *file* holding a `gitdir:` pointer, so the directory-only test let it
    // through and the tree listed it as an openable document.
    .filter((e) => !IGNORED_DIR_NAMES.has(e.name))
    .sort((a, b) => (a.is_dir === b.is_dir ? a.name.localeCompare(b.name) : a.is_dir ? -1 : 1));
}

function FileTreeNode({
  entry,
  depth,
  activePath,
  onOpenFile,
}: {
  entry: FileEntry;
  depth: number;
  activePath: string | null;
  onOpenFile: (path: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<FileEntry[] | null>(null);
  const [loading, setLoading] = useState(false);

  const toggle = async () => {
    if (!entry.is_dir) {
      onOpenFile(entry.path);
      return;
    }
    if (!expanded && children === null) {
      setLoading(true);
      try {
        const list: FileEntry[] = await api.file.list(entry.path);
        setChildren(visibleEntries(list));
      } catch (e) {
        console.error(e);
        setChildren([]);
      } finally {
        setLoading(false);
      }
    }
    setExpanded((v) => !v);
  };

  return (
    <div>
      <button
        onClick={toggle}
        style={{ paddingLeft: 8 + depth * 12 }}
        className={`w-full flex items-center gap-1 text-left pr-2 py-1 rounded text-xs truncate hover:bg-accent ${
          activePath === entry.path ? "bg-accent" : ""
        } ${entry.is_dir ? "text-muted-foreground font-medium" : ""}`}
      >
        {entry.is_dir ? (
          loading ? (
            <Loader2 className="w-3 h-3 shrink-0 animate-spin" />
          ) : expanded ? (
            <ChevronDown className="w-3 h-3 shrink-0" />
          ) : (
            <ChevronRight className="w-3 h-3 shrink-0" />
          )
        ) : (
          <span className="w-3 shrink-0" />
        )}
        <span className="truncate">
          {entry.is_dir ? "📁 " : "📄 "}
          {entry.name}
        </span>
      </button>
      {expanded &&
        children &&
        (children.length === 0 ? (
          <div style={{ paddingLeft: 8 + (depth + 1) * 12 }} className="text-[11px] text-muted-foreground py-0.5">
            empty
          </div>
        ) : (
          children.map((c) => (
            <FileTreeNode key={c.path} entry={c} depth={depth + 1} activePath={activePath} onOpenFile={onOpenFile} />
          ))
        ))}
    </div>
  );
}

function FileTree({ rootPath, activePath, onOpenFile }: { rootPath: string; activePath: string | null; onOpenFile: (path: string) => void }) {
  const [entries, setEntries] = useState<FileEntry[]>([]);

  useEffect(() => {
    let cancelled = false;
    api.file
      .list(rootPath)
      .then((list: FileEntry[]) => {
        if (!cancelled) setEntries(visibleEntries(list));
      })
      .catch((e) => console.error(e));
    return () => {
      cancelled = true;
    };
  }, [rootPath]);

  if (entries.length === 0) {
    return <div className="text-[11px] text-muted-foreground p-2">No files</div>;
  }

  return (
    <>
      {entries.map((e) => (
        <FileTreeNode key={e.path} entry={e} depth={0} activePath={activePath} onOpenFile={onOpenFile} />
      ))}
    </>
  );
}

type SearchHit = {
  file_path: string;
  line: number;
  line_text: string;
  match_start?: number | null;
  match_end?: number | null;
};

/// Splits a matched line so the matched span can be highlighted, using the
/// byte offsets the backend already computed rather than re-running the match
/// in the browser (which would disagree with the server on case rules).
function HighlightedLine({ hit }: { hit: SearchHit }) {
  const { line_text, match_start, match_end } = hit;
  if (match_start == null || match_end == null || match_end <= match_start) {
    return <span className="truncate">{line_text}</span>;
  }
  return (
    <span className="truncate">
      {line_text.slice(0, match_start)}
      <mark className="bg-primary/30 text-foreground rounded-sm px-0.5">
        {line_text.slice(match_start, match_end)}
      </mark>
      {line_text.slice(match_end)}
    </span>
  );
}

// Backed by `search.text` — ripgrep's engine in Core. This previously called
// code.search_symbols, which walked and tree-sitter parsed every file in the
// repo including target/ and node_modules/ (218 seconds on CID's own repo, so
// the panel just sat on "Searching…"). Symbol search still exists for the
// outline; this box is for finding text, which is what people type into it.
function RepoSearchPanel({ repoPath, onOpenAt }: { repoPath: string; onOpenAt: (path: string, line?: number) => void }) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [truncated, setTruncated] = useState(false);
  const [elapsedMs, setElapsedMs] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([]);
      setElapsedMs(null);
      setTruncated(false);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    const run = async () => {
      try {
        const res = (await api.search.text(repoPath, query)) as {
          hits?: SearchHit[];
          truncated?: boolean;
          elapsed_ms?: number;
        } | null;
        if (cancelled) return;
        setError(null);
        setResults(res?.hits ?? []);
        setTruncated(Boolean(res?.truncated));
        setElapsedMs(res?.elapsed_ms ?? null);
      } catch (e) {
        if (!cancelled) {
          setResults([]);
          setError(String(e));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };
    const t = setTimeout(run, 150);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query, repoPath]);

  return (
    <div className="flex flex-col h-full">
      <div className="p-2 border-b">
        <div className="relative">
          <Search className="w-3.5 h-3.5 absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search text across the repo…"
            aria-label="Search text across the repo"
            className="w-full bg-background border rounded pl-7 pr-2 py-1 text-xs"
          />
        </div>
        {elapsedMs !== null && !loading && !error && (
          <div className="text-[10px] text-muted-foreground mt-1">
            {truncated ? `first ${results.length} matches` : `${results.length} match${results.length === 1 ? "" : "es"}`} in {elapsedMs}ms
          </div>
        )}
      </div>
      <div className="flex-1 overflow-y-auto p-1">
        {loading && <div className="text-[11px] text-muted-foreground p-2">Searching…</div>}
        {error && <div className="text-[11px] text-red-400 p-2">{error}</div>}
        {!loading && !error && query.trim().length >= 2 && results.length === 0 && (
          <div className="text-[11px] text-muted-foreground p-2">No matches</div>
        )}
        {results.map((r, i) => (
          <button
            key={`${r.file_path}-${r.line}-${i}`}
            onClick={() => onOpenAt(r.file_path, r.line)}
            className="w-full text-left px-2 py-1 rounded text-xs hover:bg-accent block"
          >
            <div className="flex gap-1.5 font-mono text-[11px]">
              <span className="text-muted-foreground shrink-0 tabular-nums">{r.line}</span>
              <HighlightedLine hit={r} />
            </div>
            <div className="truncate text-[10px] text-muted-foreground">
              {r.file_path.split(/[/\\]/).slice(-2).join("/")}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

type SaveStatus = "idle" | "saving" | "saved" | "error";
/// `readOnlyReason` set means the buffer is not the file's real contents, so
/// writing it back would destroy data. Every save path must check it.
type ReadOnlyReason = "binary" | "too_large" | "error";
type OpenTab = {
  path: string;
  content: string;
  savedContent: string;
  saveStatus: SaveStatus;
  readOnlyReason?: ReadOnlyReason;
  size?: number;
};

function formatBytes(n?: number): string {
  if (n == null) return "";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function readOnlyMessage(tab: OpenTab): string {
  switch (tab.readOnlyReason) {
    case "binary":
      return `Binary file (${formatBytes(tab.size)}) — not shown, and not editable here.`;
    case "too_large":
      return `File is too large to open in the editor (${formatBytes(tab.size)}).`;
    default:
      return tab.content;
  }
}
type OutlineSymbol = { name: string; kind: string; line: number };

// 051 Wave 5.1e: a UI over code.analyze_file — previously reachable only
// internally (via code.search_symbols) and from E2E tests, never from a
// component. Fetched fresh per file open rather than kept live, since it's a
// point-in-time outline of what's on disk, not a diagnostic stream.
function OutlinePanel({ path, onJump }: { path: string; onJump: (line: number) => void }) {
  const [symbols, setSymbols] = useState<OutlineSymbol[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setSymbols(null);
    setError(null);
    api.code
      .analyzeFile(path)
      .then((result: { symbols: OutlineSymbol[] }) => {
        if (!cancelled) setSymbols(result.symbols || []);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [path]);

  return (
    <div className="absolute right-2 top-8 z-40 w-64 max-h-80 overflow-y-auto bg-card border rounded shadow-lg p-1">
      {error && <div className="text-[11px] text-destructive p-2">{error}</div>}
      {!error && symbols === null && <div className="text-[11px] text-muted-foreground p-2">Analyzing…</div>}
      {symbols && symbols.length === 0 && <div className="text-[11px] text-muted-foreground p-2">No symbols found</div>}
      {symbols?.map((s, i) => (
        <button
          key={`${s.name}-${s.line}-${i}`}
          onClick={() => onJump(s.line)}
          className="w-full text-left px-2 py-1 rounded text-xs hover:bg-accent flex items-center justify-between gap-2"
        >
          <span className="truncate">{s.name}</span>
          <span className="text-[10px] text-muted-foreground shrink-0">
            {s.kind} · L{s.line}
          </span>
        </button>
      ))}
    </div>
  );
}

function CloseTabModal({ fileName, onSaveAndClose, onDiscardAndClose, onCancel }: {
  fileName: string;
  onSaveAndClose: () => void;
  onDiscardAndClose: () => void;
  onCancel: () => void;
}) {
  const ref = useFocusTrap<HTMLDivElement>(true, onCancel);
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div
        ref={ref}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="close-tab-title"
        tabIndex={-1}
        className="bg-card border rounded-lg p-6 w-[420px] max-w-[90vw]"
      >
        <h2 id="close-tab-title" className="font-semibold mb-2">
          {translate().dialog.unsavedChangesTitle}
        </h2>
        <p className="text-sm text-muted-foreground mb-4">
          <span className="text-foreground">{fileName}</span> {translate().editor.unsavedChangesBody}
        </p>
        <div className="flex justify-end gap-2">
          <button onClick={onCancel} className="px-3 py-1.5 text-sm bg-secondary rounded">
            {translate().common.cancel}
          </button>
          <button onClick={onDiscardAndClose} className="px-3 py-1.5 text-sm bg-destructive text-destructive-foreground rounded">
            {translate().editor.discardAndClose}
          </button>
          <button onClick={onSaveAndClose} className="px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded">
            {translate().editor.saveAndClose}
          </button>
        </div>
      </div>
    </div>
  );
}

export function EditorPane() {
  // The Session's working tree, not the main checkout. Editing the main repo
  // while a worktree Session was selected is why a saved change never appeared
  // in the Diff panel — see useSessionRepoPath.
  const repoPath = useSessionRepoPath();
  const theme = useTheme((s) => s.theme);
  const [tabs, setTabs] = useState<OpenTab[]>([]);
  const [activePath, setActivePath] = useState<string | null>(null);
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [closingTab, setClosingTab] = useState<string | null>(null);
  const [leftView, setLeftView] = useState<"files" | "search">("files");
  const [showOutline, setShowOutline] = useState(false);
  const pendingLine = useRef<number | undefined>(undefined);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);

  const activeTab = tabs.find((t) => t.path === activePath) || null;
  const isDirty = (t: OpenTab) => t.content !== t.savedContent;
  const anyDirty = tabs.some(isDirty);

  useEffect(() => {
    setShowOutline(false);
  }, [activePath]);

  // Reset the whole tab set when the working tree changes — a path from another
  // repo, or from another Session's worktree, has no meaning here. Keyed on the
  // resolved path rather than the repo id so that switching between two
  // Sessions on the *same* repo (different worktrees) also clears stale tabs.
  useEffect(() => {
    setTabs([]);
    setActivePath(null);
  }, [repoPath]);

  // A dirty editor must not be lost to an accidental tab/window close.
  useEffect(() => {
    if (!anyDirty) return;
    const handler = (e: BeforeUnloadEvent) => {
      e.preventDefault();
      e.returnValue = "";
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [anyDirty]);

  const openFile = useCallback(
    async (path: string, line?: number) => {
      pendingLine.current = line;
      const existing = tabs.find((t) => t.path === path);
      if (existing) {
        setActivePath(path);
        if (line) jumpToLine(line);
        return;
      }
      setLoadingPath(path);
      try {
        const data = (await api.file.read(path)) as {
          content: string;
          binary?: boolean;
          too_large?: boolean;
          size?: number;
        };
        // Opened read-only rather than as an error. `.coverage` is a SQLite
        // database, and previously the raw "stream did not contain valid UTF-8"
        // was shown as the file's *contents* — which meant Save would then
        // write that sentence over the real bytes.
        setTabs((prev) => [
          ...prev,
          {
            path,
            content: data.content ?? "",
            savedContent: data.content ?? "",
            saveStatus: "idle",
            readOnlyReason: data.binary
              ? "binary"
              : data.too_large
              ? "too_large"
              : undefined,
            size: data.size,
          },
        ]);
        setActivePath(path);
      } catch (e) {
        console.error(e);
        const message = `Failed to read file: ${e}`;
        setTabs((prev) => [
          ...prev,
          { path, content: message, savedContent: message, saveStatus: "idle", readOnlyReason: "error" },
        ]);
        setActivePath(path);
      } finally {
        setLoadingPath(null);
      }
    },
    [tabs]
  );

  function jumpToLine(line: number) {
    const ed = editorRef.current;
    if (!ed) return;
    ed.revealLineInCenter(line);
    ed.setPosition({ lineNumber: line, column: 1 });
    ed.focus();
  }

  useEffect(() => {
    if (activeTab && pendingLine.current) {
      jumpToLine(pendingLine.current);
      pendingLine.current = undefined;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab?.path]);

  const updateActiveContent = (v: string) => {
    if (!activePath) return;
    setTabs((prev) => prev.map((t) => (t.path === activePath ? { ...t, content: v } : t)));
  };

  const saveTab = useCallback(async (path: string) => {
    const tab = tabs.find((t) => t.path === path);
    if (!tab) return;
    // The buffer for one of these is a placeholder, never the file's bytes —
    // writing it back would truncate a binary or a large file to a sentence.
    if (tab.readOnlyReason) return;
    setTabs((prev) => prev.map((t) => (t.path === path ? { ...t, saveStatus: "saving" } : t)));
    try {
      await api.file.write(path, tab.content);
      setTabs((prev) =>
        prev.map((t) => (t.path === path ? { ...t, savedContent: t.content, saveStatus: "saved" } : t))
      );
      // Best-effort, non-blocking: 051 Wave 5.1b closes semantic_engine.index_file
      // (previously reachable only internally/from E2E tests) by keeping the
      // Semantic Engine's index fresh on save rather than waiting for a full
      // repo re-scan. Isolated in its own try so a failure here (or the API
      // stub being absent in older tests) can never flip a successful save's
      // status to "error".
      try {
        if (repoPath) {
          const status = await api.semanticEngine.status(repoPath);
          if (status?.enabled) {
            await api.semanticEngine.indexFile(repoPath, path, tab.content);
          }
        }
      } catch (e) {
        console.error("[CID] semantic index refresh on save failed", e);
      }
    } catch (e) {
      console.error(e);
      setTabs((prev) => prev.map((t) => (t.path === path ? { ...t, saveStatus: "error" } : t)));
    }
  }, [tabs, repoPath]);

  // Ctrl+S / Cmd+S saves the active tab.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (activePath) saveTab(activePath);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [activePath, saveTab]);

  const requestCloseTab = (path: string) => {
    const tab = tabs.find((t) => t.path === path);
    if (tab && isDirty(tab)) {
      setClosingTab(path);
      return;
    }
    doCloseTab(path);
  };

  function doCloseTab(path: string) {
    setTabs((prev) => {
      const next = prev.filter((t) => t.path !== path);
      if (activePath === path) {
        setActivePath(next.length ? next[next.length - 1].path : null);
      }
      return next;
    });
  }

  const handleModalSaveAndClose = async () => {
    if (closingTab) {
      await saveTab(closingTab);
      doCloseTab(closingTab);
    }
    setClosingTab(null);
  };
  const handleModalDiscardAndClose = () => {
    if (closingTab) doCloseTab(closingTab);
    setClosingTab(null);
  };

  const closingTabName = closingTab?.split(/[/\\]/).pop() ?? "";

  return (
    <div className="flex h-full">
      {/* Left: file tree / search */}
      <div className="w-[240px] border-r bg-card overflow-y-auto flex flex-col">
        <div className="flex items-center gap-1 p-1 border-b shrink-0">
          <button
            onClick={() => setLeftView("files")}
            className={`flex-1 flex items-center justify-center gap-1 text-[11px] px-2 py-1 rounded ${leftView === "files" ? "bg-accent" : "text-muted-foreground hover:bg-accent/50"}`}
          >
            <FolderOpen className="w-3 h-3" /> Files
          </button>
          <button
            onClick={() => setLeftView("search")}
            className={`flex-1 flex items-center justify-center gap-1 text-[11px] px-2 py-1 rounded ${leftView === "search" ? "bg-accent" : "text-muted-foreground hover:bg-accent/50"}`}
          >
            <Search className="w-3 h-3" /> Search
          </button>
        </div>
        <div className="flex-1 min-h-0 overflow-y-auto">
          {leftView === "files" ? (
            repoPath ? (
              <div className="space-y-0.5 p-1">
                <FileTree rootPath={repoPath} activePath={activePath} onOpenFile={(p) => openFile(p)} />
              </div>
            ) : (
              <div className="text-[11px] text-muted-foreground p-2">No repo selected</div>
            )
          ) : repoPath ? (
            <RepoSearchPanel repoPath={repoPath} onOpenAt={(p, line) => openFile(p, line)} />
          ) : (
            <div className="text-[11px] text-muted-foreground p-2">No repo selected</div>
          )}
        </div>
      </div>

      {/* Right: tabs + Monaco */}
      <div className="flex-1 flex flex-col min-w-0 relative">
        <div className="h-8 border-b flex items-center overflow-x-auto bg-card shrink-0">
          {tabs.map((t) => {
            const name = t.path.split(/[/\\]/).pop() || t.path;
            const dirty = isDirty(t);
            return (
              <div
                key={t.path}
                onClick={() => setActivePath(t.path)}
                className={`flex items-center gap-1.5 px-2.5 h-full text-xs border-r cursor-pointer whitespace-nowrap ${
                  activePath === t.path ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:bg-accent/50"
                }`}
                title={t.path}
              >
                {dirty && <span className="text-yellow-500">●</span>}
                <span className="truncate max-w-[140px]">{name}</span>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    requestCloseTab(t.path);
                  }}
                  className="hover:bg-background rounded p-0.5"
                  aria-label={`Close ${name}`}
                >
                  <X className="w-3 h-3" />
                </button>
              </div>
            );
          })}
        </div>
        {activeTab && (
          <div className="h-8 border-b flex items-center px-3 text-xs bg-card gap-2 shrink-0">
            <span className="truncate text-muted-foreground">{activeTab.path}</span>
            {activeTab.readOnlyReason && (
              <span className="text-[10px] px-1 rounded bg-muted text-muted-foreground shrink-0">read-only</span>
            )}
            {activeTab.saveStatus === "saved" && <span className="text-[11px] text-muted-foreground">Saved</span>}
            {activeTab.saveStatus === "error" && <span className="text-[11px] text-destructive">Save failed</span>}
            <button
              onClick={() => setShowOutline((v) => !v)}
              className={`ml-auto p-1 rounded hover:bg-accent ${showOutline ? "bg-accent" : ""}`}
              title="Outline"
              aria-label="Toggle outline"
            >
              <ListTree className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => saveTab(activeTab.path)}
              disabled={
                activeTab.saveStatus === "saving" || !isDirty(activeTab) || !!activeTab.readOnlyReason
              }
              className="bg-primary text-primary-foreground px-2 py-0.5 rounded disabled:opacity-50"
            >
              {activeTab.saveStatus === "saving" ? "Saving…" : "Save"}
            </button>
          </div>
        )}
        {activeTab && showOutline && (
          <OutlinePanel
            path={activeTab.path}
            onJump={(line) => {
              jumpToLine(line);
              setShowOutline(false);
            }}
          />
        )}
        <div className="flex-1">
          {loadingPath ? (
            <div className="p-4 text-sm text-muted-foreground">Loading…</div>
          ) : activeTab && activeTab.readOnlyReason && activeTab.readOnlyReason !== "error" ? (
            <div className="h-full flex flex-col items-center justify-center gap-1 text-sm text-muted-foreground p-6 text-center">
              <span>{readOnlyMessage(activeTab)}</span>
              <span className="text-[11px]">The file is untouched — saving is disabled for it.</span>
            </div>
          ) : activeTab ? (
            <Editor
              path={activeTab.path}
              height="100%"
              language={languageForPath(activeTab.path)}
              value={activeTab.content}
              onChange={(v) => updateActiveContent(v || "")}
              onMount={(editor) => {
                editorRef.current = editor;
                if (pendingLine.current) {
                  jumpToLine(pendingLine.current);
                  pendingLine.current = undefined;
                }
              }}
              options={{
                minimap: { enabled: false },
                fontSize: 13,
                automaticLayout: true,
              }}
              theme={theme === "light" ? "vs" : "vs-dark"}
            />
          ) : (
            <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a file to edit</div>
          )}
        </div>
      </div>

      {closingTab && (
        <CloseTabModal
          fileName={closingTabName}
          onSaveAndClose={handleModalSaveAndClose}
          onDiscardAndClose={handleModalDiscardAndClose}
          onCancel={() => setClosingTab(null)}
        />
      )}
    </div>
  );
}
