import { useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { Search, FileWarning } from "lucide-react";

type TestImpactEntry = { symbol: string; covering_tests: string[] };
type StaleDocEntry = { doc_path: string; missing_symbols: string[] };
type GitBlameInfo = {
  file_path: string;
  line: number;
  author: string;
  email: string;
  commit_hash: string;
  commit_date: string;
  commit_summary: string;
};

// 051-Editor-Excellence-Roadmap.md Wave 5.1b: the test-impact and doc graphs
// were a headline Phase 4 feature with real backend logic and zero surface —
// folded into RepoHealthPanel's existing tab area rather than a new panel.

export function TestImpactTab({ repoPath }: { repoPath: string }) {
  const [symbol, setSymbol] = useState("");
  const [lookup, setLookup] = useState<TestImpactEntry | null>(null);
  const [entries, setEntries] = useState<TestImpactEntry[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [batchSymbols, setBatchSymbols] = useState("");
  const [batchResult, setBatchResult] = useState<string[] | null>(null);

  const runLookup = async () => {
    if (!symbol.trim()) return;
    setLoading(true);
    try {
      const res = await api.semanticEngine.testImpactForSymbol(repoPath, symbol.trim());
      setLookup({ symbol: res.symbol, covering_tests: res.covering_tests || [] });
    } catch (e) {
      toast.error(`Test-impact lookup failed: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const runBatchLookup = async () => {
    const symbols = batchSymbols
      .split(/[,\n]/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (symbols.length === 0) return;
    setLoading(true);
    try {
      const res = await api.semanticEngine.testImpactForSymbols(repoPath, symbols);
      setBatchResult(res.covering_tests || []);
    } catch (e) {
      toast.error(`Batch test-impact lookup failed: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const loadAll = async () => {
    setLoading(true);
    try {
      const res: TestImpactEntry[] = await api.semanticEngine.testImpactEntries(repoPath);
      setEntries(res || []);
    } catch (e) {
      toast.error(`Failed to load test-impact entries: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="text-[10px] text-muted-foreground">
        Which tests exercise a given symbol, from the Semantic Engine&apos;s dependency index (requires the Context
        Engine enabled for this repo, in the sidebar).
      </div>
      <div className="flex gap-1.5">
        <input
          className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
          placeholder="Symbol name (e.g. RoleProfileManager::create)"
          value={symbol}
          onChange={(e) => setSymbol(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && runLookup()}
        />
        <button onClick={runLookup} disabled={loading} aria-label="Look up covering tests" className="px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50">
          <Search className="w-3 h-3" />
        </button>
      </div>
      {lookup && (
        <div className="p-2 border rounded bg-background">
          <div className="font-medium">{lookup.symbol}</div>
          {lookup.covering_tests.length === 0 ? (
            <div className="text-muted-foreground">No known covering tests.</div>
          ) : (
            lookup.covering_tests.map((t) => (
              <div key={t} className="font-mono text-[10px]">
                {t}
              </div>
            ))
          )}
        </div>
      )}

      <div className="pt-2 border-t space-y-1.5">
        <div className="text-[10px] text-muted-foreground">
          Tests covering any of several symbols at once — e.g. every symbol touched by a patch.
        </div>
        <textarea
          className="w-full bg-background border rounded px-2 py-1 text-[11px] font-mono"
          placeholder={"One symbol per line, or comma-separated"}
          rows={2}
          value={batchSymbols}
          onChange={(e) => setBatchSymbols(e.target.value)}
        />
        <button onClick={runBatchLookup} disabled={loading} className="px-2 py-1 rounded bg-secondary disabled:opacity-50">
          Look up covering tests for all
        </button>
        {batchResult && (
          <div className="p-2 border rounded bg-background">
            {batchResult.length === 0 ? (
              <div className="text-muted-foreground">No known covering tests.</div>
            ) : (
              batchResult.map((t) => (
                <div key={t} className="font-mono text-[10px]">
                  {t}
                </div>
              ))
            )}
          </div>
        )}
      </div>

      <button onClick={loadAll} disabled={loading} className="px-2 py-1 rounded bg-secondary disabled:opacity-50">
        {entries ? "Refresh full table" : "Load full table"}
      </button>
      {entries && (
        <div className="space-y-1">
          {entries.length === 0 && <div className="text-muted-foreground">No entries indexed yet.</div>}
          {entries.map((e) => (
            <div key={e.symbol} className="p-2 border rounded bg-background">
              <div className="font-mono">{e.symbol}</div>
              <div className="text-[10px] text-muted-foreground">
                {e.covering_tests.length} covering test{e.covering_tests.length === 1 ? "" : "s"}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function DocsTab({ repoPath }: { repoPath: string }) {
  const [symbol, setSymbol] = useState("");
  const [docs, setDocs] = useState<string[] | null>(null);
  const [stale, setStale] = useState<StaleDocEntry[] | null>(null);
  const [loading, setLoading] = useState(false);

  const runLookup = async () => {
    if (!symbol.trim()) return;
    setLoading(true);
    try {
      const res = await api.semanticEngine.docsForSymbol(repoPath, symbol.trim());
      setDocs(res.docs || []);
    } catch (e) {
      toast.error(`Docs lookup failed: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  const loadStale = async () => {
    setLoading(true);
    try {
      const res: StaleDocEntry[] = await api.semanticEngine.docsStale(repoPath);
      setStale(res || []);
    } catch (e) {
      toast.error(`Failed to load stale docs: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="text-[10px] text-muted-foreground">
        Doc comments referencing a symbol, and docs that reference a symbol no longer found anywhere in the repo.
      </div>
      <div className="flex gap-1.5">
        <input
          className="flex-1 bg-background border rounded px-2 py-1 text-[11px]"
          placeholder="Symbol name"
          value={symbol}
          onChange={(e) => setSymbol(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && runLookup()}
        />
        <button onClick={runLookup} disabled={loading} aria-label="Look up docs for symbol" className="px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50">
          <Search className="w-3 h-3" />
        </button>
      </div>
      {docs && (
        <div className="p-2 border rounded bg-background space-y-1">
          {docs.length === 0 ? (
            <div className="text-muted-foreground">No docs reference this symbol.</div>
          ) : (
            docs.map((d, i) => (
              <div key={i} className="font-mono text-[10px] whitespace-pre-wrap">
                {d}
              </div>
            ))
          )}
        </div>
      )}

      <button onClick={loadStale} disabled={loading} className="flex items-center gap-1 px-2 py-1 rounded bg-secondary disabled:opacity-50">
        <FileWarning className="w-3 h-3" /> {stale ? "Refresh stale docs" : "Find stale docs"}
      </button>
      {stale && (
        <div className="space-y-1">
          {stale.length === 0 && <div className="text-muted-foreground">No stale docs found.</div>}
          {stale.map((s) => (
            <div key={s.doc_path} className="p-2 border rounded bg-background">
              <div className="font-mono">{s.doc_path}</div>
              <div className="text-[10px] text-yellow-600">references: {s.missing_symbols.join(", ")}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function BlameTab({ repoPath }: { repoPath: string }) {
  const [filePath, setFilePath] = useState("");
  const [blames, setBlames] = useState<GitBlameInfo[] | null>(null);
  const [loading, setLoading] = useState(false);

  const run = async () => {
    if (!filePath.trim()) return;
    setLoading(true);
    try {
      const result: GitBlameInfo[] = await api.semanticEngine.gitBlame(repoPath, filePath.trim());
      setBlames(result || []);
      // Cache what was just computed into the Semantic Engine's index, so a
      // later dependency-graph/search pass over this file doesn't recompute
      // blame from git it just fetched — the "load" half of git_blame/
      // load_blame this pair was clearly meant to be used together for.
      await api.semanticEngine.loadBlame(repoPath, filePath.trim(), result);
    } catch (e) {
      toast.error(`Blame lookup failed: ${e}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="text-[10px] text-muted-foreground">Per-line git blame for a file, relative to this repo&apos;s root.</div>
      <div className="flex gap-1.5">
        <input
          className="flex-1 bg-background border rounded px-2 py-1 text-[11px] font-mono"
          placeholder="src/lib.rs"
          value={filePath}
          onChange={(e) => setFilePath(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
        />
        <button onClick={run} disabled={loading} aria-label="Load blame for file" className="px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50">
          <Search className="w-3 h-3" />
        </button>
      </div>
      {blames && (
        <div className="space-y-0.5 max-h-96 overflow-y-auto">
          {blames.length === 0 && <div className="text-muted-foreground">No blame data.</div>}
          {blames.map((b) => (
            <div key={b.line} className="flex gap-2 p-1 border-b text-[10px] font-mono">
              <span className="text-muted-foreground w-8 text-right shrink-0">{b.line}</span>
              <span className="w-20 truncate shrink-0">{b.author}</span>
              <span className="w-16 truncate shrink-0 text-muted-foreground">{b.commit_hash.slice(0, 8)}</span>
              <span className="truncate">{b.commit_summary}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
