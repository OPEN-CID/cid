import { useEffect, useMemo, useRef, useState } from "react";
import { useCid, type PendingApproval } from "@/hooks/useCid";
import { api } from "@/lib/api";
import { PlanCard } from "./PlanCard";
import { ReviewCard } from "./ReviewCard";
import { CheckpointCard } from "./CheckpointCard";
import { AgentsMdReviewCard } from "./AgentsMdReviewCard";
import { McpAppCard, McpToolResultCard, extractMcpAppContent } from "../McpAppCard";
import { Send, Loader2, Check, X, Terminal as TerminalIcon } from "lucide-react";

type ContextUsage = {
  used_tokens: number;
  window_tokens: number;
  ratio: number;
  provider: string;
  model: string;
  compaction_recommended: boolean;
};

export function ChatThread() {
  const { selectedMissionId, messages, addMessage, updateMessage, loadMessages } = useCid();
  const [input, setInput] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [pendingApprovals, setPendingApprovals] = useState<PendingApproval[]>([]);
  const [contextUsage, setContextUsage] = useState<ContextUsage | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  const missionMessages = useMemo(
    () => (selectedMissionId ? messages[selectedMissionId] || [] : []),
    [selectedMissionId, messages]
  );

  const refreshContextUsage = async (missionId: string) => {
    try {
      setContextUsage(await api.mission.contextUsage(missionId));
    } catch {
      // Not fatal — the indicator just stays hidden if this fails.
    }
  };

  useEffect(() => {
    if (selectedMissionId) refreshContextUsage(selectedMissionId);
  }, [selectedMissionId, missionMessages.length]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [missionMessages]);

  useEffect(() => {
    // Subscribe to notifications
    const unsub = api.onNotification((notif) => {
      if (notif.method === "mission.message.delta" && notif.params.mission_id === selectedMissionId) {
        const { message_id, delta } = notif.params;
        const current = (messages[selectedMissionId || ""] || []).find((m) => m.id === message_id);
        if (current) {
          updateMessage(selectedMissionId!, message_id, { content: current.content + delta });
        } else {
          // Create streaming message if not exists
          addMessage(selectedMissionId!, {
            id: message_id,
            mission_id: selectedMissionId!,
            role: "assistant",
            content: delta,
            tool_calls: [],
            created_at: new Date().toISOString(),
            is_streaming: true,
          });
        }
      } else if (notif.method === "mission.message.new" && notif.params.mission_id === selectedMissionId) {
        addMessage(selectedMissionId!, {
          id: `msg-${Date.now()}`,
          mission_id: selectedMissionId!,
          role: "assistant",
          content: notif.params.content,
          tool_calls: [],
          created_at: new Date().toISOString(),
        });
      } else if (notif.method === "mission.tool_call.request" && notif.params.mission_id === selectedMissionId) {
        setPendingApprovals((prev) => [...prev, notif.params]);
      } else if (notif.method === "mission.message.complete" && notif.params.mission_id === selectedMissionId) {
        const { message_id, content } = notif.params;
        updateMessage(selectedMissionId!, message_id, { content, is_streaming: false });
      }
    });

    return () => unsub();
  }, [selectedMissionId, messages, addMessage, updateMessage]);

  const handleCompactCommand = async (missionId: string) => {
    setInput("");
    setIsSending(true);
    try {
      const result = await api.mission.contextCompact(missionId);
      const summaryText = result.digest
        ? `Context compacted — older messages summarized to keep this Mission within its context budget. Full history is still visible above; this summary is what future turns actually send to the model.`
        : `Nothing to compact yet — this Mission's recent messages are already all that would be kept.`;
      addMessage(missionId, {
        id: `compact-${Date.now()}`,
        mission_id: missionId,
        role: "system",
        content: summaryText,
        tool_calls: [],
        created_at: new Date().toISOString(),
      });
      await loadMessages(missionId);
      await refreshContextUsage(missionId);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSending(false);
    }
  };

  const handleSend = async () => {
    if (!input.trim() || !selectedMissionId) return;
    const content = input.trim();

    // `/compact` — the manual trigger for context compaction (review_prompt.md
    // §3.1); everything else still goes to the Mission as a normal message.
    if (content === "/compact") {
      await handleCompactCommand(selectedMissionId);
      return;
    }

    setInput("");
    setIsSending(true);

    try {
      const userMsg = {
        id: `tmp-${Date.now()}`,
        mission_id: selectedMissionId,
        role: "user" as const,
        content,
        tool_calls: [],
        created_at: new Date().toISOString(),
      };
      addMessage(selectedMissionId, userMsg);

      await api.mission.sendMessage(selectedMissionId, content);
      // Reload after a bit
      setTimeout(() => loadMessages(selectedMissionId), 500);
    } catch (e) {
      console.error(e);
      addMessage(selectedMissionId!, {
        id: `err-${Date.now()}`,
        mission_id: selectedMissionId!,
        role: "system",
        content: `Failed to send: ${e}`,
        tool_calls: [],
        created_at: new Date().toISOString(),
      });
    } finally {
      setIsSending(false);
    }
  };

  const handleApprove = async (toolCallId: string, approved: boolean) => {
    if (!selectedMissionId) return;
    try {
      await api.mission.approveTool(selectedMissionId, toolCallId, approved);
      setPendingApprovals((prev) => prev.filter((p) => p.tool_call_id !== toolCallId));
    } catch (e) {
      console.error(e);
    }
  };

  if (!selectedMissionId) {
    return (
      <div className="flex-1 flex items-center justify-center p-8 text-center">
        <div className="max-w-md">
          <h2 className="text-lg font-semibold mb-2">No mission selected</h2>
          <p className="text-sm text-muted-foreground mb-4">
            Select a mission from the left rail or create a new one to start chatting with CID.
          </p>
          <div className="text-xs text-muted-foreground bg-card p-3 rounded border text-left">
            <div className="font-mono">Flow 1 – First Mission (golden path):</div>
            <ol className="list-decimal ml-4 mt-2 space-y-1">
              <li>Connect a local git repo</li>
              <li>Click &quot;New Mission&quot; → choose worktree/shared</li>
              <li>Type a task – Planner responds with a plan</li>
              <li>Approve plan steps – Implementer executes with your approval per tool call</li>
              <li>Review diff in right panel – per-hunk accept/reject</li>
              <li>Merge or open PR</li>
            </ol>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        <PlanCard missionId={selectedMissionId} />
        <AgentsMdReviewCard missionId={selectedMissionId} />
        <ReviewCard missionId={selectedMissionId} />
        <CheckpointCard missionId={selectedMissionId} refreshOn={missionMessages.length} />

        {/* 051 Wave 5.2: a screen reader had no way to know a streaming
            response was arriving — polite so mid-stream deltas don't
            interrupt whatever the user is doing. */}
        <div aria-live="polite" role="log" className="space-y-4">
        {missionMessages.map((msg) => (
          <div key={msg.id} className={`flex gap-3 ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
            <div
              className={`max-w-[85%] rounded-lg px-4 py-2 text-sm ${
                msg.role === "user"
                  ? "bg-primary text-primary-foreground"
                  : msg.role === "system"
                  ? "bg-destructive/20 border border-destructive/50"
                  : "bg-card border"
              }`}
            >
              <div className="whitespace-pre-wrap">{msg.content}</div>
              {msg.tool_calls && msg.tool_calls.length > 0 && (
                <div className="mt-2 space-y-2">
                  {msg.tool_calls.map((tc) => {
                    // A server that implements the MCP Apps extension gets its own
                    // UI rendered inline; everything else falls back to the plain
                    // result card rather than a half-rendered app.
                    const appContent = tc.result ? extractMcpAppContent(tc.result) : null;
                    return (
                      <div key={tc.id} className="bg-accent/50 rounded p-2 text-xs font-mono">
                        <div className="flex items-center gap-2">
                          <TerminalIcon className="w-3 h-3" />
                          <span>{tc.name}</span>
                          <span
                            className={`ml-auto px-1 rounded text-[10px] ${
                              tc.status === "completed" ? "bg-green-500/20" : "bg-yellow-500/20"
                            }`}
                          >
                            {tc.status}
                          </span>
                        </div>
                        {tc.provenance && (
                          // review_prompt.md §1.2 point 3: provenance marker —
                          // this call was made while untrusted repo content was
                          // in context, not proof it was actually influenced.
                          <div
                            className="mt-1 text-[10px] text-yellow-600 dark:text-yellow-400"
                            title={tc.provenance}
                          >
                            ⚠ untrusted content was in context for this call
                          </div>
                        )}
                        <pre className="mt-1 overflow-x-auto">{JSON.stringify(tc.arguments, null, 2)}</pre>
                        {appContent ? (
                          <McpAppCard
                            serverId={tc.server_id ?? ""}
                            serverName={tc.name}
                            content={appContent}
                          />
                        ) : (
                          Boolean(tc.result) && <McpToolResultCard toolName={tc.name} result={tc.result} />
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          </div>
        ))}
        </div>

        {/* Pending approvals */}
        {pendingApprovals.map((approval) => (
          <div key={approval.tool_call_id} className="border border-orange-500/50 bg-orange-500/10 rounded-lg p-3">
            <div className="flex items-center gap-2 text-sm font-semibold">
              <span className="w-2 h-2 bg-orange-500 rounded-full animate-pulse" />
              Approval required: {approval.tool_name}
            </div>
            <pre className="text-xs bg-background mt-2 p-2 rounded overflow-x-auto">{JSON.stringify(approval.arguments, null, 2)}</pre>
            <div className="flex gap-2 mt-2">
              <button
                onClick={() => handleApprove(approval.tool_call_id, true)}
                className="flex items-center gap-1 bg-green-600 hover:bg-green-700 text-white text-xs px-3 py-1 rounded"
              >
                <Check className="w-3 h-3" /> Approve
              </button>
              <button
                onClick={() => handleApprove(approval.tool_call_id, false)}
                className="flex items-center gap-1 bg-red-600 hover:bg-red-700 text-white text-xs px-3 py-1 rounded"
              >
                <X className="w-3 h-3" /> Deny
              </button>
            </div>
          </div>
        ))}

        <div ref={bottomRef} />
      </div>

      {/* Composer */}
      <div className="p-3 border-t bg-card/50">
        <div className="flex gap-2 items-end">
          <textarea
            className="flex-1 bg-background border rounded-lg px-3 py-2 text-sm resize-none min-h-[44px] max-h-32 outline-none focus:ring-1 focus:ring-ring"
            placeholder="Type a task, question, or @mention an MCP tool..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
            rows={1}
          />
          <button
            onClick={handleSend}
            disabled={isSending || !input.trim()}
            aria-label="Send message"
            className="bg-primary text-primary-foreground p-2.5 rounded-lg hover:bg-primary/90 disabled:opacity-50"
          >
            {isSending ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />}
          </button>
        </div>
        <div className="text-[11px] text-muted-foreground mt-2 flex gap-3 items-center">
          <span>↵ Send • Shift+↵ New line</span>
          <span>• @ MCP tools • /compact</span>
          {contextUsage && (
            <span
              className={contextUsage.compaction_recommended ? "text-yellow-500" : ""}
              title={`~${contextUsage.used_tokens.toLocaleString()} / ${contextUsage.window_tokens.toLocaleString()} tokens (${contextUsage.provider} ${contextUsage.model})`}
            >
              • context: {Math.round(contextUsage.ratio * 100)}%
              {contextUsage.compaction_recommended ? " — try /compact" : ""}
            </span>
          )}
          <span className="ml-auto">Co-Pilot mode: every tool call requires approval</span>
        </div>
      </div>
    </div>
  );
}
