import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { RepoBrowserDialog } from "./RepoBrowserDialog";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    fs: { listDirs: vi.fn() },
    repo: { connect: vi.fn() },
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

const roots = {
  path: "C:\\",
  parent: null,
  entries: [
    { name: "Projects", path: "C:\\Projects", is_git_repo: false },
    { name: "cid", path: "C:\\Projects\\cid", is_git_repo: true },
  ],
};

describe("RepoBrowserDialog", () => {
  beforeEach(() => {
    vi.mocked(api.fs.listDirs).mockReset();
    vi.mocked(api.repo.connect).mockReset();
  });

  it("loads the filesystem roots on mount and lists directory entries", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    expect(api.fs.listDirs).toHaveBeenCalledWith(null);
    expect(screen.getByText("Projects")).toBeInTheDocument();
    expect(screen.getByText("cid")).toBeInTheDocument();
  });

  it("marks a git repository entry with a badge", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    expect(screen.getByTitle("Git repository")).toBeInTheDocument();
  });

  it("does not show a '..' row at the filesystem root (parent is null)", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    expect(screen.queryByText("..")).not.toBeInTheDocument();
  });

  it("clicking a folder navigates into it and clicking '..' goes back up", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    vi.mocked(api.fs.listDirs).mockResolvedValueOnce({
      path: "C:\\Projects",
      parent: "C:\\",
      entries: [{ name: "cid", path: "C:\\Projects\\cid", is_git_repo: true }],
    });
    fireEvent.click(screen.getByText("Projects"));
    await flushAsyncWork();

    expect(api.fs.listDirs).toHaveBeenCalledWith("C:\\Projects");
    expect(screen.getByText("..")).toBeInTheDocument();

    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    fireEvent.click(screen.getByText(".."));
    await flushAsyncWork();

    expect(api.fs.listDirs).toHaveBeenCalledWith("C:\\");
  });

  it("Connect calls repo.connect with the current path and reports success", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    vi.mocked(api.repo.connect).mockResolvedValueOnce({ id: "repo-1", name: "cid" });
    const onConnected = vi.fn();
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={onConnected} />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Connect"));
    await flushAsyncWork();

    expect(api.repo.connect).toHaveBeenCalledWith("C:\\");
    expect(onConnected).toHaveBeenCalledWith({ id: "repo-1", name: "cid" });
  });

  it("Cancel closes the dialog without connecting", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    const onClose = vi.fn();
    render(<RepoBrowserDialog onClose={onClose} onConnected={vi.fn()} />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    expect(api.repo.connect).not.toHaveBeenCalled();
  });

  it("shows an error message when listing fails, and does not crash", async () => {
    vi.mocked(api.fs.listDirs).mockRejectedValueOnce(new Error("permission denied"));
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    expect(await screen.findByText(/permission denied/)).toBeInTheDocument();
  });

  it("reports a connect failure via toast instead of throwing", async () => {
    vi.mocked(api.fs.listDirs).mockResolvedValueOnce(roots);
    vi.mocked(api.repo.connect).mockRejectedValueOnce(new Error("not a git repo"));
    const { toast } = await import("@/lib/dialog");
    render(<RepoBrowserDialog onClose={vi.fn()} onConnected={vi.fn()} />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Connect"));
    await waitFor(() => expect(toast.error).toHaveBeenCalled());
  });
});
