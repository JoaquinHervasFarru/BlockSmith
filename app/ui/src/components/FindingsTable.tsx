import { CircleCheck, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { severityLabel } from "../lib/labels";
import { formatNumber } from "../lib/format";
import type { FindingDto, SeverityKey } from "../types/api";
import { SeverityBadge } from "./Badges";

type Filter = "all" | SeverityKey;
const PAGE = 200;

export function FindingsTable({ findings }: { findings: FindingDto[] }) {
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const counts = useMemo(() => {
    const c: Record<SeverityKey, number> = { critical: 0, minor: 0, cosmetic: 0 };
    findings.forEach((f) => c[f.severity]++);
    return c;
  }, [findings]);
  const q = query.trim().toLowerCase();
  const visible = findings.filter(
    (f) => (filter === "all" || f.severity === filter)
      && (!q || f.message.toLowerCase().includes(q) || f.code.toLowerCase().includes(q) || (f.path ?? "").toLowerCase().includes(q)),
  );
  return (
    <section className="card">
      <div className="card__header card__header--wrap">
        <h3>Hallazgos</h3>
        <div className="row row--wrap">
          <label className="search">
            <Search aria-hidden />
            <input className="input" placeholder="Buscar por texto, código o ruta" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Buscar hallazgos" />
          </label>
          <div className="segmented" role="group" aria-label="Filtrar por severidad">
            {(["all", "critical", "minor", "cosmetic"] as Filter[]).map((k) => (
              <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>
                {k === "all" ? `Todos (${findings.length})` : `${severityLabel[k]} (${counts[k]})`}
              </button>
            ))}
          </div>
        </div>
      </div>
      {visible.length === 0 ? (
        <p className="card__body muted">{findings.length === 0 ? "No se encontraron problemas en este volumen." : "Ningún hallazgo coincide con el filtro."}</p>
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr><th style={{ width: 120 }}>Severidad</th><th>Problema</th><th style={{ width: 230 }}>Código</th><th style={{ width: 140 }}>Estado</th></tr>
            </thead>
            <tbody>
              {visible.slice(0, limit).map((f) => (
                <tr key={f.id}>
                  <td><SeverityBadge severity={f.severity} /></td>
                  <td>
                    {f.message}
                    {f.path && <div className="path">{f.path}</div>}
                    {f.nodeNumber != null && <div className="subtle mono">nodo {formatNumber(f.nodeNumber)}</div>}
                  </td>
                  <td><span className="badge badge--outline badge--mono">{f.code}</span></td>
                  <td>{f.fixedInRebuild ? <span className="badge badge--ok"><CircleCheck aria-hidden /> Corregido</span> : <span className="subtle">Pendiente</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {visible.length > limit && (
        <div className="card__body">
          <button className="btn btn--secondary btn--sm" onClick={() => setLimit(limit + PAGE)}>
            Mostrar {Math.min(PAGE, visible.length - limit)} más de {formatNumber(visible.length - limit)}
          </button>
        </div>
      )}
    </section>
  );
}
