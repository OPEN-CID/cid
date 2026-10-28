import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useCid, type PendingApproval } from "@/hooks/useCid";
import { api } from "@/lib/api";
import { extractMentions, mentionQueryAt } from "@/lib/mentions";

type McpToolOption = { server: string; serverId: string; tool: string; description?: string };
import { PlanCard } from "./PlanCard";
import { SessionModelBar } from "./SessionModelBar";
import { ReviewCard } from "./ReviewCard";
import { CheckpointCard } from "./CheckpointCard";
import { AgentsMdReviewCard } from "./AgentsMdReviewCard";
import { McpAppCard, McpToolResultCard, extractMcpAppContent } from "../McpAppCard";
import { MessageSegments } from "../artifacts/ArtifactCard";
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
  const { selectedSessionId, messages, addMessage, updateMessage, loadMessages } = useCid();
  const [input, setInput] = useState("");
  const [isSending, setIsSending] = useState(false);
  const [pendingApprovals, setPendingApprovals] = useState<PendingApproval[]>([]);
  const [contextUsage, setContextUsage] = useState<ContextUsage | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  // @mention MCP tools: autocomplete options plus the open query.
  const [toolOptions, setToolOptions] = useState<McpToolOption[]>([]);
  const [mentionOpen, setMentionOpen] = useState(false);
  const [mentionQuery, setMentionQuery] = useState("");
  const [mentionIndex, setMentionIndex] = useState(0);

  const loadToolOptions = useCallback(async () => {
    try {
      const servers = (await api.mcp.list()) as { id: string; name: string }[] | null;
      if (!Array.isArray(servers)) return;
      const all: McpToolOption[] = [];
      for (const s of servers) {
        try {
          const tools = (await api.mcp.tools(s.id)) as { name: string; description?: string }[];
          for (const t of Array.isArray(tools) ? tools : []) {
            all.push({ server: s.name, serverId: s.id, tool: t.name, description: t.description });
          }
        } catch {
          // One unreachable server must not hide every other server's tools.
        }
      }
      setToolOptions(all);
    } catch {
      // No MCP servers configured — the @ menu simply stays empty.
    }
  }, []);

  const mentionMatches = useMemo(() => {
    const q = mentionQuery.toLowerCase();
    return toolOptions
      .filter((t) => !q || `${t.server}.${t.tool}`.toLowerCase().includes(q))
      .slice(0, 8);
  }, [toolOptions, mentionQuery]);

  const closeMention = useCallback(() => {
    setMentionOpen(false);
    setMentionIndex(0);
  }, []);

  const completeMention = useCallback(
    (opt: McpToolOption) => {
      const el = textareaRef.current;
      if (!el) return;
      const caret = el.selectionStart ?? input.length;
      const at = input.slice(0, caret).lastIndexOf("@");
      if (at < 0) return;
      const completed = `${input.slice(0, at)}@${opt.server}.${opt.tool} ${input.slice(caret)}`;
      setInput(completed);
      closeMention();
      requestAnimationFrame(() => {
        el.focus();
        const pos = at + opt.server.length + opt.tool.length + 2;
        el.setSelectionRange(pos, pos);
      });
    },
    [input, closeMention]
  );

  // Resolve @server.tool mentions by actually calling the tool, and append
  // each result to the outgoing message. Only mentions matching a connected
  // server's real tool run — anything else stays plain text, never a call.
  // A trailing JSON object supplies arguments; without one the tool is
  // called with {} and a failure is reported inline rather than hidden.
  const resolveMentions = useCallback(
    async (content: string): Promise<string> => {
      const mentions = extractMentions(content);
      if (mentions.length === 0) return content;
      if (toolOptions.length === 0) await loadToolOptions();
      // Re-read: loadToolOptions sets state asynchronously.
      let known = toolOptions;
      if (known.length === 0) {
        try {
          const servers = (await api.mcp.list()) as { id: string; name: string }[];
          const all: McpToolOption[] = [];
          for (const s of Array.isArray(servers) ? servers : []) {
            try {
              const tools = (await api.mcp.tools(s.id)) as { name: string }[];
              for (const t of Array.isArray(tools) ? tools : []) {
                all.push({ server: s.name, serverId: s.id, tool: t.name });
              }
            } catch {
              /* per-server best effort, as above */
            }
          }
          known = all;
        } catch {
          return content;
        }
      }
      const blocks: string[] = [];
      for (const m of mentions) {
        const opt = known.find((t) => t.server === m.server && t.tool === m.tool);
        if (!opt) continue;
        try {
          const result = await api.mcp.callTool(opt.serverId, opt.tool, m.args ?? {});
          blocks.push(`[Tool @${m.server}.${m.tool} result: ${JSON.stringify(result).slice(0, 4000)}]`);
        } catch (e) {
          blocks.push(`[Tool @${m.server}.${m.tool} failed: ${e}]`);
        }
      }
      if (blocks.length === 0) return content;
      return `${content}\n\n${blocks.join("\n")}`;
    },
    [toolOptions, loadToolOptions]
  );

  const sessionMessages = useMemo(
    () => (selectedSessionId ? messages[selectedSessionId] || [] : []),
    [selectedSessionId, messages]
  );

  const refreshContextUsage = async (sessionId: string) => {
    try {
      setContextUsage(await api.session.contextUsage(sessionId));
    } catch {
      // Not fatal — the indicator just stays hidden if this fails.
    }
  };

  useEffect(() => {
    if (selectedSessionId) refreshContextUsage(selectedSessionId);
  }, [selectedSessionId, sessionMessages.length]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [sessionMessages]);

  useEffect(() => {
    // Subscribe to notifications
    const unsub = api.onNotification((notif) => {
      if (notif.method === "session.message.delta" && notif.params.session_id === selectedSessionId) {
        const { message_id, delta } = notif.params;
        const current = (messages[selectedSessionId || ""] || []).find((m) => m.id === message_id);
        if (current) {
          updateMessage(selectedSessionId!, message_id, { content: current.content + delta });
        } else {
          // Create streaming message if not exists
          addMessage(selectedSessionId!, {
            id: message_id,
            session_id: selectedSessionId!,
            role: "assistant",
            content: delta,
            tool_calls: [],
            created_at: new Date().toISOString(),
            is_streaming: true,
          });
        }
      } else if (notif.method === "session.message.new" && notif.params.session_id === selectedSessionId) {
        addMessage(selectedSessionId!, {
          id: `msg-${Date.now()}`,
          session_id: selectedSessionId!,
          role: "assistant",
          content: notif.params.content,
          tool_calls: [],
          created_at: new Date().toISOString(),
        });
      } else if (notif.method === "session.tool_call.request" && notif.params.session_id === selectedSessionId) {
        setPendingApprovals((prev) => [...prev, notif.params]);
      } else if (notif.method === "session.message.complete" && notif.params.session_id === selectedSessionId) {
        const { message_id, content } = notif.params;
        updateMessage(selectedSessionId!, message_id, { content, is_streaming: false });
      }
    });

    return () => unsub();
  }, [selectedSessionId, messages, addMessage, updateMessage]);

  const handleCompactCommand = async (sessionId: string) => {
    setInput("");
    setIsSending(true);
    try {
      const result = await api.session.contextCompact(sessionId);
      const summaryText = result.digest
        ? `Context compacted — older messages summarized to keep this Session within its context budget. Full history is still visible above; this summary is what future turns actually send to the model.`
        : `Nothing to compact yet — this Session's recent messages are already all that would be kept.`;
      addMessage(sessionId, {
        id: `compact-${Date.now()}`,
        session_id: sessionId,
        role: "system",
        content: summaryText,
        tool_calls: [],
        created_at: new Date().toISOString(),
      });
      await loadMessages(sessionId);
      await refreshContextUsage(sessionId);
    } catch (e) {
      console.error(e);
    } finally {
      setIsSending(false);
    }
  };

  const handleSend = async () => {
    if (!input.trim() || !selectedSessionId) return;
    const raw = input.trim();
    setInput("");
    setIsSending(true);
    closeMention();
    // @mentions are resolved before the turn starts so the model receives
    // tool outputs as message content, not as a promise the agent loop
    // cannot keep (it has no MCP tool to call itself).
    const content = await resolveMentions(raw).catch(() => raw);

    // `/compact` — the manual trigger for context compaction (review_prompt.md
    // §3.1); everything else still goes to the Session as a normal message.
    if (content === "/compact") {
      await handleCompactCommand(selectedSessionId);
      return;
    }

    try {
      const userMsg = {
        id: `tmp-${Date.now()}`,
        session_id: selectedSessionId,
        role: "user" as const,
        content,
        tool_calls: [],
        created_at: new Date().toISOString(),
      };
      addMessage(selectedSessionId, userMsg);

      await api.session.sendMessage(selectedSessionId, content);
      // Reload after a bit
      setTimeout(() => loadMessages(selectedSessionId), 500);
    } catch (e) {
      console.error(e);
      addMessage(selectedSessionId!, {
        id: `err-${Date.now()}`,
        session_id: selectedSessionId!,
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
    if (!selectedSessionId) return;
    try {
      await api.session.approveTool(selectedSessionId, toolCallId, approved);
      setPendingApprovals((prev) => prev.filter((p) => p.tool_call_id !== toolCallId));
    } catch (e) {
      console.error(e);
    }
  };

  if (!selectedSessionId) {
    return (
      <div className="flex-1 flex items-center justify-center p-8 text-center">
        <div className="max-w-md">
          <h2 className="text-lg font-semibold mb-2">No session selected</h2>
          <p className="text-sm text-muted-foreground mb-4">
            Select a session from the left rail or create a new one to start chatting with CID.
          </p>
          <div className="text-xs text-muted-foreground bg-card p-3 rounded border text-left">
            <div className="font-mono">Flow 1 – First Session (golden path):</div>
            <ol className="list-decimal ml-4 mt-2 space-y-1">
              <li>Connect a local git repo</li>
              <li>Click &quot;New Session&quot; → choose worktree/shared</li>
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
      <SessionModelBar sessionId={selectedSessionId} />
      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        <PlanCard sessionId={selectedSessionId} />
        <AgentsMdReviewCard sessionId={selectedSessionId} />
        <ReviewCard sessionId={selectedSessionId} />
        <CheckpointCard sessionId={selectedSessionId} refreshOn={sessionMessages.length} />

        {/* 051 Wave 5.2: a screen reader had no way to know a streaming
            response was arriving — polite so mid-stream deltas don't
            interrupt whatever the user is doing. */}
        <div aria-live="polite" role="log" className="space-y-4">
        {sessionMessages.map((msg) => (
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
              <MessageSegments role={msg.role} content={msg.content} />
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
      <div className="p-3 border-t bg-card/50 relative">
        {mentionOpen && mentionMatches.length > 0 && (
          <div className="absolute bottom-full left-3 right-3 mb-1 bg-popover border rounded-lg shadow-lg max-h-48 overflow-y-auto text-xs z-10">
            {mentionMatches.map((opt, i) => (
              <button
                key={`${opt.server}.${opt.tool}`}
                onClick={() => completeMention(opt)}
                onMouseEnter={() => setMentionIndex(i)}
                className={`w-full text-left px-3 py-1.5 flex gap-2 items-center ${i === mentionIndex ? "bg-accent" : ""}`}
              >
                <span className="font-mono font-medium">@{opt.server}.{opt.tool}</span>
                {opt.description && (
                  <span className="text-muted-foreground truncate">{opt.description}</span>
                )}
              </button>
            ))}
          </div>
        )}
        <div className="flex gap-2 items-end">
          <textarea
            ref={textareaRef}
            className="flex-1 bg-background border rounded-lg px-3 py-2 text-sm resize-none min-h-[44px] max-h-32 outline-none focus:ring-1 focus:ring-ring"
            placeholder="Type a task, question, or @mention an MCP tool..."
            value={input}
            onChange={(e) => {
              const v = e.target.value;
              setInput(v);
              const q = mentionQueryAt(v, e.target.selectionStart ?? v.length);
              if (q === null) {
                closeMention();
              } else {
                if (toolOptions.length === 0) loadToolOptions();
                setMentionQuery(q);
                setMentionIndex(0);
                setMentionOpen(true);
              }
            }}
            onKeyDown={(e) => {
              if (mentionOpen && mentionMatches.length > 0) {
                if (e.key === "ArrowDown") {
                  e.preventDefault();
                  setMentionIndex((i) => (i + 1) % mentionMatches.length);
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setMentionIndex((i) => (i - 1 + mentionMatches.length) % mentionMatches.length);
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  closeMention();
                  return;
                }
                if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
                  const opt = mentionMatches[mentionIndex] ?? mentionMatches[0];
                  if (opt) {
                    e.preventDefault();
                    completeMention(opt);
                    return;
                  }
                }
              }
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
