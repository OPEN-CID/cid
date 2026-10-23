import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { LocalModelsPanel } from "./LocalModelsPanel";
import { api } from "@/lib/api";
import { confirmDialog } from "@/lib/dialog";

vi.mock("@/lib/api", () => ({
  api: {
    localRuntime: {
      recommended: vi.fn(),
      status: vi.fn(),
      start: vi.fn(),
      stop: vi.fn(),
      pull: vi.fn(),
      install: vi.fn(),
    },
    onNotification: vi.fn(() => () => {}),
  },
}));

vi.mock("@/lib/dialog", () => ({
  toast: { error: vi.fn(), success: vi.fn() },
  confirmDialog: vi.fn(),
}));

const system = {
  os: "windows",
  arch: "x86_64",
  cpu_cores: 16,
  total_ram_mb: 32768,
  available_ram_mb: 16384,
  gpus: [{ name: "RTX 4070", vram_mb: 12288 }],
  total_vram_mb: 12288,
};

const models = [
  {
    id: "qwen2.5-coder:7b",
    name: "Qwen2.5 Coder 7B",
    parameters: "7B",
    download_mb: 4700,
    min_memory_mb: 8192,
    context_tokens: 32768,
    notes: "sweet spot",
    fit: "comfortable" as const,
    recommended: true,
  },
  {
    id: "llama3.1:70b",
    name: "Llama 3.1 70B",
    parameters: "70B",
    download_mb: 39600,
    min_memory_mb: 45056,
    context_tokens: 131072,
    notes: "workstation",
    fit: "too_large" as const,
    recommended: false,
  },
];

const flush = async () => {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};

function mockState(status: Record<string, unknown>) {
  vi.mocked(api.localRuntime.recommended).mockResolvedValue({ system, models } as never);
  vi.mocked(api.localRuntime.status).mockResolvedValue({
    installed: true,
    binary_path: "/usr/bin/ollama",
    running: false,
    ownership: "not_running",
    endpoint: "http://localhost:11434",
    installed_models: [],
    install_url: "https://ollama.com/download",
    installer: null,
    ...status,
  } as never);
}

describe("LocalModelsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.onNotification).mockReturnValue(() => {});
  });

  it("states what the machine actually has, since that decides everything else", async () => {
    mockState({});
    render(<LocalModelsPanel />);
    await flush();

    expect(screen.getByText(/16 cores/)).toBeInTheDocument();
    expect(screen.getByText(/RAM 32 GB/)).toBeInTheDocument();
    expect(screen.getByText(/RTX 4070/)).toBeInTheDocument();
  });

  /// The whole point of sizing: a model that cannot load must be visibly
  /// excluded rather than silently offered.
  it("does not let you download a model this machine cannot run", async () => {
    mockState({ running: true, ownership: "managed" });
    render(<LocalModelsPanel />);
    await flush();

    expect(screen.getByText("Not enough memory")).toBeInTheDocument();
    const buttons = screen.getAllByRole("button", { name: /GB/ });
    const tooLarge = buttons.find((b) => b.textContent?.includes("39"));
    expect(tooLarge).toBeDisabled();
  });

  it("cannot download anything until the runtime is running", async () => {
    mockState({ running: false });
    render(<LocalModelsPanel />);
    await flush();

    const fits = screen.getAllByRole("button", { name: /GB/ }).find((b) => b.textContent?.includes("4.6"));
    expect(fits).toBeDisabled();
  });

  it("starting the runtime calls start and reflects the new state", async () => {
    mockState({ running: false });
    vi.mocked(api.localRuntime.start).mockResolvedValue({
      installed: true,
      running: true,
      ownership: "managed",
      endpoint: "http://localhost:11434",
      installed_models: [],
      install_url: "https://ollama.com/download",
      binary_path: "/usr/bin/ollama",
    } as never);

    render(<LocalModelsPanel />);
    await flush();
    fireEvent.click(screen.getByRole("button", { name: /Start/ }));
    await flush();

    expect(api.localRuntime.start).toHaveBeenCalled();
  });

  /// Refusing to kill someone else's server is a real safety property, so the
  /// UI must not offer a button that would.
  it("will not offer to stop a server CID did not start", async () => {
    mockState({ running: true, ownership: "external" });
    render(<LocalModelsPanel />);
    await flush();

    expect(screen.getByText("started outside CID")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Stop/ })).toBeDisabled();
  });

  it("falls back to the download link when no installer exists on the platform", async () => {
    mockState({ installed: false, running: false, installer: null });
    render(<LocalModelsPanel />);
    await flush();

    expect(screen.getByText(/No package manager found/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /ollama.com/ })).toHaveAttribute(
      "href",
      "https://ollama.com/download",
    );
    expect(screen.queryByRole("button", { name: /Install Ollama/ })).not.toBeInTheDocument();
  });

  it("offers one-click install when a platform installer exists, and runs it after confirming", async () => {
    mockState({ installed: false, running: false, installer: "winget" });
    vi.mocked(confirmDialog).mockResolvedValueOnce(true);
    vi.mocked(api.localRuntime.install).mockResolvedValue({
      installed: true,
      binary_path: "C:\\Users\\me\\AppData\\Local\\Programs\\Ollama\\ollama.exe",
      running: false,
      ownership: "not_running",
      endpoint: "http://localhost:11434",
      installed_models: [],
      install_url: "https://ollama.com/download",
      installer: "winget",
    } as never);

    render(<LocalModelsPanel />);
    await flush();

    const button = screen.getByRole("button", { name: /Install Ollama/ });
    fireEvent.click(button);

    await waitFor(() => expect(api.localRuntime.install).toHaveBeenCalled());
  });

  it("does not install anything if the confirmation is declined", async () => {
    mockState({ installed: false, running: false, installer: "winget" });
    vi.mocked(confirmDialog).mockResolvedValueOnce(false);
    vi.mocked(api.localRuntime.install).mockClear();

    render(<LocalModelsPanel />);
    await flush();

    fireEvent.click(screen.getByRole("button", { name: /Install Ollama/ }));

    await flush();
    expect(api.localRuntime.install).not.toHaveBeenCalled();
  });

  it("shows an already-downloaded model as installed instead of offering it again", async () => {
    mockState({ running: true, ownership: "managed", installed_models: ["qwen2.5-coder:7b"] });
    render(<LocalModelsPanel />);
    await flush();

    await waitFor(() => expect(screen.getByText("Downloaded")).toBeInTheDocument());
  });
});
