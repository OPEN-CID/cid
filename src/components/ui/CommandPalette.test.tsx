import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { axe } from "vitest-axe";
import { CommandPalette, type Command } from "./CommandPalette";

// 051-Editor-Excellence-Roadmap.md Wave 5.2: Ctrl+K command palette — the
// keyboard-first entry point to every surface.

describe("CommandPalette", () => {
  const commands: Command[] = [
    { id: "a", label: "Go to Editor", action: vi.fn() },
    { id: "b", label: "Go to Terminal", action: vi.fn() },
    { id: "c", label: "New Mission", action: vi.fn() },
  ];

  it("is closed until Ctrl+K is pressed", () => {
    render(<CommandPalette commands={commands} />);
    expect(screen.queryByPlaceholderText("Type a command…")).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(screen.getByPlaceholderText("Type a command…")).toBeInTheDocument();
  });

  it("filters commands by typed text", () => {
    render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    fireEvent.change(screen.getByPlaceholderText("Type a command…"), { target: { value: "terminal" } });

    expect(screen.getByText("Go to Terminal")).toBeInTheDocument();
    expect(screen.queryByText("Go to Editor")).not.toBeInTheDocument();
  });

  it("Enter runs the highlighted command and closes the palette", () => {
    render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    const input = screen.getByPlaceholderText("Type a command…");
    fireEvent.change(input, { target: { value: "New Mission" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(commands[2].action).toHaveBeenCalled();
    expect(screen.queryByPlaceholderText("Type a command…")).not.toBeInTheDocument();
  });

  it("clicking a command runs it", () => {
    render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    fireEvent.click(screen.getByText("Go to Terminal"));

    expect(commands[1].action).toHaveBeenCalled();
  });

  it("Escape closes the palette without running anything", () => {
    render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const modal = screen.getByRole("dialog", { name: "Command palette" });

    fireEvent.keyDown(modal, { key: "Escape" });

    expect(screen.queryByPlaceholderText("Type a command…")).not.toBeInTheDocument();
    expect(commands[0].action).not.toHaveBeenCalled();
  });

  it("has no detectable accessibility violations when open", async () => {
    const { container } = render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });

    expect(await axe(container)).toHaveNoViolations();
  });

  it("ArrowDown moves the highlighted command before Enter runs it", () => {
    render(<CommandPalette commands={commands} />);
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const input = screen.getByPlaceholderText("Type a command…");

    fireEvent.keyDown(input, { key: "ArrowDown" });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(commands[1].action).toHaveBeenCalled();
  });
});
