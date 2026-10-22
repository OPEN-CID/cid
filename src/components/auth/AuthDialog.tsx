import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/api";
import { toast } from "@/lib/dialog";
import { User, LogOut, X } from "lucide-react";

export const AUTH_TOKEN_KEY = "cid.session_token";

export function storedSessionToken(): string | null {
  try {
    return localStorage.getItem(AUTH_TOKEN_KEY);
  } catch {
    return null;
  }
}

type WhoAmI = { username?: string; role?: string } | null;

// Multi-user sign-in. The access-token box in the connection banner unlocks
// a token-protected Core; this dialog signs a *person* in, which is what
// admin-gated actions (governance policy changes, role management) require.
// Either can exist without the other, so neither borrows the other's UI.
export function AuthDialog({ onClose }: { onClose: () => void }) {
  const [bootstrapped, setBootstrapped] = useState<boolean | null>(null);
  const [me, setMe] = useState<WhoAmI>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const status = await api.auth.status();
      setBootstrapped(!!status?.bootstrapped);
      const token = storedSessionToken();
      if (token) {
        try {
          const session = await api.auth.session(token);
          setMe({ username: session?.username, role: session?.role });
        } catch {
          localStorage.removeItem(AUTH_TOKEN_KEY);
          setMe(null);
        }
      } else {
        setMe(null);
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const submit = async (kind: "login" | "register") => {
    if (!username.trim() || !password) return;
    setBusy(true);
    setError(null);
    try {
      // Registration returns the user record, not a session — sign straight
      // in afterwards so "create account" ends signed in, not on a login
      // form asking for the password just typed.
      if (kind === "register") {
        await api.auth.register({ username: username.trim(), password });
      }
      const res = await api.auth.login(username.trim(), password);
      const token = res?.token ?? res?.session_token;
      if (!token) throw new Error("no session token returned");
      localStorage.setItem(AUTH_TOKEN_KEY, token);
      setPassword("");
      await refresh();
      toast.success(kind === "login" ? "Signed in" : "Account created — you are the Owner");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const logout = async () => {
    const token = storedSessionToken();
    if (token) {
      try {
        await api.auth.logout(token);
      } catch {
        // The session may already be expired server-side; clearing the
        // local copy is still the correct end state.
      }
      localStorage.removeItem(AUTH_TOKEN_KEY);
    }
    setMe(null);
    setPassword("");
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card border rounded-lg p-5 w-[380px] max-w-[90vw]" role="dialog" aria-label="Sign in">
        <div className="flex items-center gap-2 mb-4">
          <User className="w-4 h-4" />
          <h2 className="font-semibold">Sign in</h2>
          <button onClick={onClose} aria-label="Close" className="ml-auto text-muted-foreground hover:text-foreground">
            <X className="w-4 h-4" />
          </button>
        </div>

        {bootstrapped === null && !error && <div className="text-xs text-muted-foreground">Loading…</div>}

        {me ? (
          <div className="space-y-3 text-sm">
            <div>
              Signed in as <span className="font-medium">{me.username ?? "user"}</span>
              {me.role && <span className="text-muted-foreground"> ({me.role})</span>}
            </div>
            <button
              onClick={logout}
              className="flex items-center gap-1 text-xs px-3 py-1.5 rounded bg-secondary"
            >
              <LogOut className="w-3 h-3" /> Sign out
            </button>
          </div>
        ) : (
          bootstrapped !== null && (
            <div className="space-y-3">
              {!bootstrapped && (
                <div className="text-[11px] text-muted-foreground">
                  No accounts exist yet — the first registration becomes the Owner.
                </div>
              )}
              <input
                className="w-full bg-background border rounded px-2 py-1.5 text-sm"
                placeholder="Username"
                autoComplete="username"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
              <input
                className="w-full bg-background border rounded px-2 py-1.5 text-sm"
                placeholder="Password"
                type="password"
                autoComplete={bootstrapped ? "current-password" : "new-password"}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit(bootstrapped ? "login" : "register");
                }}
              />
              <div className="flex gap-2">
                {bootstrapped ? (
                  <button
                    onClick={() => submit("login")}
                    disabled={busy || !username.trim() || !password}
                    className="text-xs px-3 py-1.5 rounded bg-primary text-primary-foreground disabled:opacity-50"
                  >
                    Sign in
                  </button>
                ) : (
                  <button
                    onClick={() => submit("register")}
                    disabled={busy || !username.trim() || !password}
                    className="text-xs px-3 py-1.5 rounded bg-primary text-primary-foreground disabled:opacity-50"
                  >
                    Create owner account
                  </button>
                )}
              </div>
            </div>
          )
        )}
        {error && <div className="text-[11px] text-red-400 mt-3">{error}</div>}
      </div>
    </div>
  );
}
