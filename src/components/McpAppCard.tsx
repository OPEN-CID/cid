import { useState, useCallback } from "react";
import { api } from "@/lib/api";
import {
  ExternalLink,
  RefreshCw,
  CheckCircle,
  XCircle,
  Loader2,
  ChevronDown,
  ChevronUp,
} from "lucide-react";

interface McpAppProps {
  serverId: string;
  serverName: string;
  content: McpAppContent;
}

interface McpAppContent {
  type: "html" | "markdown" | "form" | "choice_cards" | "dashboard";
  title?: string;
  body?: string;
  html?: string;
  fields?: McpFormField[];
  choices?: McpChoice[];
  dashboard_url?: string;
  refresh_interval_seconds?: number;
  submit_url?: string;
}

interface McpFormField {
  id: string;
  label: string;
  type: "text" | "number" | "select" | "checkbox" | "textarea";
  required?: boolean;
  placeholder?: string;
  options?: { value: string; label: string }[];
  default?: string;
}

interface McpChoice {
  id: string;
  label: string;
  description?: string;
  icon?: string;
  variant?: "default" | "primary" | "danger";
}

const APP_CONTENT_TYPES = ["html", "markdown", "form", "choice_cards", "dashboard"];

/**
 * Pull an MCP Apps payload out of a tool result, if the server sent one.
 *
 * Per the 2026-07-28 MCP Apps extension a server returns renderable UI
 * alongside its text result. Servers place it under different keys in practice,
 * so the known locations are checked and anything unrecognised falls through to
 * the plain result card rather than being rendered as a broken app.
 */
export function extractMcpAppContent(result: unknown): McpAppContent | null {
  if (!result || typeof result !== "object") return null;
  const obj = result as Record<string, unknown>;
  const meta = obj._meta as Record<string, unknown> | undefined;
  const candidates: unknown[] = [meta?.["mcp/app"], meta?.mcp_app, obj.mcp_app, obj.app, obj];
  for (const candidate of candidates) {
    if (candidate && typeof candidate === "object") {
      const c = candidate as Record<string, unknown>;
      if (typeof c.type === "string" && APP_CONTENT_TYPES.includes(c.type)) {
        return candidate as McpAppContent;
      }
    }
  }
  return null;
}

/** Inline MCP App card rendered in the chat thread */
export function McpAppCard({ serverId, serverName, content }: McpAppProps) {
  const [expanded, setExpanded] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [formValues, setFormValues] = useState<Record<string, string>>({});
  const [formSubmitted, setFormSubmitted] = useState(false);
  const [dashboardHtml, setDashboardHtml] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const handleFormSubmit = useCallback(
    async (e: React.FormEvent) => {
      e.preventDefault();
      setSubmitting(true);
      try {
        if (content.submit_url) {
          await api.mcp.callTool(serverId, content.submit_url, formValues);
        }
        setFormSubmitted(true);
      } catch (err) {
        console.error("MCP form submission failed:", err);
      } finally {
        setSubmitting(false);
      }
    },
    [serverId, content.submit_url, formValues]
  );

  const handleChoice = useCallback(
    async (choiceId: string) => {
      setSubmitting(true);
      try {
        await api.mcp.callTool(serverId, `${content.title || "action"}_select`, {
          choice_id: choiceId,
        });
        setFormSubmitted(true);
      } catch (err) {
        console.error("MCP choice selection failed:", err);
      } finally {
        setSubmitting(false);
      }
    },
    [serverId, content.title]
  );

  const loadDashboard = useCallback(async () => {
    if (!content.dashboard_url) return;
    setRefreshing(true);
    try {
      const result = await api.mcp.callTool(serverId, content.dashboard_url, {});
      if (typeof result === "string") {
        setDashboardHtml(result);
      } else if (result?.html) {
        setDashboardHtml(result.html);
      }
    } catch (err) {
      console.error("Dashboard load failed:", err);
    } finally {
      setRefreshing(false);
    }
  }, [serverId, content.dashboard_url]);

  // Auto-refresh dashboard
  useState(() => {
    if (content.type === "dashboard" && content.refresh_interval_seconds) {
      loadDashboard();
      const interval = setInterval(
        loadDashboard,
        (content.refresh_interval_seconds || 30) * 1000
      );
      return () => clearInterval(interval);
    }
  });

  const variantStyles = {
    default: "border hover:bg-accent/50",
    primary: "border-primary/30 bg-primary/5 hover:bg-primary/10 text-primary-foreground",
    danger: "border-red-500/30 bg-red-500/5 hover:bg-red-500/10 text-red-200",
  };

  return (
    <div className="my-3 border rounded-lg bg-card overflow-hidden">
      {/* Header */}
      <div
        className="flex items-center gap-2 px-3 py-2 bg-muted/30 cursor-pointer"
        onClick={() => setExpanded(!expanded)}
      >
        <ExternalLink className="w-3.5 h-3.5 text-muted-foreground" />
        <span className="text-xs font-medium text-muted-foreground">
          MCP App: {serverName}
        </span>
        {content.title && (
          <>
            <span className="text-muted-foreground">/</span>
            <span className="text-xs font-medium">{content.title}</span>
          </>
        )}
        <span className="text-[10px] px-1.5 py-0.5 rounded bg-muted text-muted-foreground ml-auto mr-2">
          {content.type.replace("_", " ")}
        </span>
        {expanded ? (
          <ChevronUp className="w-3.5 h-3.5 text-muted-foreground" />
        ) : (
          <ChevronDown className="w-3.5 h-3.5 text-muted-foreground" />
        )}
      </div>

      {expanded && (
        <div className="p-3">
          {/* HTML/Markdown Content */}
          {(content.type === "html" || content.type === "markdown") && (
            <div
              className="prose prose-sm max-w-none dark:prose-invert"
              dangerouslySetInnerHTML={{
                __html: content.html || content.body || "",
              }}
            />
          )}

          {/* Form */}
          {content.type === "form" && content.fields && !formSubmitted && (
            <form onSubmit={handleFormSubmit} className="space-y-3">
              {content.fields.map((field) => (
                <div key={field.id}>
                  <label className="text-xs font-medium mb-1 block">
                    {field.label}
                    {field.required && <span className="text-red-400 ml-0.5">*</span>}
                  </label>
                  {field.type === "textarea" ? (
                    <textarea
                      className="w-full bg-background border rounded px-2.5 py-1.5 text-sm min-h-[60px]"
                      placeholder={field.placeholder}
                      required={field.required}
                      value={formValues[field.id] || ""}
                      onChange={(e) =>
                        setFormValues((v) => ({ ...v, [field.id]: e.target.value }))
                      }
                    />
                  ) : field.type === "select" ? (
                    <select
                      className="w-full bg-background border rounded px-2.5 py-1.5 text-sm"
                      required={field.required}
                      value={formValues[field.id] || field.default || ""}
                      onChange={(e) =>
                        setFormValues((v) => ({ ...v, [field.id]: e.target.value }))
                      }
                    >
                      <option value="">Select...</option>
                      {field.options?.map((opt) => (
                        <option key={opt.value} value={opt.value}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                  ) : field.type === "checkbox" ? (
                    <label className="flex items-center gap-2 text-sm cursor-pointer">
                      <input
                        type="checkbox"
                        className="rounded"
                        checked={formValues[field.id] === "true"}
                        onChange={(e) =>
                          setFormValues((v) => ({
                            ...v,
                            [field.id]: e.target.checked ? "true" : "false",
                          }))
                        }
                      />
                      {field.label}
                    </label>
                  ) : (
                    <input
                      type={field.type}
                      className="w-full bg-background border rounded px-2.5 py-1.5 text-sm"
                      placeholder={field.placeholder}
                      required={field.required}
                      value={formValues[field.id] || ""}
                      onChange={(e) =>
                        setFormValues((v) => ({ ...v, [field.id]: e.target.value }))
                      }
                    />
                  )}
                </div>
              ))}
              <button
                type="submit"
                disabled={submitting}
                className="flex items-center gap-2 px-4 py-2 text-sm bg-primary text-primary-foreground rounded disabled:opacity-50"
              >
                {submitting ? (
                  <>
                    <Loader2 className="w-3.5 h-3.5 animate-spin" /> Submitting...
                  </>
                ) : (
                  "Submit"
                )}
              </button>
            </form>
          )}

          {/* Form submitted state */}
          {content.type === "form" && formSubmitted && (
            <div className="flex items-center gap-2 text-sm text-green-400">
              <CheckCircle className="w-4 h-4" />
              Form submitted successfully
            </div>
          )}

          {/* Choice Cards */}
          {content.type === "choice_cards" && content.choices && !formSubmitted && (
            <div className="space-y-2">
              {content.choices.map((choice) => (
                <button
                  key={choice.id}
                  onClick={() => handleChoice(choice.id)}
                  disabled={submitting}
                  className={`w-full text-left p-3 rounded border text-sm transition-colors disabled:opacity-50 ${
                    variantStyles[choice.variant || "default"]
                  }`}
                >
                  <div className="font-medium">{choice.label}</div>
                  {choice.description && (
                    <div className="text-xs text-muted-foreground mt-0.5">
                      {choice.description}
                    </div>
                  )}
                </button>
              ))}
              {submitting && (
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <Loader2 className="w-3.5 h-3.5 animate-spin" /> Processing choice...
                </div>
              )}
            </div>
          )}

          {/* Choice submitted state */}
          {content.type === "choice_cards" && formSubmitted && (
            <div className="flex items-center gap-2 text-sm text-green-400">
              <CheckCircle className="w-4 h-4" />
              Choice submitted
            </div>
          )}

          {/* Dashboard */}
          {content.type === "dashboard" && (
            <div>
              {dashboardHtml ? (
                <div
                  className="prose prose-sm max-w-none dark:prose-invert"
                  dangerouslySetInnerHTML={{ __html: dashboardHtml }}
                />
              ) : (
                <div className="flex items-center justify-center py-8 text-muted-foreground">
                  <Loader2 className="w-5 h-5 animate-spin" />
                </div>
              )}
              <div className="flex items-center gap-2 mt-3 pt-2 border-t">
                <button
                  onClick={loadDashboard}
                  disabled={refreshing}
                  className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
                >
                  <RefreshCw
                    className={`w-3 h-3 ${refreshing ? "animate-spin" : ""}`}
                  />
                  Refresh
                </button>
                <span className="text-xs text-muted-foreground">
                  {content.refresh_interval_seconds
                    ? `Auto-refresh: ${content.refresh_interval_seconds}s`
                    : ""}
                </span>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Simplified inline MCP tool result renderer for non-App content */
export function McpToolResultCard({
  toolName,
  result,
  isError,
}: {
  toolName: string;
  result: unknown;
  isError?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const resultStr =
    typeof result === "string" ? result : JSON.stringify(result, null, 2);

  return (
    <div
      className={`my-2 border rounded-lg overflow-hidden text-sm ${
        isError
          ? "border-red-500/30 bg-red-500/5"
          : "border-green-500/30 bg-green-500/5"
      }`}
    >
      <div
        className="flex items-center gap-2 px-3 py-1.5 cursor-pointer hover:bg-muted/30 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        {isError ? (
          <XCircle className="w-3.5 h-3.5 text-red-400" />
        ) : (
          <CheckCircle className="w-3.5 h-3.5 text-green-400" />
        )}
        <span className="text-xs font-medium text-muted-foreground">{toolName}</span>
        <span className="text-[10px] text-muted-foreground ml-auto">
          {expanded ? "collapse" : "expand"}
        </span>
      </div>
      {expanded && (
        <div className="px-3 py-2 border-t bg-background/50">
          <pre className="text-xs whitespace-pre-wrap break-all font-mono max-h-60 overflow-y-auto">
            {resultStr}
          </pre>
        </div>
      )}
    </div>
  );
}