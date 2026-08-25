import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useCid } from "@/hooks/useCid";
import { api } from "@/lib/api";
import { useTheme } from "@/theme/useTheme";

// xterm's canvas renderer needs a resolved color, not a `var(--x)` reference —
// tokens.json stores HSL triplets (e.g. "222 47% 4%") consumed elsewhere via
// `hsl(var(--x))`, so read the live custom property and wrap it the same way.
function readThemeColor(varName: string, fallback: string): string {
  if (typeof document === "undefined") return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(varName).trim();
  return value ? `hsl(${value})` : fallback;
}

function xtermTheme() {
  return {
    background: readThemeColor("--background", "#0a0e13"),
    foreground: readThemeColor("--foreground", "#e2e8f0"),
  };
}

/// Which tree the shell opens in.
///
/// A terminal is the one place a human runs commands that the agent neither
/// proposed nor sees, so the target is an explicit choice and the header states
/// the resolved path rather than inferring it. `session` keeps the shell inside
/// the Session's worktree — where its changes are captured by that Session's
/// checkpoints and show up in its diff. `repo` opens the main checkout, which is
/// outside that isolation; work done there belongs to no Session, which is why
/// it is flagged rather than merely labelled.
type TerminalTarget = { sessionId: string; workdir: "session" | "repo" };

export function TerminalPane() {
  const { selectedSessionId, sessions: allSessions, repos: allRepos } = useCid();
  // Defaulted for the same reason as useSessionRepoPath: these arrive
  // asynchronously and a first render can legitimately precede them.
  const sessions = allSessions ?? [];
  const repos = allRepos ?? [];
  const theme = useTheme((s) => s.theme);
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [ptyId, setPtyId] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  const [cwd, setCwd] = useState<string | null>(null);
  const [target, setTarget] = useState<TerminalTarget | null>(null);

  // Follow the selected Session by default; an explicit choice sticks until the
  // user selects a different Session.
  useEffect(() => {
    if (selectedSessionId) setTarget({ sessionId: selectedSessionId, workdir: "session" });
  }, [selectedSessionId]);

  const activeSession = sessions.find((s) => s.id === target?.sessionId);
  const isForeign = !!target && target.sessionId !== selectedSessionId;
  const isMainRepo = target?.workdir === "repo";
  const repoName = repos.find((r) => r.id === activeSession?.repo_channel_id)?.name;

  useEffect(() => {
    if (!containerRef.current) return;

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 13,
      fontFamily: "Menlo, Monaco, 'Courier New', monospace",
      theme: xtermTheme(),
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    // `fit()` reads the renderer's cell dimensions, which do not exist yet if
    // the pane is laid out at zero size when this lazy component mounts. It
    // throws "Cannot read properties of undefined (reading 'dimensions')" —
    // harmless, but it lands in the console on every open, and the 1s interval
    // below re-fits anyway once there is something to measure.
    const safeFit = () => {
      try {
        fitAddon.fit();
      } catch {
        /* not laid out yet */
      }
    };
    safeFit();

    termRef.current = term;
    fitRef.current = fitAddon;

    term.writeln("CID Terminal");
    term.writeln("Connecting to PTY...");

    const handleResize = () => safeFit();
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      term.dispose();
    };
  }, []);

  useEffect(() => {
    if (!target || !termRef.current) return;

    // Both handlers are torn down when the target changes. They used to be
    // registered inside the async body and their disposers returned from *it*,
    // which React never sees — so switching working directory left the previous
    // PTY's output still writing into the terminal and every keystroke being
    // sent to both shells.
    let disposed = false;
    let onDataDisposable: { dispose: () => void } | null = null;
    let unsub: (() => void) | null = null;

    const initPty = async () => {
      try {
        const pty = await api.pty.create(target.sessionId, 120, 30, target.workdir);
        if (disposed) return;
        setPtyId(pty.id);
        setIsConnected(true);
        // `pty.cwd` comes back from Core rather than being reconstructed here —
        // the shell's real directory is the only thing worth showing.
        setCwd(pty.cwd ?? null);
        termRef.current?.writeln(`\r\n${pty.cwd ?? "PTY"} (${pty.id.slice(0, 8)})\r\n`);

        // Always the freshly-created pty.id, not the ptyId state var: that
        // closure would still read null on this same render (setPtyId above
        // hasn't committed yet).
        onDataDisposable =
          termRef.current?.onData((data) => {
            api.pty.write(pty.id, data).catch(console.error);
          }) ?? null;

        unsub = api.onNotification((notif) => {
          if (notif.method === "pty.output" && notif.params.pty_id === pty.id) {
            termRef.current?.write(notif.params.data);
          }
        });
      } catch (e) {
        termRef.current?.writeln(`Failed to create PTY: ${e}\r\n`);
        console.error(e);
      }
    };

    initPty();
    return () => {
      disposed = true;
      onDataDisposable?.dispose();
      unsub?.();
    };
  }, [target]);

  const handleResize = useCallback(async () => {
    if (!fitRef.current || !ptyId) return;
    // `fit()` reads the renderer's cell dimensions, which do not exist while the
    // pane is detached — switching to another tab left this 1s interval firing
    // against a terminal with no element and threw
    // "Cannot read properties of undefined (reading 'dimensions')" into the
    // console every second.
    if (!termRef.current?.element?.isConnected) return;
    let dims: { cols: number; rows: number } | undefined;
    try {
      fitRef.current.fit();
      dims = fitRef.current.proposeDimensions();
    } catch {
      return;
    }
    if (dims) {
      try {
        await api.pty.resize(ptyId, dims.cols, dims.rows);
      } catch (e) {
        console.warn("[CID] PTY resize failed:", e);
      }
    }
  }, [ptyId]);

  useEffect(() => {
    const interval = setInterval(handleResize, 1000);
    return () => clearInterval(interval);
  }, [handleResize]);

  useEffect(() => {
    if (termRef.current) {
      termRef.current.options.theme = xtermTheme();
    }
  }, [theme]);

  if (!selectedSessionId) {
    return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a session to open terminal</div>;
  }

  const otherSessions = sessions.filter((s) => s.id !== selectedSessionId && s.worktree_path);

  const targetValue = target ? `${target.sessionId}::${target.workdir}` : "";
  const onTargetChange = (value: string) => {
    const [sessionId, workdir] = value.split("::");
    setCwd(null);
    setIsConnected(false);
    setTarget({ sessionId, workdir: workdir === "repo" ? "repo" : "session" });
  };

  return (
    <div className="h-full flex flex-col bg-background">
      <div className="h-8 border-b border-border flex items-center gap-2 px-3 text-xs text-muted-foreground">
        <select
          value={targetValue}
          onChange={(e) => onTargetChange(e.target.value)}
          aria-label="Terminal working directory"
          className="bg-background border rounded px-1.5 py-0.5 text-[11px] max-w-[45%]"
        >
          <option value={`${selectedSessionId}::session`}>This Session — worktree</option>
          <option value={`${selectedSessionId}::repo`}>Main repo{repoName ? ` (${repoName})` : ""} — outside this Session</option>
          {otherSessions.map((s) => (
            <option key={s.id} value={`${s.id}::session`}>
              Other Session — {s.title}
            </option>
          ))}
        </select>

        {/* Amber, not decorative: commands run here are not captured by the
            selected Session's checkpoints and will not appear in its diff. */}
        {(isMainRepo || isForeign) && (
          <span className="shrink-0 text-[10px] px-1 rounded bg-amber-500/20 text-amber-400">
            {isMainRepo ? "outside Session" : "other Session"}
          </span>
        )}

        <span className="truncate" title={cwd ?? undefined}>
          {cwd ?? (ptyId ? "" : "connecting…")}
        </span>

        <span className="ml-auto shrink-0 flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${isConnected ? "bg-green-500" : "bg-yellow-500"}`} />
          {isConnected ? "Connected" : "Connecting"}
        </span>
      </div>
      <div ref={containerRef} className="flex-1" />
    </div>
  );
}
