import { useState, useEffect } from "react";
import { useCid } from "@/hooks/useCid";
import { api, type JsonRpcNotification } from "@/lib/api";
import { toast } from "@/lib/dialog";

type HistoryEvent = {
  id: string;
  timestamp: string;
  actor: string;
  action: string;
  target: string;
  status: "success" | "failed" | "pending";
  details?: unknown;
};

export function HistoryPanel() {
  const { selectedMissionId } = useCid();
  const [events, setEvents] = useState<HistoryEvent[]>([]);
  const [filter, setFilter] = useState<"all" | "file" | "terminal" | "mcp" | "git">("all");

  useEffect(() => {
    // In Phase 0, history is derived from messages and tool calls
    // For now simulate with some events from notifications
    const handleNotif = (notif: JsonRpcNotification) => {
      if (notif.params?.mission_id !== selectedMissionId) return;

      const event: HistoryEvent = {
        id: `${Date.now()}-${Math.random()}`,
        timestamp: new Date().toISOString(),
        actor: "Implementer",
        action: notif.method,
        target: notif.params.tool_name || notif.params.pty_id || "mission",
        status: "success",
        details: notif.params,
      };

      if (["pty.output", "mission.message.delta"].includes(notif.method)) return; // noisy

      setEvents((prev) => [event, ...prev].slice(0, 100));
    };

    const unsub = api.onNotification(handleNotif);
    return () => unsub();
  }, [selectedMissionId]);

  if (!selectedMissionId) {
    return <div className="h-full flex items-center justify-center text-sm text-muted-foreground">Select a mission to view history</div>;
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
    a.download = `cid-history-${selectedMissionId}.json`;
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
        {filtered.length === 0 ? (
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
                <div className={`w-2 h-2 rounded-full ${ev.status === "success" ? "bg-green-500" : ev.status === "failed" ? "bg-red-500" : "bg-yellow-500"}`} />
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex gap-1.5 items-center">
                  <span className="font-medium">{ev.actor}</span>
                  <span className="text-muted-foreground">{ev.action}</span>
                  <span className="truncate">{ev.target}</span>
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
