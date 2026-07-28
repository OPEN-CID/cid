import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { axe } from "vitest-axe";
import { DialogHost } from "./DialogHost";
import { toast, confirmDialog, showInfoDialog, useDialogStore } from "@/lib/dialog";

// 050-Gold-Standard-Review.md F12 / 051 Wave 5.3: the replacement for
// window.alert()/window.confirm() — real, dismissible, testable UI.

describe("DialogHost", () => {
  beforeEach(() => {
    useDialogStore.setState({ toasts: [], confirmRequest: null, infoRequest: null });
  });

  it("renders a toast pushed via toast.error and it can be dismissed", async () => {
    render(<DialogHost />);
    toast.error("Something broke");

    expect(await screen.findByText("Something broke")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Dismiss"));
    await waitFor(() => expect(screen.queryByText("Something broke")).not.toBeInTheDocument());
  });

  it("renders a success toast with status role, not alert", async () => {
    render(<DialogHost />);
    toast.success("Saved");

    expect(await screen.findByText("Saved")).toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("confirmDialog resolves true when Confirm is clicked", async () => {
    render(<DialogHost />);
    const pending = confirmDialog("Remove this MCP server?");

    expect(await screen.findByText("Remove this MCP server?")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Confirm"));

    expect(await pending).toBe(true);
    expect(screen.queryByText("Remove this MCP server?")).not.toBeInTheDocument();
  });

  it("confirmDialog resolves false when Cancel is clicked", async () => {
    render(<DialogHost />);
    const pending = confirmDialog("Remove this MCP server?");

    fireEvent.click(await screen.findByText("Cancel"));

    expect(await pending).toBe(false);
  });

  it("the confirm dialog has no detectable accessibility violations", async () => {
    const { container } = render(<DialogHost />);
    confirmDialog("Remove this MCP server?");
    await screen.findByText("Remove this MCP server?");

    expect(await axe(container)).toHaveNoViolations();
  });

  it("showInfoDialog renders the content and can be closed", async () => {
    render(<DialogHost />);
    showInfoDialog("Tools — test-server", JSON.stringify({ tools: ["a", "b"] }));

    expect(await screen.findByText("Tools — test-server")).toBeInTheDocument();
    expect(screen.getByText(/"tools"/)).toBeInTheDocument();

    fireEvent.click(screen.getByLabelText("Close"));
    await waitFor(() => expect(screen.queryByText("Tools — test-server")).not.toBeInTheDocument());
  });
});
