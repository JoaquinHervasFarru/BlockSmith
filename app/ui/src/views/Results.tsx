import { CircleCheck, Hammer } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { SeverityBadge } from "../components/Badges";
import { StatTile } from "../components/StatTile";
import { formatNumber } from "../lib/format";
import { severityLabel } from "../lib/labels";
import type { Finding, Severity } from "../types/models";

type Filter = "all" | Severity;

export function Results() {
  const [findings, setFindings] = useState<Finding[]>([]);
  const [filter, setFilter] = useState<Filter>("all");

  useEffect(() => {
    api.findings(1).then(setFindings);
  }, []);

  const counts = useMemo(() => {
    const c: Record<Severity, number> = { critical: 0, minor: 0, cosmetic: 0 };
    findings.forEach((f) => c[f.severity]++);
    return c;
  }, [findings]);
  const visible = filter === "all" ? findings : findings.filter((f) => f.severity === filter);

  return (
    <>
      <div className="page-header">
        <div>
          <h1>Archivo Fotos · resultados del escaneo</h1>
          <p>HFS+ · Samsung SSD 870 EVO (USB) · escaneado el 16 nov 2026, 23:51</p>
        </div>
        <div className="row">
          <button className="btn btn--secondary">Exportar informe</button>
          <button className="btn btn--primary"><Hammer aria-hidden /> Ver reconstrucción</button>
        </div>
      </div>

      <div className="stat-grid">
        <StatTile label="Nodos del catálogo" value={formatNumber(48_211)} hint={<>Tras reconstruir: <strong>{formatNumber(31_877)}</strong> (−34 %)</>} />
        <StatTile label="Nodos dañados" value={37} hint="Todos los registros recuperados" />
        <StatTile label="Fragmentación del directorio" value="41,8 %" hint="Tras reconstruir: 0 %" />
        <StatTile label="Elementos recuperados" value={41} hint="2 en «Recovered Items»" />
      </div>

      <section className="card">
        <div className="card__header">
          <h3>Hallazgos</h3>
          <div className="segmented" role="group" aria-label="Filtrar por severidad">
            {(["all", "critical", "minor", "cosmetic"] as Filter[]).map((k) => (
              <button key={k} aria-pressed={filter === k} onClick={() => setFilter(k)}>
                {k === "all" ? `Todos (${findings.length})` : `${severityLabel[k]} (${counts[k]})`}
              </button>
            ))}
          </div>
        </div>
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr><th style={{ width: 120 }}>Severidad</th><th>Problema</th><th style={{ width: 210 }}>Código</th><th style={{ width: 150 }}>Reconstrucción</th></tr>
            </thead>
            <tbody>
              {visible.map((f) => (
                <tr key={f.id}>
                  <td><SeverityBadge severity={f.severity} /></td>
                  <td>
                    {f.message}
                    {f.path && <div className="path">{f.path}</div>}
                    {f.nodeNumber != null && <div className="subtle mono">nodo {formatNumber(f.nodeNumber)}</div>}
                  </td>
                  <td><span className="badge badge--outline badge--mono">{f.code}</span></td>
                  <td>
                    {f.fixedInRebuild
                      ? <span className="badge badge--ok"><CircleCheck aria-hidden /> Corregido</span>
                      : <span className="subtle">Requiere revisión</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </>
  );
}
