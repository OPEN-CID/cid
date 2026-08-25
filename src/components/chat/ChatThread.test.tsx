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
    selectedSessionId: null,
    messages: {},
    addMessage: vi.fn(),
    updateMessage: vi.fn(),
    loadMessages: vi.fn(),
  }),
}));

describe("ChatThread", () => {
  it("shows empty state when no session selected", () => {
    render(<ChatThread />);
    expect(screen.getByText(/No session selected/)).toBeInTheDocument();
    expect(screen.getByText(/Flow 1 – First Session/)).toBeInTheDocument();
  });
});
