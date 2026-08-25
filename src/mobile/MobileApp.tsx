import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import {
  ArrowLeft,
  Check,
  X,
  Send,
  RefreshCw,
  Mic,
  MicOff,
  Terminal as TerminalIcon,
  GitBranch,
  Bell,
} from "lucide-react";

/**
 * Mobile companion shell — approval and monitoring only.
 *
 * Part 1's mobile non-goal and Part 19's mobile screen spec: Session list →
 * Session thread with pending approval cards → approve/deny/comment →
 * read-only terminal and diff. Deliberately no file tree and no editor; code is
 * not written from here.
 *
 * Built on the same React bundle and the same JSON-RPC contract as the desktop
 * and web shells, per the Phase 2 bake-off ADR (0010).
 */

type Session = {
  id: string;
  title: string;
  status: string;
  autonomy_level: string;
  isolation_mode: string;
  worktree_path?: string | null;
  repo_channel_id: string;
  updated_at: string;
};

type Message = {
  id: string;
  role: string;
  content: string;
  created_at: string;
};

type PendingApproval = {
  tool_call_id: string;
  tool_name: string;
  arguments: unknown;
};

const STATUS_COLOR: Record<string, string> = {
  running: "bg-blue-500",
  blocked_on_approval: "bg-orange-500",
  review: "bg-purple-500",
  done: "bg-green-500",
  failed: "bg-red-500",
  closed: "bg-muted-foreground/40",
  created: "bg-muted-foreground/40",
  planning: "bg-yellow-500",
};

function statusDot(status: string) {
  return STATUS_COLOR[status] ?? "bg-muted-foreground/40";
}

/** Ask for notification permission once, then post one per pending approval. */
function useApprovalNotifications(pending: PendingApproval[], sessionTitle: string) {
  const [enabled, setEnabled] = useState(
    typeof Notification !== "undefined" && Notification.permission === "granted"
  );
  const [seen, setSeen] = useState<Set<string>>(new Set());

  const request = useCallback(async () => {
    if (typeof Notification === "undefined") return;
    const result = await Notification.requestPermission();
    setEnabled(result === "granted");
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const fresh = pending.filter((p) => !seen.has(p.tool_call_id));
    if (fresh.length === 0) return;
    for (const p of fresh) {
      try {
        new Notification("CID needs your approval", {
          body: `${sessionTitle}: ${p.tool_name}`,
          tag: p.tool_call_id,
        });
      } catch {
        // Notification construction can throw on unsupported platforms; the
        // in-app card is still shown, so this is not worth surfacing.
      }
    }
    setSeen((prev) => new Set([...prev, ...fresh.map((p) => p.tool_call_id)]));
  }, [enabled, pending, sessionTitle, seen]);

  return { enabled, request };
}

// The Web Speech API isn't in TS's standard DOM lib — this is the minimal
// shape this file actually uses, not the full spec.
type SpeechRecognitionResultLike = { transcript: string };
interface SpeechRecognitionLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((event: { results: SpeechRecognitionResultLike[][] }) => void) | null;
  onend: (() => void) | null;
  onerror: (() => void) | null;
  start(): void;
  stop(): void;
}
type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

/**
 * Voice input via the browser's speech recognition, where it exists.
 * Returns `supported: false` on platforms without it rather than showing a
 * button that does nothing.
 */
function useVoiceInput(onText: (text: string) => void) {
  const Recognition = useMemo(() => {
    const w = window as unknown as {
      SpeechRecognition?: SpeechRecognitionCtor;
      webkitSpeechRecognition?: SpeechRecognitionCtor;
    };
    return w.SpeechRecognition || w.webkitSpeechRecognition || null;
  }, []);
  const [listening, setListening] = useState(false);
  const [recognition, setRecognition] = useState<SpeechRecognitionLike | null>(null);

  const toggle = useCallback(() => {
    if (!Recognition) return;
    if (listening && recognition) {
      recognition.stop();
      setListening(false);
      return;
    }
    const r = new Recognition();
    r.continuous = false;
    r.interimResults = false;
    r.lang = navigator.language || "en-US";
    r.onresult = (event) => {
      const text = event.results?.[0]?.[0]?.transcript;
      if (text) onText(text);
    };
    r.onend = () => setListening(false);
    r.onerror = () => setListening(false);
    r.start();
    setRecognition(r);
    setListening(true);
  }, [Recognition, listening, recognition, onText]);

  return { supported: !!Recognition, listening, toggle };
}

function SessionList({ onOpen }: { onOpen: (m: Session) => void }) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setSessions((await api.session.list()) ?? []);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
    const unsub = api.onNotification((n) => {
      if (n.method?.startsWith("session.")) load();
    });
    return () => unsub();
  }, [load]);

  // Sessions waiting on a human come first — that is the whole reason to open
  // this app on a phone.
  const sorted = useMemo(
    () =>
      [...sessions].sort((a, b) => {
        const aBlocked = a.status === "blocked_on_approval" ? 0 : 1;
        const bBlocked = b.status === "blocked_on_approval" ? 0 : 1;
        return aBlocked - bBlocked || b.updated_at.localeCompare(a.updated_at);
      }),
    [sessions]
  );

  return (
    <div className="flex flex-col h-full">
      <header className="h-14 border-b flex items-center px-4 gap-2 bg-card shrink-0">
        <span className="font-semibold">Sessions</span>
        <button onClick={load} className="ml-auto p-2 -mr-2" aria-label="Refresh">
          <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
        </button>
      </header>

      <div className="flex-1 overflow-y-auto">
        {error && <div className="m-4 p-3 rounded border text-sm text-red-300">{error}</div>}
        {!error && sorted.length === 0 && !loading && (
          <div className="p-8 text-center text-sm text-muted-foreground">
            No sessions yet. Start one from the desktop or web app.
          </div>
        )}
        {sorted.map((m) => (
          <button
            key={m.id}
            onClick={() => onOpen(m)}
            className="w-full text-left px-4 py-3 border-b active:bg-accent/50"
          >
            <div className="flex items-center gap-2">
              <span className={`w-2 h-2 rounded-full shrink-0 ${statusDot(m.status)}`} />
              <span className="font-medium text-sm truncate">{m.title}</span>
            </div>
            <div className="mt-1 flex gap-2 text-[11px] text-muted-foreground">
              <span>{m.status.replace(/_/g, " ")}</span>
              <span>·</span>
              <span>{m.autonomy_level}</span>
              {m.status === "blocked_on_approval" && (
                <span className="ml-auto text-orange-400 font-medium">needs you</span>
              )}
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

function SessionDetail({ session, onBack }: { session: Session; onBack: () => void }) {
  const [tab, setTab] = useState<"thread" | "diff" | "terminal">("thread");
  const [messages, setMessages] = useState<Message[]>([]);
  const [pending, setPending] = useState<PendingApproval[]>([]);
  const [reply, setReply] = useState("");
  const [busy, setBusy] = useState(false);
  const [diff, setDiff] = useState<string>("");
  const [terminal, setTerminal] = useState<string[]>([]);

  const notifications = useApprovalNotifications(pending, session.title);
  const voice = useVoiceInput((text) => setReply((r) => (r ? `${r} ${text}` : text)));

  const loadMessages = useCallback(async () => {
    try {
      setMessages((await api.message.list(session.id)) ?? []);
    } catch {
      // The thread simply stays as-is if Core is briefly unreachable.
    }
  }, [session.id]);

  useEffect(() => {
    loadMessages();
    const unsub = api.onNotification((n) => {
      if (n.params?.session_id !== session.id) return;
      if (n.method === "session.tool_call.request") {
        setPending((prev) => [...prev, n.params as PendingApproval]);
      } else if (n.method === "session.tool_call.complete") {
        setPending((prev) => prev.filter((p) => p.tool_call_id !== n.params.tool_call_id));
        loadMessages();
      } else if (n.method?.startsWith("session.message")) {
        loadMessages();
      } else if (n.method === "pty.output") {
        setTerminal((prev) => [...prev.slice(-400), String(n.params?.data ?? "")]);
      }
    });
    return () => unsub();
  }, [session.id, loadMessages]);

  useEffect(() => {
    if (tab !== "diff") return;
    const path = session.worktree_path;
    if (!path) return;
    api.git
      .diff(path)
      .then((d) => setDiff(JSON.stringify(d, null, 2)))
      .catch((e) => setDiff(String(e)));
  }, [tab, session.worktree_path]);

  const decide = async (toolCallId: string, approved: boolean) => {
    setBusy(true);
    try {
      await api.session.approveTool(session.id, toolCallId, approved);
      setPending((prev) => prev.filter((p) => p.tool_call_id !== toolCallId));
    } finally {
      setBusy(false);
    }
  };

  const send = async () => {
    if (!reply.trim()) return;
    const content = reply.trim();
    setReply("");
    setBusy(true);
    try {
      await api.session.sendMessage(session.id, content);
      await loadMessages();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      <header className="h-14 border-b flex items-center px-2 gap-2 bg-card shrink-0">
        <button onClick={onBack} className="p-2" aria-label="Back">
          <ArrowLeft className="w-5 h-5" />
        </button>
        <div className="min-w-0">
          <div className="font-medium text-sm truncate">{session.title}</div>
          <div className="text-[11px] text-muted-foreground">
            {session.status.replace(/_/g, " ")} · {session.autonomy_level}
          </div>
        </div>
        {!notifications.enabled && (
          <button
            onClick={notifications.request}
            className="ml-auto p-2 text-muted-foreground"
            aria-label="Enable notifications"
          >
            <Bell className="w-4 h-4" />
          </button>
        )}
      </header>

      <nav className="flex border-b shrink-0">
        {(["thread", "diff", "terminal"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`flex-1 py-2.5 text-xs capitalize ${
              tab === t ? "border-b-2 border-primary font-medium" : "text-muted-foreground"
            }`}
          >
            {t === "diff" && <GitBranch className="w-3 h-3 inline mr-1" />}
            {t === "terminal" && <TerminalIcon className="w-3 h-3 inline mr-1" />}
            {t}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {tab === "thread" && (
          <>
            {pending.map((p) => (
              <div
                key={p.tool_call_id}
                className="border border-orange-500/50 bg-orange-500/10 rounded-lg p-3"
              >
                <div className="text-sm font-semibold flex items-center gap-2">
                  <span className="w-2 h-2 bg-orange-500 rounded-full animate-pulse" />
                  Approval needed: {p.tool_name}
                </div>
                <pre className="text-[11px] bg-background mt-2 p-2 rounded overflow-x-auto max-h-40">
                  {JSON.stringify(p.arguments, null, 2)}
                </pre>
                <div className="flex gap-2 mt-2">
                  <button
                    onClick={() => decide(p.tool_call_id, true)}
                    disabled={busy}
                    className="flex-1 flex items-center justify-center gap-1 bg-green-600 text-white text-sm py-2 rounded disabled:opacity-50"
                  >
                    <Check className="w-4 h-4" /> Approve
                  </button>
                  <button
                    onClick={() => decide(p.tool_call_id, false)}
                    disabled={busy}
                    className="flex-1 flex items-center justify-center gap-1 bg-red-600 text-white text-sm py-2 rounded disabled:opacity-50"
                  >
                    <X className="w-4 h-4" /> Deny
                  </button>
                </div>
              </div>
            ))}

            {messages.map((m) => (
              <div
                key={m.id}
                className={`rounded-lg px-3 py-2 text-sm ${
                  m.role === "user"
                    ? "bg-primary text-primary-foreground ml-8"
                    : m.role === "system"
                    ? "bg-destructive/20 border border-destructive/40"
                    : "bg-card border mr-4"
                }`}
              >
                <div className="whitespace-pre-wrap break-words">{m.content}</div>
              </div>
            ))}
          </>
        )}

        {tab === "diff" && (
          <pre className="text-[11px] font-mono whitespace-pre-wrap break-all">
            {session.worktree_path ? diff || "Loading diff…" : "This Session has no worktree."}
          </pre>
        )}

        {tab === "terminal" && (
          <>
            <div className="text-[11px] text-muted-foreground mb-2">
              Read-only. Commands are not run from mobile.
            </div>
            <pre className="text-[11px] font-mono whitespace-pre-wrap break-all bg-background rounded p-2">
              {terminal.length > 0 ? terminal.join("") : "No terminal output yet."}
            </pre>
          </>
        )}
      </div>

      {tab === "thread" && (
        <div className="border-t p-2 flex gap-2 items-end shrink-0 bg-card">
          <textarea
            className="flex-1 bg-background border rounded-lg px-3 py-2 text-sm resize-none max-h-24"
            placeholder="Comment…"
            rows={1}
            value={reply}
            onChange={(e) => setReply(e.target.value)}
          />
          {voice.supported && (
            <button
              onClick={voice.toggle}
              className={`p-2.5 rounded-lg ${voice.listening ? "bg-red-600 text-white" : "bg-secondary"}`}
              aria-label="Voice input"
            >
              {voice.listening ? <MicOff className="w-4 h-4" /> : <Mic className="w-4 h-4" />}
            </button>
          )}
          <button
            onClick={send}
            disabled={busy || !reply.trim()}
            className="bg-primary text-primary-foreground p-2.5 rounded-lg disabled:opacity-50"
            aria-label="Send"
          >
            <Send className="w-4 h-4" />
          </button>
        </div>
      )}
    </div>
  );
}

export function MobileApp() {
  const [selected, setSelected] = useState<Session | null>(null);
  const [connected, setConnected] = useState(false);

  useEffect(() => {
    api
      .connect()
      .then(() => setConnected(true))
      .catch(() => setConnected(false));
  }, []);

  return (
    <div className="h-screen flex flex-col bg-background text-foreground">
      {!connected && (
        <div className="bg-yellow-500/20 text-yellow-200 text-[11px] px-3 py-1.5 text-center shrink-0">
          Connecting to Core…
        </div>
      )}
      <div className="flex-1 min-h-0">
        {selected ? (
          <SessionDetail session={selected} onBack={() => setSelected(null)} />
        ) : (
          <SessionList onOpen={setSelected} />
        )}
      </div>
    </div>
  );
}
