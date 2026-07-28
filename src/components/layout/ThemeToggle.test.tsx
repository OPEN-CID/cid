import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ThemeToggle } from "./ThemeToggle";
import { useTheme } from "@/theme/useTheme";

vi.mock("@/lib/api", () => ({
  api: {
    settings: { update: vi.fn().mockResolvedValue({}), get: vi.fn().mockResolvedValue({}) },
  },
}));

describe("ThemeToggle", () => {
  beforeEach(() => {
    useTheme.setState({ theme: "dark" });
  });

  it("shows a sun icon and 'switch to light' label in dark mode", () => {
    render(<ThemeToggle />);
    expect(screen.getByLabelText("Switch to light theme")).toBeInTheDocument();
  });

  it("shows a moon icon and 'switch to dark' label in light mode", () => {
    useTheme.setState({ theme: "light" });
    render(<ThemeToggle />);
    expect(screen.getByLabelText("Switch to dark theme")).toBeInTheDocument();
  });

  it("clicking toggles the shared theme store", () => {
    render(<ThemeToggle />);
    fireEvent.click(screen.getByLabelText("Switch to light theme"));
    expect(useTheme.getState().theme).toBe("light");
  });
});
