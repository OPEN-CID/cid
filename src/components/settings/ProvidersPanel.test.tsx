import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ProvidersPanel } from "./ProvidersPanel";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    settings: { get: vi.fn(), update: vi.fn() },
    localRuntime: { list: vi.fn(), detect: vi.fn() },
    // TeamIntegrationsPanel (051 Wave 5.1d) is rendered inside ProvidersPanel now.
    slack: { configGet: vi.fn().mockResolvedValue({ configured: false }), configure: vi.fn() },
    teams: { configGet: vi.fn().mockResolvedValue({ configured: false }), configure: vi.fn() },
  },
}));

async function flushAsyncWork() {
  await act(async () => {
    for (let i = 0; i < 6; i++) {
      await Promise.resolve();
    }
  });
}

const runtimes = [
  { name: "Ollama", available: true, version: "0.5.1", models: [{ id: "llama3.1" }, { id: "qwen2.5" }] },
  { name: "LM Studio", available: false, models: [] },
];

describe("ProvidersPanel", () => {
  beforeEach(() => {
    vi.mocked(api.settings.get).mockReset();
    vi.mocked(api.settings.update).mockReset();
    vi.mocked(api.localRuntime.list).mockReset();
    vi.mocked(api.localRuntime.detect).mockReset();
  });

  it("loads settings and detected local runtimes on mount", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({ anthropic_api_key: "sk-ant-...abcd" });
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce(runtimes);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    expect(screen.getByText("Ollama")).toBeInTheDocument();
    expect(screen.getByText("LM Studio")).toBeInTheDocument();
    expect(screen.queryByText(/No local runtime detected/)).not.toBeInTheDocument();
  });

  it("shows the no-runtime message when nothing is available", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    expect(screen.getByText(/No local runtime detected/)).toBeInTheDocument();
  });

  it("rescan calls detect(true) and replaces the runtime list", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);
    vi.mocked(api.localRuntime.detect).mockResolvedValueOnce(runtimes);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Rescan"));
    await flushAsyncWork();

    expect(vi.mocked(api.localRuntime.detect)).toHaveBeenCalledWith(true);
    expect(screen.getByText("Ollama")).toBeInTheDocument();
  });

  it("saving does not resend a redacted key the user never touched", async () => {
    vi.mocked(api.settings.get).mockResolvedValue({ anthropic_api_key: "sk-ant-...abcd" });
    vi.mocked(api.localRuntime.list).mockResolvedValue([]);
    vi.mocked(api.settings.update).mockResolvedValueOnce({});

    render(<ProvidersPanel />);
    await flushAsyncWork();

    fireEvent.click(screen.getByText("Save"));
    await flushAsyncWork();

    const payload = vi.mocked(api.settings.update).mock.calls[0][0] as { anthropic_api_key?: string };
    expect(payload.anthropic_api_key).toBeUndefined();
  });

  it("saving does send a newly-typed key", async () => {
    vi.mocked(api.settings.get).mockResolvedValue({ anthropic_api_key: "sk-ant-...abcd" });
    vi.mocked(api.localRuntime.list).mockResolvedValue([]);
    vi.mocked(api.settings.update).mockResolvedValueOnce({});

    render(<ProvidersPanel />);
    await flushAsyncWork();

    // The redacted stored value becomes the placeholder itself once loaded.
    fireEvent.change(screen.getByPlaceholderText("sk-ant-...abcd"), {
      target: { value: "sk-ant-brand-new-real-key" },
    });
    fireEvent.click(screen.getByText("Save"));
    await flushAsyncWork();

    const payload = vi.mocked(api.settings.update).mock.calls[0][0] as { anthropic_api_key?: string };
    expect(payload.anthropic_api_key).toBe("sk-ant-brand-new-real-key");
  });
});
