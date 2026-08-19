import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { api, wsBearerProtocol, AuthRequiredError } from "./api";

// The gap these cover shipped invisibly: Core has always required a bearer
// token off loopback, and the browser client had no way to send one — every
// hosted deployment would have failed with a closed socket and a 401. Nothing
// in the suite noticed, because every tested environment (dev, Tauri, E2E) is
// loopback with no token at all.

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];
  static readonly OPEN = 1;
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];

  constructor(
    public url: string,
    public protocols?: string | string[],
  ) {
    FakeWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = 3;
  }
}

describe("wsBearerProtocol", () => {
  it("base64url-encodes the token without padding", () => {
    // A subprotocol must be a valid HTTP token, so `+`, `/`, and `=` cannot
    // appear. Must match ws_bearer_protocol in cid-core/src/access/mod.rs.
    expect(wsBearerProtocol("abc")).toBe("cid.bearer.YWJj");
    const awkward = wsBearerProtocol("tok en,with/odd+chars");
    expect(awkward.startsWith("cid.bearer.")).toBe(true);
    expect(awkward).not.toMatch(/[+/=]/);
  });

  it("round-trips through the same decoding Core performs", () => {
    const token = "s3cret-token";
    const encoded = wsBearerProtocol(token).replace("cid.bearer.", "");
    const decoded = atob(encoded.replace(/-/g, "+").replace(/_/g, "/"));
    expect(decoded).toBe(token);
  });
});

describe("CidApiClient authentication", () => {
  beforeEach(() => {
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    localStorage.clear();
    api.setAuthToken(null);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    api.setAuthToken(null);
  });

  it("persists the token so a reload does not need it pasted again", () => {
    expect(api.hasAuthToken()).toBe(false);

    api.setAuthToken("stored-token");

    expect(api.hasAuthToken()).toBe(true);
    expect(localStorage.getItem("cid.auth_token")).toBe("stored-token");
  });

  it("trims the token, since a pasted value usually carries whitespace", () => {
    api.setAuthToken("  padded-token \n");
    expect(localStorage.getItem("cid.auth_token")).toBe("padded-token");
  });

  it("clears the stored token", () => {
    api.setAuthToken("stored-token");
    api.setAuthToken(null);

    expect(api.hasAuthToken()).toBe(false);
    expect(localStorage.getItem("cid.auth_token")).toBeNull();
  });

  it("offers the token as a subprotocol on the WebSocket handshake", () => {
    api.setAuthToken("s3cret-token");
    api.connect();

    const socket = FakeWebSocket.instances.at(-1);
    expect(socket?.protocols).toEqual([wsBearerProtocol("s3cret-token")]);
  });

  it("omits the subprotocol entirely when there is no token", () => {
    api.connect();

    const socket = FakeWebSocket.instances.at(-1);
    expect(socket?.protocols).toBeUndefined();
  });

  it("sends the bearer header on HTTP RPC when a token is set", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", FakeWebSocket);
    api.setAuthToken("s3cret-token");

    await api.call("repo.list");

    const headers = fetchMock.mock.calls[0][1].headers;
    expect(headers["Authorization"]).toBe("Bearer s3cret-token");
  });

  it("sends no Authorization header when no token is set", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: () => Promise.resolve({ jsonrpc: "2.0", id: "1", result: [] }),
    });
    vi.stubGlobal("fetch", fetchMock);

    await api.call("repo.list");

    expect(fetchMock.mock.calls[0][1].headers["Authorization"]).toBeUndefined();
  });

  it("raises a distinguishable error on 401 so the UI can prompt", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 401, json: () => Promise.resolve({}) }),
    );

    await expect(api.call("repo.list")).rejects.toBeInstanceOf(AuthRequiredError);
  });

  it("distinguishes a rejected token from a missing one", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: false, status: 401, json: () => Promise.resolve({}) }),
    );
    vi.stubGlobal("WebSocket", FakeWebSocket);
    api.setAuthToken("wrong-token");

    await expect(api.call("repo.list")).rejects.toThrow("Access token rejected by Core");
  });
});
