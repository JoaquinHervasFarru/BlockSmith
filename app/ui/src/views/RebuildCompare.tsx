import { ArrowLeft, Download, Filter, ShieldCheck, Wrench } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { backend } from "../api";
import { ChangeBadge } from "../components/ChangeBadge";
import { DirectoryTree } from "../components/DirectoryTree";
import { ErrorCallout, Loading } from "../components/Feedback";
import { StatTile } from "../components/StatTile";
import { formatBytes, formatNumber } from "../lib/format";
import { useAsync } from "../lib/useAsync";
import type { ChangeKey } from "../types/api";

export function RebuildCompare() {
  const id = Number(useParams().id);
  const rebuild = useAsync(() => backend.getRebuild(id), [id]);
  const diff = useAsync(() => backend.getDiff(id), [id]);
  const [kind, setKind] = useState<ChangeKey | "all">("all");
  const [onlyChanges, setOnlyChanges] = useState(false);
  const visible = useMemo(() => (diff.data ?? []).filter((d) => kind === "all" || d.change === kind), [diff.data, kind]);

  if (rebuild.error) return <ErrorCallout message={rebuild.error} />;
  if (!rebuild.data) return <Loading />;
  const r = rebuild.data;
  const saving = r.nodesBefore > 0 ? Math.round(100 * (1 - r.nodesAfter / r.nodesBefore)) : 0;

  return (
    <>
      <Link to={`/sesion/${r.sessionId}`} className="back-link"><ArrowLeft aria-hidden /> Resultados del análisis</Link>
      <div className="page-header">
        <div>
          <h1>{r.volumeName ?? "Volumen"} · reconstrucción</h1>
          <p>Vista previa: nada se ha escrito todavía. Revisa los cambios y exporta una imagen reparada.</p>
        </div>
        {r.status === "done" && r.target === "in_place" ? (
          <Link className="btn btn--secondary" to={`/reconstruccion/${id}/aplicar`}><ShieldCheck aria-hidden /> Ver reparación</Link>
        ) : r.status === "done" && r.outputPath ? (
          <Link className="btn btn--secondary" to={`/reconstruccion/${id}/exportar`}><ShieldCheck aria-hidden /> Ver exportación</Link>
        ) : (
          <div className="row row--wrap">
            <Link className="btn btn--secondary" to={`/reconstruccion/${id}/aplicar`}><Wrench aria-hidden /> Reparar en el origen…</Link>
            <Link className="btn btn--primary" to={`/reconstruccion/${id}/exportar`}><Download aria-hidden /> Exportar imagen reparada</Link>
          </div>
        )}
      </div>

      <div className="stat-grid">
        <StatTile label="Nodos del directorio" value={`${formatNumber(r.nodesBefore)} → ${formatNumber(r.nodesAfter)}`} hint={saving > 0 ? `${saving} % más compacto` : undefined} />
        <StatTile label="Elementos recuperados" value={formatNumber(r.recoveredCount)} />
        <StatTile label="Elementos perdidos" value={formatNumber(r.lostCount)} hint={r.lostCount ? "Solo se conservaba su nombre" : "Ninguno"} />
        <StatTile label="Datos que se escribirán" value={r.bytesToWrite ? formatBytes(r.bytesToWrite) : "—"} hint="Solo en la imagen nueva" />
      </div>

      <section className="card">
        <div className="card__header card__header--wrap">
          <h3>Cambios ({formatNumber(diff.data?.length ?? 0)})</h3>
          <div className="row row--wrap">
            <button className="badge badge--button" aria-pressed={kind === "all"} onClick={() => setKind("all")}>Todos</button>
            {r.changes.map((c) => (
              <button key={c.change} className="badge--button-wrap" aria-pressed={kind === c.change} onClick={() => setKind(kind === c.change ? "all" : c.change)}>
                <ChangeBadge change={c.change} /> <span className="subtle">{c.count}</span>
              </button>
            ))}
          </div>
        </div>
        {diff.data && diff.data.length === 0 ? (
          <p className="card__body muted">El directorio no necesita cambios: la reconstrucción solo lo compactará.</p>
        ) : (
          <div className="table-wrap diff-list">
            <table className="table">
              <thead><tr><th style={{ width: 210 }}>Cambio</th><th>Elemento</th><th>Detalle</th></tr></thead>
              <tbody>
                {visible.slice(0, 300).map((d) => (
                  <tr key={d.id}>
                    <td><ChangeBadge change={d.change} /></td>
                    <td className="path">{d.newPath ?? d.oldPath ?? "—"}{d.oldPath && d.newPath && d.oldPath !== d.newPath && <div className="subtle">antes: {d.oldPath}</div>}</td>
                    <td className="muted">{d.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {visible.length > 300 && <p className="card__body subtle">Se muestran 300 de {formatNumber(visible.length)} cambios. El informe los incluye todos.</p>}
          </div>
        )}
      </section>

      <section className="stack">
        <div className="row">
          <h2 className="section-title grow">Directorio antes y después</h2>
          <button className="btn btn--ghost btn--sm" aria-pressed={onlyChanges} onClick={() => setOnlyChanges(!onlyChanges)}>
            <Filter aria-hidden /> {onlyChanges ? "Mostrando solo cambios" : "Solo cambios"}
          </button>
        </div>
        <div className="split">
          <div className="card">
            <div className="card__header"><h3>Original</h3><span className="subtle">tal como está en la imagen</span></div>
            <div className="tree-pane"><DirectoryTree side="original" id={r.sessionId} onlyChanges={onlyChanges} /></div>
          </div>
          <div className="card">
            <div className="card__header"><h3>Reconstruido</h3><span className="subtle">tras aplicar los cambios</span></div>
            <div className="tree-pane"><DirectoryTree side="rebuilt" id={id} onlyChanges={onlyChanges} /></div>
          </div>
        </div>
      </section>
    </>
  );
}
