import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { AuthDialog } from "./AuthDialog";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: {
    auth: {
      status: vi.fn(),
      register: vi.fn(),
      login: vi.fn(),
      logout: vi.fn(),
      session: vi.fn(),
    },
  },
}));

describe("AuthDialog", () => {
  beforeEach(() => {
    localStorage.clear();
    vi.mocked(api.auth.status).mockReset();
    vi.mocked(api.auth.register).mockReset();
    vi.mocked(api.auth.login).mockReset();
    vi.mocked(api.auth.logout).mockReset().mockResolvedValue({ ok: true } as any);
    vi.mocked(api.auth.session).mockReset();
  });

  it("offers owner creation on a fresh install and signs in afterwards", async () => {
    vi.mocked(api.auth.status).mockResolvedValue({ bootstrapped: false });
    vi.mocked(api.auth.register).mockResolvedValue({ id: "u1" } as any);
    vi.mocked(api.auth.login).mockResolvedValue({ token: "tok", username: "owner", role: "owner" } as any);
    vi.mocked(api.auth.session).mockResolvedValue({ username: "owner", role: "owner" } as any);

    render(<AuthDialog onClose={() => {}} />);
    expect(await screen.findByText(/first registration becomes the Owner/)).toBeInTheDocument();

    fireEvent.change(screen.getByPlaceholderText("Username"), { target: { value: "owner" } });
    fireEvent.change(screen.getByPlaceholderText("Password"), { target: { value: "pw" } });
    fireEvent.click(screen.getByText("Create owner account"));

    await waitFor(() => expect(api.auth.register).toHaveBeenCalledWith({ username: "owner", password: "pw" }));
    await waitFor(() => expect(api.auth.login).toHaveBeenCalledWith("owner", "pw"));
    expect(localStorage.getItem("cid.session_token")).toBe("tok");
    expect(await screen.findByText("owner")).toBeInTheDocument();
  });

  it("signs in on a bootstrapped install and signs out cleanly", async () => {
    vi.mocked(api.auth.status).mockResolvedValue({ bootstrapped: true });
    vi.mocked(api.auth.login).mockResolvedValue({ token: "tok", username: "dev", role: "developer" } as any);
    vi.mocked(api.auth.session).mockResolvedValue({ username: "dev", role: "developer" } as any);

    render(<AuthDialog onClose={() => {}} />);
    fireEvent.change(await screen.findByPlaceholderText("Username"), { target: { value: "dev" } });
    fireEvent.change(screen.getByPlaceholderText("Password"), { target: { value: "pw" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByText("dev")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Sign out"));
    await waitFor(() => expect(api.auth.logout).toHaveBeenCalledWith("tok"));
    expect(localStorage.getItem("cid.session_token")).toBeNull();
  });

  it("drops a rejected stored token instead of looping", async () => {
    localStorage.setItem("cid.session_token", "stale");
    vi.mocked(api.auth.status).mockResolvedValue({ bootstrapped: true });
    vi.mocked(api.auth.session).mockRejectedValue(new Error("expired"));

    render(<AuthDialog onClose={() => {}} />);
    await waitFor(() => expect(localStorage.getItem("cid.session_token")).toBeNull());
  });
});
