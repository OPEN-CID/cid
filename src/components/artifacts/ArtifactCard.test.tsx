import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { parseArtifacts, ArtifactCard, MessageSegments } from "./ArtifactCard";

describe("parseArtifacts", () => {
  it("leaves plain text without fences untouched", () => {
    expect(parseArtifacts("just a normal reply\nwith lines")).toEqual([
      { kind: "text", text: "just a normal reply\nwith lines" },
    ]);
  });

  it("turns a closed artifact fence into an artifact segment", () => {
    const segs = parseArtifacts(
      "Here you go:\n```artifact-html My Dashboard\n<h1>hi</h1>\n```\nDone."
    );
    expect(segs).toEqual([
      { kind: "text", text: "Here you go:" },
      { kind: "artifact", title: "My Dashboard", code: "<h1>hi</h1>" },
      { kind: "text", text: "Done." },
    ]);
  });

  it("defaults the title when the info line carries none", () => {
    const segs = parseArtifacts("```artifact-html\n<p>x</p>\n```");
    expect(segs).toEqual([{ kind: "artifact", title: "Artifact", code: "<p>x</p>" }]);
  });

  it("leaves an unclosed fence as text (still streaming, never a broken preview)", () => {
    const content = "Working on it:\n```artifact-html\n<h1>half";
    expect(parseArtifacts(content)).toEqual([{ kind: "text", text: content }]);
  });

  it("leaves an empty fence as text rather than rendering an empty card", () => {
    const content = "```artifact-html\n   \n```";
    expect(parseArtifacts(content)).toEqual([{ kind: "text", text: content }]);
  });

  it("does not treat ordinary code fences as artifacts", () => {
    const content = "```html\n<p>code sample</p>\n```";
    expect(parseArtifacts(content)).toEqual([{ kind: "text", text: content }]);
  });

  it("handles multiple artifacts in one message", () => {
    const segs = parseArtifacts(
      "```artifact-html One\n<a>1</a>\n```\nmiddle\n```artifact-html Two\n<b>2</b>\n```"
    );
    expect(segs.filter((s) => s.kind === "artifact")).toHaveLength(2);
  });
});

describe("ArtifactCard", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders the preview iframe sandboxed with no same-origin access", () => {
    render(<ArtifactCard title="Demo" code="<h1>hi</h1><script>alert(1)</script>" />);
    const frame = screen.getByTitle("Demo preview") as HTMLIFrameElement;
    // The security boundary, asserted structurally: scripts may run, but
    // the opaque origin grants no access to the app's DOM or storage.
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(frame.getAttribute("srcDoc")).toContain("<h1>hi</h1>");
  });

  it("switches between Preview and Code tabs", () => {
    render(<ArtifactCard title="Demo" code="<p>source</p>" />);
    expect(screen.queryByText("<p>source</p>")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: /Code/ }));
    expect(screen.getByText("<p>source</p>")).toBeInTheDocument();
  });

  it("mints the download blob on click, not during render", () => {
    const create = vi.fn(() => "blob:mock");
    const revoke = vi.fn();
    vi.stubGlobal("URL", { createObjectURL: create, revokeObjectURL: revoke });
    try {
      render(<ArtifactCard title="My App" code="<p>dl</p>" />);
      // Rendering alone must not touch the blob API (leak per artifact).
      expect(create).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: /Download My App/ }));
      expect(create).toHaveBeenCalledOnce();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("MessageSegments", () => {
  it("renders artifacts only in assistant messages, never in user messages", () => {
    const body = "```artifact-html Trap\n<script>alert(1)</script>\n```";
    const { rerender } = render(<MessageSegments role="user" content={body} />);
    expect(screen.queryByTitle("Trap preview")).not.toBeInTheDocument();

    rerender(<MessageSegments role="assistant" content={body} />);
    expect(screen.getByTitle("Trap preview")).toBeInTheDocument();
  });
});
