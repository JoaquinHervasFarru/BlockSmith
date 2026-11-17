import { TriangleAlert } from "lucide-react";
import { useEffect, useRef, type ReactNode } from "react";

/** Diálogo de confirmación propio: no depende de window.confirm, que cada webview
 *  implementa a su manera (en algunas ni siquiera se muestra). */
export function ConfirmDialog({ title, children, confirmLabel, tone = "primary", onConfirm, onCancel }: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  tone?: "primary" | "write";
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancelRef.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  return (
    <div className="modal" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" onClick={onCancel}>
      <div className="modal__panel card card--pad stack" onClick={(e) => e.stopPropagation()}>
        <div className="row"><TriangleAlert className="confirm__icon" aria-hidden /><h3 id="confirm-title">{title}</h3></div>
        <div className="muted">{children}</div>
        <div className="row confirm__actions">
          <button ref={cancelRef} className="btn btn--secondary" onClick={onCancel}>Cancelar</button>
          <button className={tone === "write" ? "btn btn--write" : "btn btn--primary"} onClick={onConfirm}>{confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}
