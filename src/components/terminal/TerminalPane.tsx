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

export function TerminalPane() {
  const { selectedMissionId } = useCid();
  const theme = useTheme((s) => s.theme);
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const [ptyId, setPtyId] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);

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
    fitAddon.fit();

    termRef.current = term;
    fitRef.current = fitAddon;

    term.writeln("CID Terminal");
    term.writeln("Connecting to PTY...");

    const handleResize = () => fitAddon.fit();
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      term.dispose();
    };
  }, []);

  useEffect(() => {
    if (!selectedMissionId || !termRef.current) return;

    const initPty = async () => {
      try {
        const pty = await api.pty.create(selectedMissionId, 120, 30);
        setPtyId(pty.id);
        setIsConnected(true);
        termRef.current?.writeln(`\r\nPTY created: ${pty.id}\r\n`);

        // Setup data handler for user input — always the freshly-created
        // pty.id, not the ptyId state var: that closure would still read
        // null on this same render (setPtyId above hasn't committed yet).
        termRef.current?.onData((data) => {
          api.pty.write(pty.id, data).catch(console.error);
        });

        // Subscribe to PTY output via notifications
        const unsub = api.onNotification((notif) => {
          if (notif.method === "pty.output" && notif.params.pty_id === pty.id) {
            termRef.current?.write(notif.params.data);
          }
        });

        return () => unsub();
      } catch (e) {
        termRef.current?.writeln(`Failed to create PTY: ${e}\r\n`);
        console.error(e);
      }
    };

    initPty();
  }, [selectedMissionId]);

  const handleResize = useCallback(async () => {
    if (!fitRef.current || !ptyId) return;
    fitRef.current.fit();
    const dims = fitRef.current.proposeDimensions();
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

  if (!selectedMissionId) {
    return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a mission to open terminal</div>;
  }

  return (
    <div className="h-full flex flex-col bg-background">
      <div className="h-8 border-b border-border flex items-center px-3 text-xs text-muted-foreground">
        <span>PTY: {ptyId ? ptyId.slice(0, 8) : "connecting..."}</span>
        <span className="ml-auto flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${isConnected ? "bg-green-500" : "bg-yellow-500"}`} />
          {isConnected ? "Connected" : "Connecting"}
        </span>
      </div>
      <div ref={containerRef} className="flex-1" />
    </div>
  );
}
