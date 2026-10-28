import { memo, useMemo, useState } from "react";
import { Code2, Eye, Copy, Download, Check, FileBox, FolderDown } from "lucide-react";
import { api } from "@/lib/api";
import { useSessionRepoPath } from "@/hooks/useSessionRepoPath";

export type MessageSegment =
  | { kind: "text"; text: string }
  | { kind: "artifact"; title: string; code: string };

const FENCE_OPEN = /^```\s*artifact-html\s*(.*)$/;
const FENCE_CLOSE = /^\s*```\s*$/;

/**
 * Split assistant message content into plain-text runs and runnable
 * artifacts. The agent emits an artifact as a fenced block:
 *
 *   ```artifact-html Optional title
 *   <html>…</html>
 *   ```
 *
 * Only *closed* fences become artifacts — while a response is still
 * streaming the fence is incomplete and stays plain text, so a half-written
 * app never renders as a broken preview. Unparseable or empty blocks are
 * likewise left as text rather than dropped: no content is ever lost.
 */
export function parseArtifacts(content: string): MessageSegment[] {
  const segments: MessageSegment[] = [];
  const lines = content.split("\n");
  let text: string[] = [];
  let i = 0;

  const flushText = () => {
    if (text.length > 0) {
      // Preserve the exact original text (minus the trailing newline the
      // split consumed) so non-artifact rendering is byte-identical.
      segments.push({ kind: "text", text: text.join("\n") });
      text = [];
    }
  };

  while (i < lines.length) {
    const open = lines[i].match(FENCE_OPEN);
    if (!open) {
      text.push(lines[i]);
      i++;
      continue;
    }
    // Look ahead for the closing fence before committing: an unclosed
    // fence (still streaming, or a typo) is not an artifact.
    let close = -1;
    for (let j = i + 1; j < lines.length; j++) {
      if (FENCE_CLOSE.test(lines[j])) {
        close = j;
        break;
      }
    }
    if (close < 0) {
      text.push(lines[i]);
      i++;
      continue;
    }
    const code = lines.slice(i + 1, close).join("\n");
    if (code.trim().length === 0) {
      text.push(lines[i]);
      i++;
      continue;
    }
    flushText();
    segments.push({
      kind: "artifact",
      title: open[1].trim() || "Artifact",
      code,
    });
    i = close + 1;
  }
  flushText();
  return segments;
}

/**
 * A runnable agent-built app inside the thread — the Claude-artifacts shape:
 * Preview / Code tabs, copy, and download as a standalone `.html` file.
 *
 * Security boundary: the preview is an `<iframe>` with `sandbox="allow-scripts"`
 * and **no** `allow-same-origin`, so the artifact runs in an opaque origin —
 * no access to the app's DOM, storage, or credentials. Scripts are allowed
 * (otherwise it wouldn't be an app); top navigation, popups, plugins and
 * same-origin access are not. Network calls from inside remain subject to
 * the page's CORS rules, same as any third-party iframe.
 */
export const ArtifactCard = memo(function ArtifactCard({
  title,
  code,
}: {
  title: string;
  code: string;
}) {
  const [tab, setTab] = useState<"preview" | "code">("preview");
  const [copied, setCopied] = useState(false);
  // Save-to-session state: filename row, two-step overwrite, result status.
  const [saveOpen, setSaveOpen] = useState(false);
  const [saveBusy, setSaveBusy] = useState(false);
  const [fileName, setFileName] = useState("");
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const [saveStatus, setSaveStatus] = useState<string | null>(null);
  const repoPath = useSessionRepoPath();
  const defaultFileName = useMemo(
    () => `${title.replace(/[^\w-]+/g, "_").slice(0, 60) || "artifact"}.html`,
    [title]
  );

  /**
   * Save the artifact as a real file in the Session's working tree, so it
   * enters the normal review pipeline: Diff → per-hunk review → checkpoint
   * → merge. The path is resolved through the session worktree (never the
   * main checkout when a worktree Session is selected) and the backend
   * confines the write to connected repos — `..` escapes and absolute
   * paths outside any repo are refused server-side. Overwriting an existing
   * file takes an explicit second click, never a blind write.
   */
  const saveToSession = async () => {
    const base = repoPath;
    if (!base) {
      setSaveStatus("No session repo available — select a Session first.");
      return;
    }
    const name = (fileName || defaultFileName).trim();
    if (!name) {
      setSaveStatus("Enter a file name first.");
      return;
    }
    const target = /^[a-zA-Z]:[\\/]|^\//.test(name) ? name : `${base.replace(/[\\/]+$/, "")}/${name}`;
    setSaveBusy(true);
    setSaveStatus(null);
    try {
      if (!confirmOverwrite) {
        // Readable means it exists: arm the second click instead of writing.
        // Unreadable means absent (or unreadable, in which case the write
        // below fails loudly with the real reason) — proceed to write.
        let exists = false;
        try {
          await api.file.read(target);
          exists = true;
        } catch {
          exists = false;
        }
        if (exists) {
          setConfirmOverwrite(true);
          setSaveStatus(`"${name}" already exists — Save again to overwrite it.`);
          return;
        }
      }
      await api.file.write(target, code);
      setConfirmOverwrite(false);
      setSaveStatus(`Saved to ${target} — see the Diff panel to review it.`);
    } catch (e) {
      setSaveStatus(`Save failed: ${e}`);
    } finally {
      setSaveBusy(false);
    }
  };

  // The download URL is minted on click, not during render: minting per
  // render would leak a blob URL for every artifact in a long thread (and
  // breaks under test DOMs without createObjectURL). Revoked after use.
  const download = () => {
    const blob = new Blob([code], { type: "text/html;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${title.replace(/[^\w-]+/g, "_").slice(0, 60) || "artifact"}.html`;
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(code);
    } catch {
      // Clipboard unavailable (permissions, insecure context) — the Code
      // tab still lets the user select the source manually.
      const ta = document.createElement("textarea");
      ta.value = code;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="mt-2 border rounded-lg overflow-hidden bg-background">
      <div className="flex items-center gap-2 px-3 py-1.5 bg-accent/50 border-b text-xs">
        <FileBox className="w-3.5 h-3.5 text-muted-foreground" />
        <span className="font-medium truncate flex-1">{title}</span>
        <div className="flex rounded overflow-hidden border" role="tablist" aria-label={`${title} view`}>
          <button
            role="tab"
            aria-selected={tab === "preview"}
            onClick={() => setTab("preview")}
            className={`flex items-center gap-1 px-2 py-0.5 ${tab === "preview" ? "bg-primary text-primary-foreground" : "text-muted-foreground"}`}
          >
            <Eye className="w-3 h-3" /> Preview
          </button>
          <button
            role="tab"
            aria-selected={tab === "code"}
            onClick={() => setTab("code")}
            className={`flex items-center gap-1 px-2 py-0.5 ${tab === "code" ? "bg-primary text-primary-foreground" : "text-muted-foreground"}`}
          >
            <Code2 className="w-3 h-3" /> Code
          </button>
        </div>
        <button
          onClick={copyCode}
          aria-label={`Copy ${title} source`}
          className="p-1 rounded text-muted-foreground hover:text-foreground"
          title="Copy source"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-green-500" /> : <Copy className="w-3.5 h-3.5" />}
        </button>
        <button
          onClick={download}
          aria-label={`Download ${title} as HTML`}
          className="p-1 rounded text-muted-foreground hover:text-foreground"
          title="Download as .html"
        >
          <Download className="w-3.5 h-3.5" />
        </button>
        <button
          onClick={() => {
            setSaveOpen((s) => !s);
            setSaveStatus(null);
            setConfirmOverwrite(false);
          }}
          aria-label={`Save ${title} to session files`}
          className="p-1 rounded text-muted-foreground hover:text-foreground"
          title="Save into the Session's working tree (reviewable in Diff)"
        >
          <FolderDown className="w-3.5 h-3.5" />
        </button>
      </div>
      {saveOpen && (
        <div className="px-3 py-2 border-b bg-accent/30 space-y-1.5 text-xs">
          <div className="flex gap-2 items-center">
            <input
              className="flex-1 bg-background border rounded px-2 py-1 text-[11px] font-mono"
              placeholder={defaultFileName}
              value={fileName}
              onChange={(e) => {
                setFileName(e.target.value);
                setConfirmOverwrite(false);
              }}
              aria-label="File name for saved artifact"
            />
            <button
              onClick={saveToSession}
              disabled={saveBusy}
              className="px-2 py-1 rounded bg-primary text-primary-foreground disabled:opacity-50"
            >
              {saveBusy ? "Saving…" : confirmOverwrite ? "Overwrite" : "Save"}
            </button>
          </div>
          <div className="text-[10px] text-muted-foreground">
            Saved into the Session&apos;s working tree (never the main checkout), as a real file you can diff,
            review, and merge. Escapes outside connected repos are refused.
          </div>
          {saveStatus && <div className="text-muted-foreground">{saveStatus}</div>}
        </div>
      )}
      {tab === "preview" ? (
        <iframe
          title={`${title} preview`}
          sandbox="allow-scripts"
          srcDoc={code}
          loading="lazy"
          className="w-full h-[420px] bg-white"
        />
      ) : (
        <pre className="p-3 text-[11px] font-mono overflow-auto max-h-[420px] whitespace-pre-wrap">{code}</pre>
      )}
    </div>
  );
});

/**
 * Assistant message body: plain-text runs render exactly as before;
 * closed artifact fences render as runnable cards. User messages bypass
 * parsing entirely — pasted HTML must never auto-execute, even sandboxed.
 */
export function MessageSegments({ role, content }: { role: string; content: string }) {
  const segments = useMemo(
    () => (role === "assistant" ? parseArtifacts(content) : [{ kind: "text" as const, text: content }]),
    [role, content]
  );
  return (
    <>
      {segments.map((seg, idx) =>
        seg.kind === "artifact" ? (
          <ArtifactCard key={idx} title={seg.title} code={seg.code} />
        ) : (
          <div key={idx} className="whitespace-pre-wrap">
            {seg.text}
          </div>
        )
      )}
    </>
  );
}
