import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { useCid } from "../../hooks/useCid";
import { RefreshCw, AlertTriangle, Copy } from "lucide-react";
import { TestImpactTab, DocsTab, BlameTab } from "./SemanticInsights";

type ModuleTestStats = { module_path: string; fn_count: number; test_count: number };
type DuplicateTestGroup = { tests: string[]; body_preview: string };
type RepoHealthReport = {
  modules: ModuleTestStats[];
  total_fns: number;
  total_tests: number;
  untested_modules: string[];
  duplicate_test_groups: DuplicateTestGroup[];
};

/**
 * Repository Health (Phase 6): a signal-based view over the current repo's own
 * test suite — untested modules and duplicate/redundant tests — not
 * instrumented line coverage (this repo has no coverage-tooling build step
 * wired up yet; that's a real, named gap, not faked with a plausible number).
 */
type Tab = "tests" | "test_impact" | "docs" | "blame";

export function RepoHealthPanel() {
  const { repos, selectedRepoId } = useCid();
  const [report, setReport] = useState<RepoHealthReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("tests");

  const repoPath = repos.find((r) => r.id === selectedRepoId)?.path;

  const scan = useCallback(async () => {
    if (!repoPath) return;
    setLoading(true);
    setError(null);
    try {
      setReport(await api.repoHealth.scan(repoPath));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [repoPath]);

  useEffect(() => {
    scan();
  }, [scan]);

  if (!repoPath) {
    return (
      <div className="p-4 text-xs text-muted-foreground">Select a repo channel to see its health.</div>
    );
  }

  return (
    <div className="p-4 space-y-4 overflow-y-auto h-full text-xs">
      <div className="flex items-center justify-between">
        <div className="font-medium">Repository Health</div>
        {tab === "tests" && (
          <button
            onClick={scan}
            disabled={loading}
            className="flex items-center gap-1 px-2 py-0.5 rounded bg-secondary disabled:opacity-50"
          >
            <RefreshCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} /> Rescan
          </button>
        )}
      </div>

      <div className="flex gap-1 border-b pb-2">
        {(["tests", "test_impact", "docs", "blame"] as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`px-2 py-1 rounded capitalize ${tab === t ? "bg-accent" : "text-muted-foreground hover:bg-accent/50"}`}
          >
            {t.replace("_", " ")}
          </button>
        ))}
      </div>

      {tab === "test_impact" && <TestImpactTab repoPath={repoPath} />}
      {tab === "docs" && <DocsTab repoPath={repoPath} />}
      {tab === "blame" && <BlameTab repoPath={repoPath} />}

      {tab === "tests" && (
        <>
      {error && <div className="text-red-500">{error}</div>}

      {report && (
        <>
          <div className="grid grid-cols-3 gap-2">
            <Stat label="Functions" value={report.total_fns} />
            <Stat label="Tests" value={report.total_tests} />
            <Stat
              label="Untested modules"
              value={report.untested_modules.length}
              warn={report.untested_modules.length > 0}
            />
          </div>

          <div className="text-[10px] text-muted-foreground">
            &quot;Tests&quot; counts <code>#[test]</code> functions found in the source — a signal that an
            area has some test presence, not instrumented line coverage. Wiring real coverage
            (tarpaulin/llvm-cov) is a known gap, tracked rather than approximated here.
          </div>

          {report.untested_modules.length > 0 && (
            <div>
              <div className="flex items-center gap-1 font-medium mb-1">
                <AlertTriangle className="w-3 h-3 text-yellow-500" /> Modules with no tests
              </div>
              <div className="space-y-0.5">
                {report.untested_modules.map((m) => (
                  <div key={m} className="font-mono text-[10px] text-muted-foreground truncate">
                    {m}
                  </div>
                ))}
              </div>
            </div>
          )}

          {report.duplicate_test_groups.length > 0 && (
            <div>
              <div className="flex items-center gap-1 font-medium mb-1">
                <Copy className="w-3 h-3 text-yellow-500" /> Likely duplicate tests
              </div>
              <div className="space-y-1.5">
                {report.duplicate_test_groups.map((g, i) => (
                  <div key={i} className="p-2 border rounded bg-background">
                    {g.tests.map((t) => (
                      <div key={t} className="font-mono text-[10px]">
                        {t}
                      </div>
                    ))}
                    <div className="text-[10px] text-muted-foreground mt-1 truncate">
                      {g.body_preview}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          {report.untested_modules.length === 0 && report.duplicate_test_groups.length === 0 && (
            <div className="text-muted-foreground">No untested modules or duplicate tests found.</div>
          )}
        </>
      )}
        </>
      )}
    </div>
  );
}

function Stat({ label, value, warn }: { label: string; value: number; warn?: boolean }) {
  return (
    <div className="p-2 border rounded bg-background">
      <div className={`text-lg font-semibold ${warn ? "text-yellow-500" : ""}`}>{value}</div>
      <div className="text-[10px] text-muted-foreground">{label}</div>
    </div>
  );
}
