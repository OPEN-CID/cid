import { useCallback, useEffect, useState } from "react";
import { api, type ModelInfo } from "@/lib/api";

export type ModelReadiness = {
  /** True once at least one model can actually be called. */
  ready: boolean;
  /** Still asking Core — render nothing rather than a wrong answer. */
  checking: boolean;
  /** The model that would actually run, for honest display. */
  activeModel: string | null;
  refresh: () => void;
};

/**
 * Whether CID can run an agent at all.
 *
 * This exists because the product failed silently without it: with no provider
 * key and no local model, creating a Session still "worked", the Planner
 * produced a placeholder plan, the Implementer was blocked, and the status bar
 * reported `claude-sonnet-5 (anthropic)` — a model that could not be called,
 * because the display defaulted the provider to "anthropic" and the id to the
 * schema default regardless of whether a key existed. The whole product looked
 * broken rather than unconfigured.
 *
 * `available` on `model.list` is Core's own answer to "is this callable" (it
 * reflects key presence per provider), so readiness is derived from that rather
 * than re-implementing the rule here.
 */
export function useModelReadiness(connected: boolean): ModelReadiness {
  const [models, setModels] = useState<ModelInfo[] | null>(null);
  const [nonce, setNonce] = useState(0);

  const refresh = useCallback(() => setNonce((n) => n + 1), []);

  useEffect(() => {
    if (!connected) return;
    let cancelled = false;
    api.model
      .list()
      .then((list) => {
        if (!cancelled) setModels(Array.isArray(list) ? list : []);
      })
      .catch(() => {
        // Treated as "unknown", not "not ready" — a failed lookup must not
        // put a scary banner in front of a working install.
        if (!cancelled) setModels(null);
      });
    return () => {
      cancelled = true;
    };
  }, [connected, nonce]);

  const usable = (models ?? []).filter((m) => m.available);
  return {
    checking: connected && models === null,
    ready: usable.length > 0,
    activeModel: usable.length ? `${usable.find((m) => m.default)?.id ?? usable[0].id}` : null,
    refresh,
  };
}
