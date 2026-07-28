import { create } from "zustand";
import { api } from "@/lib/api";

export type ThemeName = "dark" | "light";

const STORAGE_KEY = "cid-theme";

function applyThemeToDom(theme: ThemeName) {
  document.documentElement.setAttribute("data-theme", theme);
}

function readInitialTheme(): ThemeName {
  return localStorage.getItem(STORAGE_KEY) === "light" ? "light" : "dark";
}

type ThemeState = {
  theme: ThemeName;
  setTheme: (theme: ThemeName) => void;
  toggleTheme: () => void;
  /** Reconciles with the user's saved backend setting, called once on app
   * start. Backend wins over the localStorage default only when the user
   * hasn't made an explicit choice on *this* device (index.html's inline
   * script already applied the localStorage/default value before first
   * paint, so this only adjusts things if they disagree). */
  syncFromSettings: () => Promise<void>;
};

/**
 * review_prompt.md §7: the toggle half of the theming system. Token values
 * themselves live in src/theme/tokens.json -> generated into src/index.css
 * (scripts/generate-theme-css.mjs) — this store only decides *which* of the
 * two generated blocks applies, via a `data-theme` attribute on `<html>`.
 */
export const useTheme = create<ThemeState>((set, get) => ({
  theme: typeof document !== "undefined" ? readInitialTheme() : "dark",

  setTheme: (theme) => {
    applyThemeToDom(theme);
    localStorage.setItem(STORAGE_KEY, theme);
    set({ theme });
    // Best-effort: Core may not be reachable yet, or at all (this UI can run
    // standalone). The local/DOM state above is already correct regardless.
    api.settings.update({ theme }).catch(() => {});
  },

  toggleTheme: () => {
    get().setTheme(get().theme === "dark" ? "light" : "dark");
  },

  syncFromSettings: () => {
    return api.settings
      .get()
      .then((settings: { theme?: string }) => {
        const backendTheme: ThemeName = settings?.theme === "light" ? "light" : "dark";
        const hasExplicitLocalChoice = localStorage.getItem(STORAGE_KEY) !== null;
        if (!hasExplicitLocalChoice && backendTheme !== get().theme) {
          applyThemeToDom(backendTheme);
          set({ theme: backendTheme });
        }
      })
      .catch(() => {
        // No Core connection yet — the local/default theme already applied
        // via index.html's inline script stands.
      });
  },
}));
