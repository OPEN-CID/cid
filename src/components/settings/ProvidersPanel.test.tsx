import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { ProvidersPanel } from "./ProvidersPanel";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    settings: { get: vi.fn(), update: vi.fn() },
    localRuntime: { list: vi.fn(), detect: vi.fn() },
    model: { list: vi.fn().mockResolvedValue([]) },
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

// Field names mirror the real `model.list` wire response verified against a
// running Core (`available`, not `enabled`).
const models = [
  { id: "claude-sonnet-5", name: "Claude Sonnet 5", provider: "anthropic", context_length: 1_000_000, default: true, available: true },
  { id: "gpt-4o", name: "GPT-4o", provider: "openai", context_length: 128_000, default: false, available: false },
];

describe("ProvidersPanel", () => {
  beforeEach(() => {
    vi.mocked(api.settings.get).mockReset();
    vi.mocked(api.settings.update).mockReset();
    vi.mocked(api.localRuntime.list).mockReset();
    vi.mocked(api.localRuntime.detect).mockReset();
    vi.mocked(api.model.list).mockReset().mockResolvedValue([]);
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

  it("populates a per-role model dropdown from model.list, disabling models with no API key", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);
    vi.mocked(api.model.list).mockResolvedValueOnce(models);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    const gptOption = screen.getAllByText(/GPT-4o/)[0].closest("option") as HTMLOptionElement;
    expect(gptOption).toBeDisabled();
    const sonnetOption = screen.getAllByText(/Claude Sonnet 5/)[0].closest("option") as HTMLOptionElement;
    expect(sonnetOption).not.toBeDisabled();
  });

  it("falls back to the free-text model id input when model.list has nothing for the chosen provider", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);
    vi.mocked(api.model.list).mockResolvedValueOnce([]);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    expect(screen.getAllByPlaceholderText("model id").length).toBe(3);
  });

  it("custom model id… reveals the free-text input for a role that has a populated dropdown", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);
    vi.mocked(api.model.list).mockResolvedValueOnce(models);

    render(<ProvidersPanel />);
    await flushAsyncWork();

    expect(screen.queryByPlaceholderText("model id")).not.toBeInTheDocument();

    fireEvent.click(screen.getAllByText("custom model id…")[0]);

    expect(screen.getByPlaceholderText("model id")).toBeInTheDocument();
  });

  it("does not block rendering when model.list fails", async () => {
    vi.mocked(api.settings.get).mockResolvedValueOnce({});
    vi.mocked(api.localRuntime.list).mockResolvedValueOnce([]);
    vi.mocked(api.model.list).mockRejectedValueOnce(new Error("offline"));

    render(<ProvidersPanel />);
    await flushAsyncWork();

    expect(screen.getAllByPlaceholderText("model id").length).toBe(3);
  });
});
