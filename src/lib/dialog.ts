import { create } from "zustand";

// 050-Gold-Standard-Review.md F12 / 051 Wave 5.3: blocking, unstyled,
// untestable window.alert()/window.confirm() calls were the only error and
// confirmation UI in the app. A single zustand-backed store (matching the
// existing useCid/useTheme pattern) replaces both with real, dismissible,
// testable components rendered by <DialogHost/>.

export type ToastKind = "error" | "success" | "info";
export type ToastItem = { id: string; kind: ToastKind; message: string };

type ConfirmRequest = { message: string; resolve: (v: boolean) => void };
type InfoRequest = { title: string; content: string };

type DialogState = {
  toasts: ToastItem[];
  confirmRequest: ConfirmRequest | null;
  infoRequest: InfoRequest | null;
  pushToast: (kind: ToastKind, message: string) => void;
  dismissToast: (id: string) => void;
  requestConfirm: (message: string) => Promise<boolean>;
  resolveConfirm: (v: boolean) => void;
  showInfo: (title: string, content: string) => void;
  closeInfo: () => void;
};

let toastCounter = 0;

export const useDialogStore = create<DialogState>((set, get) => ({
  toasts: [],
  confirmRequest: null,
  infoRequest: null,

  pushToast: (kind, message) => {
    const id = `toast-${++toastCounter}`;
    set((s) => ({ toasts: [...s.toasts, { id, kind, message }] }));
    setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 4000);
  },

  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  requestConfirm: (message) =>
    new Promise<boolean>((resolve) => {
      set({ confirmRequest: { message, resolve } });
    }),

  resolveConfirm: (v) => {
    get().confirmRequest?.resolve(v);
    set({ confirmRequest: null });
  },

  showInfo: (title, content) => set({ infoRequest: { title, content } }),
  closeInfo: () => set({ infoRequest: null }),
}));

export const toast = {
  error: (message: string) => useDialogStore.getState().pushToast("error", message),
  success: (message: string) => useDialogStore.getState().pushToast("success", message),
  info: (message: string) => useDialogStore.getState().pushToast("info", message),
};

/** Promise-based replacement for `window.confirm`, resolved by DialogHost. */
export function confirmDialog(message: string): Promise<boolean> {
  return useDialogStore.getState().requestConfirm(message);
}

/** For content too large for a toast (e.g. a JSON dump) — a real modal instead of `window.alert`. */
export function showInfoDialog(title: string, content: string) {
  useDialogStore.getState().showInfo(title, content);
}
