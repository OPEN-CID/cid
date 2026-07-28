import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { ChatThread } from "./ChatThread";

// Mock API
vi.mock("@/lib/api", () => ({
  api: {
    onNotification: () => () => {},
  },
}));

vi.mock("@/hooks/useCid", () => ({
  useCid: () => ({
    selectedMissionId: null,
    messages: {},
    addMessage: vi.fn(),
    updateMessage: vi.fn(),
    loadMessages: vi.fn(),
  }),
}));

describe("ChatThread", () => {
  it("shows empty state when no mission selected", () => {
    render(<ChatThread />);
    expect(screen.getByText(/No mission selected/)).toBeInTheDocument();
    expect(screen.getByText(/Flow 1 – First Mission/)).toBeInTheDocument();
  });
});
