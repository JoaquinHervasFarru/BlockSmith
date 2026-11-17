import { CircleAlert, LoaderCircle } from "lucide-react";
import type { ReactNode } from "react";

export function ErrorCallout({ title = "Algo ha fallado", message, action }: { title?: string; message: string; action?: ReactNode }) {
  return (
    <div className="callout callout--crit" role="alert">
      <CircleAlert aria-hidden />
      <div className="grow">
        <strong>{title}</strong>
        {message}
        {action && <div style={{ marginTop: 8 }}>{action}</div>}
      </div>
    </div>
  );
}

export function Loading({ label = "Cargando…" }: { label?: string }) {
  return (
    <div className="loading" role="status">
      <LoaderCircle className="spin" aria-hidden /> {label}
    </div>
  );
}

export function EmptyState({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="card card--pad empty-state">
      {icon}
      <p><strong>{title}</strong></p>
      {children && <div className="muted">{children}</div>}
    </div>
  );
}

export function ProgressBar({ value, label }: { value: number | null; label?: string }) {
  return (
    <div className={"progress" + (value === null ? " progress--indeterminate" : "")} role="progressbar"
      aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={value === null ? undefined : Math.round(value * 100)}>
      <span style={{ width: value === null ? undefined : `${Math.min(100, value * 100)}%` }} />
    </div>
  );
}
