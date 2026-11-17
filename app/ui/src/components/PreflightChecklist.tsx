import { CircleCheck, OctagonX, TriangleAlert } from "lucide-react";
import type { PreflightCheckDto } from "../types/api";

const icon = { ok: CircleCheck, warn: TriangleAlert, block: OctagonX };

export function PreflightChecklist({ checks }: { checks: PreflightCheckDto[] }) {
  return (
    <ul className="preflight">
      {checks.map((c) => {
        const Icon = icon[c.status];
        return (
          <li key={c.id} className={`preflight__item preflight__item--${c.status}`}>
            <Icon aria-hidden />
            <div>
              <strong>{c.label}</strong>
              <span className="visually-hidden">{c.status === "ok" ? " (correcto)" : c.status === "warn" ? " (aviso)" : " (bloquea)"}</span>
              <div className="muted">{c.detail}</div>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
