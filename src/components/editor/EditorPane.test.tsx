import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { EditorPane } from "./EditorPane";
import { api } from "@/lib/api";
import { useCid } from "@/hooks/useCid";

// 050-Gold-Standard-Review.md F2 / 051 Wave 1.2 + Wave 4.2: EditorPane is now
// a tabbed editor. Opening a second file no longer risks losing edits in the
// first (it stays open in its own tab) — only *closing* a dirty tab prompts.

vi.mock("@monaco-editor/react", () => ({
  default: ({ value, onChange }: { value: string; onChange: (v: string) => void }) => (
    <textarea data-testid="monaco-stub" value={value} onChange={(e) => onChange(e.target.value)} />
  ),
}));

vi.mock("@/lib/api", () => ({
  api: {
    file: {
      read: vi.fn(),
      write: vi.fn(),
      list: vi.fn(),
    },
    contextEngine: {
      status: vi.fn(),
      search: vi.fn(),
    },
    code: {
      searchSymbols: vi.fn(),
      analyzeFile: vi.fn(),
    },
    semanticEngine: {
      status: vi.fn(),
      indexFile: vi.fn(),
    },
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: vi.fn(),
}));

vi.mock("@/theme/useTheme", () => ({
  useTheme: vi.fn((selector: (s: { theme: string }) => unknown) => selector({ theme: "dark" })),
}));

const repos = [{ id: "repo-1", path: "/repo" }];
const files = [
  { name: "a.txt", path: "/repo/a.txt", is_dir: false, is_file: true, size: 5 },
  { name: "b.txt", path: "/repo/b.txt", is_dir: false, is_file: true, size: 5 },
];

describe("EditorPane", () => {
  beforeEach(() => {
    vi.mocked(api.file.read).mockReset();
    vi.mocked(api.file.write).mockReset();
    vi.mocked(api.file.list).mockReset();
    vi.mocked(api.contextEngine.status).mockReset();
    vi.mocked(api.contextEngine.search).mockReset();
    vi.mocked(api.code.searchSymbols).mockReset();
    vi.mocked(api.code.analyzeFile).mockReset();
    vi.mocked(api.semanticEngine.status).mockReset();
    vi.mocked(api.semanticEngine.indexFile).mockReset();
    vi.mocked(api.file.list).mockResolvedValue(files);
    vi.mocked(api.contextEngine.status).mockResolvedValue({ enabled: false });
    vi.mocked(api.semanticEngine.status).mockResolvedValue({ enabled: false });
    vi.mocked(useCid).mockReturnValue({ selectedRepoId: "repo-1", repos } as any);
  });

  it("opens a file as a tab when clicked", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    await screen.findByDisplayValue("hello a");

    expect(screen.getAllByText(/a\.txt/).length).toBeGreaterThan(0);
  });

  it("opening a second file keeps the first open as its own tab", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    await screen.findByDisplayValue("hello a");
    fireEvent.change(screen.getByDisplayValue("hello a"), { target: { value: "edited a" } });

    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello b" });
    fireEvent.click(screen.getByText(/b\.txt/));
    await screen.findByDisplayValue("hello b");

    // No prompt — the edited a.txt tab is simply no longer active.
    expect(screen.queryByText(/Unsaved changes/)).not.toBeInTheDocument();

    // Switching back to a.txt shows the edit is still there, untouched.
    const tabs = screen.getAllByTitle("/repo/a.txt");
    fireEvent.click(tabs[0]);
    await screen.findByDisplayValue("edited a");
  });

  it("closing a clean tab removes it without prompting", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    await screen.findByDisplayValue("hello a");

    fireEvent.click(screen.getByLabelText("Close a.txt"));
    await waitFor(() => expect(screen.queryByTitle("/repo/a.txt")).not.toBeInTheDocument());
    expect(screen.queryByText(/Unsaved changes/)).not.toBeInTheDocument();
  });

  it("closing a dirty tab prompts instead of silently discarding it", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });

    fireEvent.click(screen.getByLabelText("Close a.txt"));

    expect(await screen.findByText(/Unsaved changes/)).toBeInTheDocument();
    // Tab must still be open — the close hasn't happened yet.
    expect(screen.getByTitle("/repo/a.txt")).toBeInTheDocument();
  });

  it("discarding the close prompt loses the edit and closes the tab", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });
    fireEvent.click(screen.getByLabelText("Close a.txt"));
    await screen.findByText(/Unsaved changes/);

    fireEvent.click(screen.getByText("Discard & close"));

    await waitFor(() => expect(screen.queryByTitle("/repo/a.txt")).not.toBeInTheDocument());
    expect(api.file.write).not.toHaveBeenCalled();
  });

  it("saving re-indexes the file in the Semantic Engine when it's enabled for the repo", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    vi.mocked(api.semanticEngine.status).mockResolvedValue({ enabled: true });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });

    vi.mocked(api.file.write).mockResolvedValueOnce({ ok: true });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });

    await waitFor(() => expect(api.file.write).toHaveBeenCalledWith("/repo/a.txt", "edited a"));
    await waitFor(() => expect(api.semanticEngine.indexFile).toHaveBeenCalledWith("/repo", "/repo/a.txt", "edited a"));
  });

  it("saving does not re-index when the Semantic Engine is off for the repo", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    vi.mocked(api.semanticEngine.status).mockResolvedValue({ enabled: false });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });

    vi.mocked(api.file.write).mockResolvedValueOnce({ ok: true });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });

    await waitFor(() => expect(api.file.write).toHaveBeenCalled());
    await waitFor(() => expect(api.semanticEngine.status).toHaveBeenCalled());
    expect(api.semanticEngine.indexFile).not.toHaveBeenCalled();
  });

  it("saving from the close prompt writes before closing", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });
    fireEvent.click(screen.getByLabelText("Close a.txt"));
    await screen.findByText(/Unsaved changes/);

    vi.mocked(api.file.write).mockResolvedValueOnce({ ok: true });
    fireEvent.click(screen.getByText("Save & close"));

    await waitFor(() => expect(api.file.write).toHaveBeenCalledWith("/repo/a.txt", "edited a"));
    await waitFor(() => expect(screen.queryByTitle("/repo/a.txt")).not.toBeInTheDocument());
  });

  it("Ctrl+S saves the active tab without a mouse click", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    const editor = await screen.findByDisplayValue("hello a");
    fireEvent.change(editor, { target: { value: "edited a" } });

    vi.mocked(api.file.write).mockResolvedValueOnce({ ok: true });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });

    await waitFor(() => expect(api.file.write).toHaveBeenCalledWith("/repo/a.txt", "edited a"));
  });

  it("switching the left panel to Search runs a symbol search when Context Engine is off", async () => {
    vi.mocked(api.contextEngine.status).mockResolvedValue({ enabled: false });
    vi.mocked(api.code.searchSymbols).mockResolvedValue({
      results: [{ file_path: "/repo/a.txt", symbol: { name: "helper", kind: "function", line: 3 } }],
    });
    render(<EditorPane />);

    fireEvent.click(screen.getByText("Search"));
    fireEvent.change(screen.getByPlaceholderText(/Search symbols/), { target: { value: "helper" } });

    await waitFor(() => expect(api.code.searchSymbols).toHaveBeenCalledWith("/repo", "helper"));
    expect(await screen.findByText("helper")).toBeInTheDocument();
  });

  it("switching the left panel to Search uses context_engine.search when enabled", async () => {
    vi.mocked(api.contextEngine.status).mockResolvedValue({ enabled: true });
    vi.mocked(api.contextEngine.search).mockResolvedValue({
      results: [{ file_path: "/repo/a.txt", name: "helper", kind: "function", line: 3 }],
    });
    render(<EditorPane />);

    fireEvent.click(screen.getByText("Search"));
    fireEvent.change(screen.getByPlaceholderText(/Search symbols/), { target: { value: "helper" } });

    await waitFor(() => expect(api.contextEngine.search).toHaveBeenCalledWith("/repo", "helper"));
    expect(await screen.findByText("helper")).toBeInTheDocument();
    expect(api.code.searchSymbols).not.toHaveBeenCalled();
  });

  it("clicking a search result opens that file as a tab", async () => {
    vi.mocked(api.contextEngine.status).mockResolvedValue({ enabled: false });
    vi.mocked(api.code.searchSymbols).mockResolvedValue({
      results: [{ file_path: "/repo/a.txt", symbol: { name: "helper", kind: "function", line: 3 } }],
    });
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    render(<EditorPane />);

    fireEvent.click(screen.getByText("Search"));
    fireEvent.change(screen.getByPlaceholderText(/Search symbols/), { target: { value: "helper" } });
    fireEvent.click(await screen.findByText("helper"));

    await screen.findByDisplayValue("hello a");
    expect(api.file.read).toHaveBeenCalledWith("/repo/a.txt");
  });

  it("toggling Outline fetches and lists the open file's symbols", async () => {
    vi.mocked(api.file.read).mockResolvedValueOnce({ content: "hello a" });
    vi.mocked(api.code.analyzeFile).mockResolvedValueOnce({
      symbols: [{ name: "run", kind: "function", line: 5 }],
    });
    render(<EditorPane />);

    fireEvent.click(await screen.findByText(/a\.txt/));
    await screen.findByDisplayValue("hello a");

    fireEvent.click(screen.getByLabelText("Toggle outline"));

    await waitFor(() => expect(api.code.analyzeFile).toHaveBeenCalledWith("/repo/a.txt"));
    expect(await screen.findByText("run")).toBeInTheDocument();
  });
});
