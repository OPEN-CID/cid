import { describe, it, expect, vi, beforeEach } from "vitest";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    settings: {
      update: vi.fn().mockResolvedValue({}),
      get: vi.fn(),
    },
  },
}));

describe("useTheme", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    vi.mocked(api.settings.update).mockClear();
    vi.mocked(api.settings.get).mockReset();
    vi.resetModules();
  });

  it("defaults to dark when nothing is stored", async () => {
    const { useTheme } = await import("./useTheme");
    expect(useTheme.getState().theme).toBe("dark");
  });

  it("setTheme updates state, the DOM attribute, localStorage, and persists to settings", async () => {
    const { useTheme } = await import("./useTheme");

    useTheme.getState().setTheme("light");

    expect(useTheme.getState().theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("cid-theme")).toBe("light");
    expect(vi.mocked(api.settings.update)).toHaveBeenCalledWith({ theme: "light" });
  });

  it("toggleTheme flips between dark and light", async () => {
    const { useTheme } = await import("./useTheme");

    expect(useTheme.getState().theme).toBe("dark");
    useTheme.getState().toggleTheme();
    expect(useTheme.getState().theme).toBe("light");
    useTheme.getState().toggleTheme();
    expect(useTheme.getState().theme).toBe("dark");
  });

  it("a repeated import reads the previously persisted choice back from localStorage", async () => {
    const first = await import("./useTheme");
    first.useTheme.getState().setTheme("light");

    vi.resetModules();
    const second = await import("./useTheme");
    expect(second.useTheme.getState().theme).toBe("light");
  });

  it("syncFromSettings adopts the backend theme when this device has no explicit local choice", async () => {
    vi.mocked(api.settings.get).mockResolvedValue({ theme: "light" });
    const { useTheme } = await import("./useTheme");

    expect(useTheme.getState().theme).toBe("dark");
    await useTheme.getState().syncFromSettings();

    expect(useTheme.getState().theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("syncFromSettings does not override an explicit local choice on this device", async () => {
    vi.mocked(api.settings.get).mockResolvedValue({ theme: "light" });
    const { useTheme } = await import("./useTheme");

    useTheme.getState().setTheme("dark"); // explicit choice, even though it matches the default
    await useTheme.getState().syncFromSettings();

    expect(useTheme.getState().theme).toBe("dark");
  });
});
