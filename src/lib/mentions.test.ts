import { describe, it, expect } from "vitest";
import { extractMentions, mentionQueryAt } from "./mentions";

describe("extractMentions", () => {
  it("finds a bare @server.tool mention", () => {
    expect(extractMentions("ask @jira.get_issue please")).toEqual([
      { raw: "@jira.get_issue", server: "jira", tool: "get_issue", args: undefined },
    ]);
  });

  it("parses trailing JSON arguments", () => {
    expect(extractMentions('@jira.get_issue {"key": "ENG-1"} and go')).toEqual([
      {
        raw: '@jira.get_issue {"key": "ENG-1"}',
        server: "jira",
        tool: "get_issue",
        args: { key: "ENG-1" },
      },
    ]);
  });

  it("leaves invalid JSON as an arg-less mention rather than failing", () => {
    expect(extractMentions("@jira.get_issue {oops")).toEqual([
      { raw: "@jira.get_issue", server: "jira", tool: "get_issue", args: undefined },
    ]);
  });

  it("ignores email addresses and lone @ signs", () => {
    expect(extractMentions("mail me@example.com @ thanks")).toEqual([]);
    expect(extractMentions("deploy @ production")).toEqual([]);
    expect(extractMentions("see a@b.c for details")).toEqual([]);
  });
});

describe("mentionQueryAt", () => {
  it("returns the fragment after @", () => {
    expect(mentionQueryAt("ask @jir", 8)).toBe("jir");
    expect(mentionQueryAt("ask @", 5)).toBe("");
  });

  it("returns null outside an @-token", () => {
    expect(mentionQueryAt("hello", 5)).toBeNull();
    expect(mentionQueryAt("ask @jira now", 12)).toBeNull();
    expect(mentionQueryAt("a@b@c", 5)).toBeNull();
    expect(mentionQueryAt("mail me@example.com", 17)).toBeNull();
  });
});
