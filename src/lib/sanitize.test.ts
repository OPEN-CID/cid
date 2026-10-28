import { describe, it, expect } from "vitest";
import { sanitizeInlineHtml } from "./sanitize";

describe("sanitizeInlineHtml", () => {
  it("strips script elements entirely", () => {
    const out = sanitizeInlineHtml(`<p>hi</p><script>alert(1)</script>`);
    expect(out).not.toContain("<script");
    expect(out).toContain("hi");
  });

  it("strips event-handler attributes but keeps the element", () => {
    const out = sanitizeInlineHtml(`<img src="x.png" onerror="alert(1)" /><b>bold</b>`);
    expect(out).not.toContain("onerror");
    expect(out).toContain("<b>bold</b>");
  });

  it("removes javascript: URLs", () => {
    const out = sanitizeInlineHtml(`<a href="javascript:alert(1)">click</a>`);
    expect(out).not.toContain("javascript:");
  });

  it("forbids raw form elements (forms render as React controls instead)", () => {
    const out = sanitizeInlineHtml(
      `<form action="https://evil.example/collect"><input name="secret" /><button>go</button></form>`
    );
    expect(out).not.toContain("<form");
    expect(out).not.toContain("<input");
  });

  it("forces links outward with no opener access", () => {
    const out = sanitizeInlineHtml(`<a href="https://example.com">docs</a>`);
    expect(out).toContain('target="_blank"');
    expect(out).toContain("noopener");
  });

  it("keeps benign formatting markup intact", () => {
    const out = sanitizeInlineHtml(`<h1>Title</h1><p>Some <code>code</code> text.</p>`);
    expect(out).toContain("<h1>Title</h1>");
    expect(out).toContain("<code>code</code>");
  });
});
