import { useState, useEffect } from "react";
import { useCid } from "@/hooks/useCid";
import { api, type JsonRpcNotification } from "@/lib/api";
import { toast } from "@/lib/dialog";

// There is no dedicated audit-log table in cid-core — history here is
// derived from what actually persists: `message.list`'s messages and each
// message's real `tool_calls` (id/name/arguments/status/result), plus
// notifications broadcast while this panel is open. Anything not
// recoverable from those two sources (e.g. which specific sub-role issued a
// live tool-call notification, once its `session.tool_call.complete` no
// longer carries `tool_name`) is labeled "unknown" rather than guessed.
type HistoryStatus = "pending_approval" | "approved" | "denied" | "running" | "completed" | "failed" | "unknown";

type HistoryEvent = {
  id: string;
  timestamp: string;
  actor: string;
  action: string;
  target: string;
  status: HistoryStatus;
  details?: unknown;
};

function capitalize(s: string): string {
  return s.length === 0 ? s : s[0].toUpperCase() + s.slice(1);
}

function describeTarget(args: unknown): string {
  if (args && typeof args === "object") {
    const a = args as Record<string, unknown>;
    const candidate = a.path ?? a.file_path ?? a.repo_path ?? a.dir_path ?? a.command ?? a.query ?? a.symbol;
    if (typeof candidate === "string") return candidate;
  }
  return "";
}

function eventsFromMessages(messages: RpcMessage[]): HistoryEvent[] {
  const events: HistoryEvent[] = [];
  for (const msg of messages) {
    for (const tc of msg.tool_calls ?? []) {
      events.push({
        id: tc.id,
        timestamp: msg.created_at,
        actor: capitalize(msg.role),
        action: tc.name,
        target: describeTarget(tc.arguments),
        status: (tc.status as HistoryStatus) ?? "unknown",
        details: { arguments: tc.arguments, result: tc.result },
      });
    }
  }
  // message.list returns oldest-first; newest-first matches how live events
  // get prepended below.
  return events.reverse();
}

type RpcToolCall = { id: string; name: string; arguments: unknown; status: string; result?: unknown };
type RpcMessage = { role: string; created_at: string; tool_calls?: RpcToolCall[] };

// Tool calls are always issued by the model executing a session turn — there
// is no persisted Planner/Implementer/Reviewer distinction at the
// notification layer, so "assistant" is the most specific honest label.
// Everything else broadcast on this socket (diff updates, pty output, plan
// changes, ...) isn't attributable to a role at all.
function actorForNotification(method: string): string {
  return method.startsWith("session.tool_call") ? "Assistant" : "System";
}

function statusForNotification(method: string): HistoryStatus {
  if (method === "session.tool_call.request") return "pending_approval";
  if (method === "session.tool_call.complete") return "completed";
  return "unknown";
}

const STATUS_COLOR: Record<HistoryStatus, string> = {
  completed: "bg-green-500",
  approved: "bg-green-500",
  failed: "bg-red-500",
  denied: "bg-red-500",
  running: "bg-yellow-500",
  pending_approval: "bg-yellow-500",
  unknown: "bg-muted-foreground/40",
};

export function HistoryPanel() {
  const { selectedSessionId } = useCid();
  const [events, setEvents] = useState<HistoryEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState<"all" | "file" | "terminal" | "mcp" | "git">("all");

  useEffect(() => {
    if (!selectedSessionId) {
      setEvents([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    api.message
      .list(selectedSessionId)
      .then((messages: RpcMessage[]) => {
        if (!cancelled) setEvents(eventsFromMessages(messages ?? []));
      })
      .catch((e) => {
        if (!cancelled) toast.error(`Failed to load history: ${e}`);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedSessionId]);

  useEffect(() => {
    const handleNotif = (notif: JsonRpcNotification) => {
      if (notif.params?.session_id !== selectedSessionId) return;
      if (["pty.output", "session.message.delta"].includes(notif.method)) return; // noisy

      const toolName = typeof notif.params?.tool_name === "string" ? notif.params.tool_name : null;
      const event: HistoryEvent = {
        id: `${Date.now()}-${Math.random()}`,
        timestamp: new Date().toISOString(),
        actor: actorForNotification(notif.method),
        // Prefer the real tool name when the notification carries one (it
        // matches the historical `action` values below and keeps the file/
        // terminal/git/mcp filters meaningful); otherwise the raw method
        // name is the most specific honest label available.
        action: toolName ?? notif.method,
        target: describeTarget(notif.params?.arguments) || notif.params?.pty_id || "",
        status: statusForNotification(notif.method),
        details: notif.params,
      };

      setEvents((prev) => [event, ...prev].slice(0, 100));
    };

    const unsub = api.onNotification(handleNotif);
    return () => unsub();
  }, [selectedSessionId]);

  if (!selectedSessionId) {
    return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a session to view history</div>;
  }

  const filtered = filter === "all" ? events : events.filter((e) => e.action.includes(filter));

  // Both buttons below rendered but did nothing on click (050-Gold-Standard-
  // Review.md-style dead-control gap, found while adding this component's
  // first tests) — real, minimal implementations rather than leaving them
  // decorative.
  const exportJson = () => {
    const blob = new Blob([JSON.stringify(filtered, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `cid-history-${selectedSessionId}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const copyMarkdown = async () => {
    const lines = filtered.map(
      (ev) => `- **${ev.actor}** \`${ev.action}\` ${ev.target} — ${new Date(ev.timestamp).toLocaleString()}`
    );
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      toast.success("History copied as Markdown");
    } catch (e) {
      toast.error(`Failed to copy: ${e}`);
    }
  };

  return (
    <div className="h-full flex flex-col">
      <div className="h-10 border-b flex items-center px-3 gap-2">
        <span className="text-sm font-medium">History</span>
        <div className="ml-auto flex gap-1">
          {(["all", "file", "terminal", "git", "mcp"] as const).map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`text-[11px] px-2 py-1 rounded capitalize ${filter === f ? "bg-accent text-accent-foreground" : "hover:bg-accent/50 text-muted-foreground"}`}
            >
              {f}
            </button>
          ))}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto divide-y">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">Loading history…</div>
        ) : filtered.length === 0 ? (
          <div className="p-4 text-xs text-muted-foreground">
            No history yet. History logs every tool call, terminal command, file edit, and MCP call.
            <div className="mt-2 p-2 bg-card rounded border text-[11px]">
              This panel will show:
              <ul className="list-disc ml-4 mt-1 space-y-0.5">
                <li>File reads/writes</li>
                <li>Terminal commands (with secret redaction)</li>
                <li>Git operations</li>
                <li>MCP tool calls as inline cards</li>
                <li>Approval events</li>
              </ul>
            </div>
          </div>
        ) : (
          filtered.map((ev) => (
            <div key={ev.id} className="p-2.5 flex gap-2 text-xs hover:bg-accent/30">
              <div className="shrink-0 mt-0.5">
                <div className={`w-2 h-2 rounded-full ${STATUS_COLOR[ev.status]}`} title={ev.status} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex gap-1.5 items-center">
                  <span className="font-medium">{ev.actor}</span>
                  <span className="text-muted-foreground">{ev.action}</span>
                  <span className="truncate">{ev.target}</span>
                  <span className="text-[10px] text-muted-foreground">{ev.status.replace(/_/g, " ")}</span>
                  <span className="ml-auto text-[10px] text-muted-foreground">{new Date(ev.timestamp).toLocaleTimeString()}</span>
                </div>
                {Boolean(ev.details) && (
                  <pre className="mt-1 text-[11px] bg-background p-1.5 rounded overflow-x-auto max-h-24">{JSON.stringify(ev.details, null, 2)}</pre>
                )}
              </div>
            </div>
          ))
        )}
      </div>

      <div className="p-2 border-t flex gap-2">
        <button onClick={exportJson} disabled={filtered.length === 0} className="text-[11px] px-2 py-1 rounded bg-secondary disabled:opacity-50">
          Export JSON
        </button>
        <button onClick={copyMarkdown} disabled={filtered.length === 0} className="text-[11px] px-2 py-1 rounded bg-secondary disabled:opacity-50">
          Copy as Markdown
        </button>
      </div>
    </div>
  );
}
