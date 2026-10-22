export type Mention = {
  /** Full matched text, e.g. `@jira.get_issue {"key": "ENG-1"}`. */
  raw: string;
  server: string;
  tool: string;
  /** Parsed trailing JSON argument, or undefined when absent/invalid. */
  args: unknown;
};

/**
 * Find `@server.tool` mentions in composer text, with an optional trailing
 * JSON object as the tool's arguments. The JSON must start on the same line
 * as the mention; anything after it stays ordinary text.
 *
 * Deliberately strict about the mention shape (`@` + word + `.` + word), and
 * the `@` must not sit inside another word — so email addresses and
 * decorators never trigger a tool call. Resolution additionally only fires
 * for (server, tool) pairs the connected MCP servers actually offer;
 * anything else stays plain text.
 */
export function extractMentions(text: string): Mention[] {
  const out: Mention[] = [];
  const re = /(?<![A-Za-z0-9_@.-])@([A-Za-z0-9_-]+)\.([A-Za-z0-9_-]+)([ \t]*(\{[^\n]{0,2000}\}))?/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    const [, server, tool, , jsonText] = m;
    let args: unknown;
    if (jsonText) {
      try {
        args = JSON.parse(jsonText);
      } catch {
        args = undefined;
      }
    }
    out.push({ raw: m[0], server, tool, args });
  }
  return out;
}

/**
 * The live `@`-completion query: the word fragment after the last `@`
 * before the caret, or null when the caret is not inside an @-token.
 */
export function mentionQueryAt(text: string, caret: number): string | null {
  const before = text.slice(0, caret);
  const at = before.lastIndexOf("@");
  if (at < 0) return null;
  // The @ must start the text or follow whitespace — never continue a word
  // (email addresses) or another @.
  if (at > 0 && !/\s/.test(before[at - 1])) return null;
  const fragment = before.slice(at + 1);
  // A space, newline, or second @ ends the token.
  if (/[\s@]/.test(fragment)) return null;
  // Must look like server[.tool] so far — a bare "@" still opens the list.
  if (fragment !== "" && !/^[A-Za-z0-9_.-]+$/.test(fragment)) return null;
  return fragment;
}
