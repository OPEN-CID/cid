import type { ReactNode } from "react";
import { X, AlertCircle, CheckCircle2, Info } from "lucide-react";
import { useDialogStore } from "@/lib/dialog";
import { useFocusTrap } from "@/lib/useFocusTrap";
import { t } from "@/lib/i18n";

const TOAST_STYLES: Record<string, string> = {
  error: "bg-destructive text-destructive-foreground border-destructive",
  success: "bg-card border-green-500/50 text-foreground",
  info: "bg-card border text-foreground",
};

const TOAST_ICON: Record<string, ReactNode> = {
  error: <AlertCircle className="w-4 h-4 shrink-0" />,
  success: <CheckCircle2 className="w-4 h-4 shrink-0" />,
  info: <Info className="w-4 h-4 shrink-0" />,
};

/** Mounted once at the app root — the render target for toast/confirm/info state in src/lib/dialog.ts. */
export function DialogHost() {
  const toasts = useDialogStore((s) => s.toasts);
  const dismissToast = useDialogStore((s) => s.dismissToast);
  const confirmRequest = useDialogStore((s) => s.confirmRequest);
  const resolveConfirm = useDialogStore((s) => s.resolveConfirm);
  const infoRequest = useDialogStore((s) => s.infoRequest);
  const closeInfo = useDialogStore((s) => s.closeInfo);

  return (
    <>
      <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 max-w-sm">
        {toasts.map((item) => (
          <div
            key={item.id}
            role={item.kind === "error" ? "alert" : "status"}
            className={`rounded-lg shadow-lg px-3 py-2 text-sm flex items-start gap-2 border ${TOAST_STYLES[item.kind]}`}
          >
            {TOAST_ICON[item.kind]}
            <span className="flex-1 break-words">{item.message}</span>
            <button
              onClick={() => dismissToast(item.id)}
              aria-label="Dismiss"
              className="shrink-0 opacity-70 hover:opacity-100"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>
        ))}
      </div>

      {confirmRequest && (
        <ConfirmDialogModal message={confirmRequest.message} onResolve={resolveConfirm} />
      )}

      {infoRequest && <InfoDialogModal title={infoRequest.title} content={infoRequest.content} onClose={closeInfo} />}
    </>
  );
}

function ConfirmDialogModal({ message, onResolve }: { message: string; onResolve: (v: boolean) => void }) {
  const ref = useFocusTrap<HTMLDivElement>(true, () => onResolve(false));
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[110]">
      <div
        ref={ref}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-dialog-message"
        tabIndex={-1}
        className="bg-card border rounded-lg p-6 w-[420px] max-w-[90vw]"
      >
        <p id="confirm-dialog-message" className="text-sm mb-4">
          {message}
        </p>
        <div className="flex justify-end gap-2">
          <button onClick={() => onResolve(false)} className="px-3 py-1.5 text-sm bg-secondary rounded">
            {t().common.cancel}
          </button>
          <button
            onClick={() => onResolve(true)}
            className="px-3 py-1.5 text-sm bg-destructive text-destructive-foreground rounded"
          >
            {t().common.confirm}
          </button>
        </div>
      </div>
    </div>
  );
}

function InfoDialogModal({ title, content, onClose }: { title: string; content: string; onClose: () => void }) {
  const ref = useFocusTrap<HTMLDivElement>(true, onClose);
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-[110]">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby="info-dialog-title"
        tabIndex={-1}
        className="bg-card border rounded-lg p-6 w-[560px] max-w-[90vw] max-h-[80vh] flex flex-col"
      >
        <div className="flex items-center justify-between mb-3">
          <h2 id="info-dialog-title" className="font-semibold text-sm">
            {title}
          </h2>
          <button onClick={onClose} aria-label={t().common.close}>
            <X className="w-4 h-4" />
          </button>
        </div>
        <pre className="text-xs bg-background border rounded p-2 overflow-auto flex-1">{content}</pre>
      </div>
    </div>
  );
}
