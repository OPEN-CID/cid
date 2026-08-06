import { useEffect, useState, useCallback } from "react";
import { useCid } from "@/hooks/useCid";
import { api } from "@/lib/api";
import {
  Wifi,
  WifiOff,
  AlertTriangle,
  RefreshCw,
  Shield,
  Users,
  Clock,
  Activity,
  Server,
  ChevronDown,
  ChevronUp,
} from "lucide-react";

type ConnectionStatus = "connected" | "connecting" | "disconnected" | "error";

interface HealthInfo {
  uptime: number;
  connectedClients: number;
  activeMissions: number;
  platform: string;
}

export function WebShellProvider({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

/** Connection status banner shown at the top of the app */
export function ConnectionBanner() {
  const { connected } = useCid();
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  const [retryCount, setRetryCount] = useState(0);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (connected) {
      setStatus("connected");
      setRetryCount(0);
    } else {
      setStatus("disconnected");
    }
  }, [connected]);

  const handleRetry = useCallback(async () => {
    setStatus("connecting");
    setRetryCount((c) => c + 1);
    try {
      await api.connect();
    } catch {
      setStatus("error");
    }
  }, []);

  // Exponential backoff retry
  useEffect(() => {
    if (status === "disconnected" || status === "error") {
      const delay = Math.min(1000 * Math.pow(2, retryCount), 30000);
      const timer = setTimeout(handleRetry, delay);
      return () => clearTimeout(timer);
    }
  }, [status, retryCount, handleRetry]);

  const statusConfig = {
    connected: {
      icon: Wifi,
      color: "bg-green-500",
      border: "border-green-500/30",
      bg: "bg-green-500/10",
      text: "text-green-200",
      label: "Core connected",
    },
    connecting: {
      icon: RefreshCw,
      color: "bg-yellow-500 animate-spin",
      border: "border-yellow-500/30",
      bg: "bg-yellow-500/10",
      text: "text-yellow-200",
      label: "Connecting to Core...",
    },
    disconnected: {
      icon: WifiOff,
      color: "bg-red-500",
      border: "border-red-500/30",
      bg: "bg-red-500/10",
      text: "text-red-200",
      label: "Core offline",
    },
    error: {
      icon: AlertTriangle,
      color: "bg-orange-500",
      border: "border-orange-500/30",
      bg: "bg-orange-500/10",
      text: "text-orange-200",
      label: "Connection error",
    },
  };

  const config = statusConfig[status];
  const Icon = config.icon;

  if (status === "connected") return null;

  return (
    <div
      className={`fixed top-0 left-0 right-0 z-50 ${config.bg} border-b ${config.border} backdrop-blur-sm`}
    >
      <div className="flex items-center gap-3 px-4 py-2.5">
        <span className={`w-2.5 h-2.5 rounded-full ${config.color}`} />
        <Icon className={`w-4 h-4 ${config.text}`} />
        <span className={`text-sm font-medium ${config.text}`}>{config.label}</span>
        <span className="text-xs text-muted-foreground">
          {retryCount > 0 ? `Retry #${retryCount}` : ""}
        </span>
        <button
          onClick={handleRetry}
          className={`ml-auto flex items-center gap-1 text-xs ${config.text} hover:underline`}
        >
          <RefreshCw className={`w-3 h-3 ${status === "connecting" ? "animate-spin" : ""}`} />
          Reconnect
        </button>
        <button
          onClick={() => setExpanded(!expanded)}
          className="text-muted-foreground hover:text-foreground"
          aria-label={expanded ? "Collapse connection details" : "Expand connection details"}
          aria-expanded={expanded}
        >
          {expanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        </button>
      </div>
      {expanded && (
        <div className="px-4 pb-3 space-y-2 border-t border-border/50">
          <p className="text-xs text-muted-foreground">
            Start Core with <code className="bg-muted px-1.5 py-0.5 rounded text-[11px]">cargo run -p cid-core -- --port 5919</code> or{" "}
            <code className="bg-muted px-1.5 py-0.5 rounded text-[11px]">npm run dev:core</code>
          </p>
          <p className="text-xs text-muted-foreground">
            Target: <span className="text-foreground">ws://127.0.0.1:5919/ws</span>
          </p>
        </div>
      )}
    </div>
  );
}

/** Health dashboard showing Core stats */
export function HealthDashboard() {
  const { connected } = useCid();
  const [health, setHealth] = useState<HealthInfo | null>(null);
  const [showDashboard, setShowDashboard] = useState(false);

  const fetchHealth = useCallback(async () => {
    try {
      const resp = await fetch("http://127.0.0.1:5919/health");
      const data = await resp.json();
      setHealth({
        uptime: data.uptime || 0,
        connectedClients: data.connected_clients || 0,
        activeMissions: data.active_missions || 0,
        platform: data.platform || navigator.platform,
      });
    } catch {
      setHealth(null);
    }
  }, []);

  useEffect(() => {
    if (connected) {
      fetchHealth();
      const interval = setInterval(fetchHealth, 15000);
      return () => clearInterval(interval);
    }
  }, [connected, fetchHealth]);

  const formatUptime = (seconds: number) => {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = seconds % 60;
    if (h > 0) return `${h}h ${m}m`;
    if (m > 0) return `${m}m ${s}s`;
    return `${s}s`;
  };

  return (
    <div className="relative">
      <button
        onClick={() => setShowDashboard(!showDashboard)}
        className="flex items-center gap-1.5 text-[11px] text-muted-foreground hover:text-foreground transition-colors"
      >
        <Activity className="w-3 h-3" />
        {connected ? "Healthy" : "Offline"}
        {showDashboard ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
      </button>

      {showDashboard && (
        <div className="absolute right-0 bottom-full mb-2 w-64 bg-card border rounded-lg shadow-lg p-4 z-50">
          <h4 className="font-medium text-sm mb-3 flex items-center gap-2">
            <Server className="w-4 h-4" />
            Core Health
          </h4>
          <div className="space-y-2 text-sm">
            <div className="flex justify-between">
              <span className="text-muted-foreground flex items-center gap-1.5">
                <Wifi className="w-3 h-3" />
                Status
              </span>
              <span
                className={connected ? "text-green-400 font-medium" : "text-red-400 font-medium"}
              >
                {connected ? "Online" : "Offline"}
              </span>
            </div>
            {health && (
              <>
                <div className="flex justify-between">
                  <span className="text-muted-foreground flex items-center gap-1.5">
                    <Clock className="w-3 h-3" />
                    Uptime
                  </span>
                  <span className="font-medium">{formatUptime(health.uptime)}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground flex items-center gap-1.5">
                    <Users className="w-3 h-3" />
                    Clients
                  </span>
                  <span className="font-medium">{health.connectedClients}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground flex items-center gap-1.5">
                    <Activity className="w-3 h-3" />
                    Active Missions
                  </span>
                  <span className="font-medium">{health.activeMissions}</span>
                </div>
              </>
            )}
          </div>
          <button
            onClick={fetchHealth}
            className="mt-3 w-full text-xs flex items-center justify-center gap-1 py-1.5 rounded bg-secondary hover:bg-accent transition-colors"
          >
            <RefreshCw className="w-3 h-3" /> Refresh
          </button>
        </div>
      )}
    </div>
  );
}


/**
 * Access control, reported from Core rather than kept as local UI state.
 *
 * Whether a token is required is decided by how Core was launched — it is a
 * startup policy, not a runtime toggle, so this panel reports and explains it
 * instead of pretending to change it.
 */
export function AccessControlPanel() {
  const [health, setHealth] = useState<{
    auth_required?: boolean;
    loopback_only?: boolean;
    connected_clients?: number;
    version?: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const port = import.meta.env.VITE_CID_CORE_PORT || "5919";
    const host = import.meta.env.VITE_CID_CORE_HOST || "127.0.0.1";
    const fetchHealth = async () => {
      try {
        const resp = await fetch(`http://${host}:${port}/health`);
        setHealth(await resp.json());
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    };
    fetchHealth();
    const interval = setInterval(fetchHealth, 5000);
    return () => clearInterval(interval);
  }, []);

  const loopbackOnly = health?.loopback_only ?? true;
  const authRequired = health?.auth_required ?? false;
  const exposedWithoutAuth = !loopbackOnly && !authRequired;

  return (
    <div className="p-4 space-y-4">
      <h3 className="font-medium text-sm flex items-center gap-2">
        <Shield className="w-4 h-4" />
        Access Control
      </h3>

      {error && (
        <div className="text-xs text-muted-foreground border rounded p-2">
          Core unreachable: {error}
        </div>
      )}

      <div className="space-y-2 text-sm">
        <div className="flex items-center justify-between border rounded px-2.5 py-2">
          <div>
            <div className="font-medium">Reachable from other machines</div>
            <div className="text-xs text-muted-foreground">
              Set by Core&apos;s <code>--host</code> flag at startup
            </div>
          </div>
          <span
            className={`text-xs px-2 py-0.5 rounded ${
              loopbackOnly ? "bg-muted text-muted-foreground" : "bg-yellow-500/20 text-yellow-300"
            }`}
          >
            {loopbackOnly ? "loopback only" : "remote"}
          </span>
        </div>

        <div className="flex items-center justify-between border rounded px-2.5 py-2">
          <div>
            <div className="font-medium">Bearer token required</div>
            <div className="text-xs text-muted-foreground">
              Set by <code>--auth-token</code> or <code>CID_AUTH_TOKEN</code>
            </div>
          </div>
          <span
            className={`text-xs px-2 py-0.5 rounded ${
              authRequired ? "bg-green-500/20 text-green-300" : "bg-muted text-muted-foreground"
            }`}
          >
            {authRequired ? "required" : "none"}
          </span>
        </div>

        {exposedWithoutAuth && (
          <div className="flex gap-2 border border-red-500/50 bg-red-500/10 rounded px-2.5 py-2 text-xs text-red-200">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            <span>
              Core is reachable beyond localhost with no token. Restart it with{" "}
              <code>--auth-token</code>.
            </span>
          </div>
        )}

        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Users className="w-3 h-3" />
          Connected clients:{" "}
          <span className="font-medium text-foreground">{health?.connected_clients ?? 0}</span>
        </div>
      </div>

      <div className="text-xs text-muted-foreground border-t pt-2">
        To expose Core to a team server:
        <pre className="mt-1 bg-background rounded p-2 overflow-x-auto">
{`cid-core --host 0.0.0.0 --auth-token "$(cid-core --generate-token)"`}
        </pre>
        Core refuses to start on a non-loopback address without a token.
      </div>
    </div>
  );
}

/** Loading skeleton for main content while connecting */
export function LoadingSkeleton() {
  return (
    <div className="flex-1 flex items-center justify-center">
      <div className="space-y-6 text-center">
        <div className="w-16 h-16 mx-auto rounded-full bg-muted animate-pulse" />
        <div className="space-y-2">
          <div className="h-4 w-48 mx-auto bg-muted rounded animate-pulse" />
          <div className="h-3 w-32 mx-auto bg-muted rounded animate-pulse" />
        </div>
        <div className="space-y-3 w-64">
          <div className="h-2 bg-muted rounded animate-pulse" />
          <div className="h-2 bg-muted rounded animate-pulse w-3/4 mx-auto" />
          <div className="h-2 bg-muted rounded animate-pulse w-1/2 mx-auto" />
        </div>
      </div>
    </div>
  );
}

/** Bottom status bar with connection indicator */
export function ConnectionStatusBar() {
  const { connected } = useCid();
  return (
    <div className="h-7 border-t bg-card flex items-center px-3 text-[11px] text-muted-foreground gap-4">
      <div className="flex items-center gap-1.5">
        <span
          className={`w-2 h-2 rounded-full ${connected ? "bg-green-500" : "bg-red-500 animate-pulse"}`}
        />
        <span>
          Core: {connected ? "connected" : "offline"} (ws://127.0.0.1:5919)
        </span>
      </div>
      <span className="ml-auto">CID Phase 2 • Tauri v2 • Rust core</span>
      <HealthDashboard />
    </div>
  );
}