import { FileText, History } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { backend } from "../api";
import { FsBadge, SeverityCountBadges } from "../components/Badges";
import { EmptyState, ErrorCallout, Loading } from "../components/Feedback";
import { formatDate } from "../lib/format";
import { sessionStatusLabel } from "../lib/labels";
import { errorText, useAsync } from "../lib/useAsync";

export function HistoryView() {
  const sessions = useAsync(() => backend.listSessions(), []);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const report = async (id: number) => {
    setError(null);
    try {
      setSaved((await backend.writeReport(id, "html")).path);
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <>
      <div className="page-header">
        <div><h1>Historial</h1><p>Todos los análisis y reconstrucciones, guardados en la base de datos local.</p></div>
      </div>
      {saved && <div className="callout callout--info"><FileText aria-hidden /><div><strong>Informe guardado</strong><span className="mono">{saved}</span></div></div>}
      {error && <ErrorCallout message={error} />}
      {sessions.loading && !sessions.data ? <Loading /> : sessions.error ? <ErrorCallout message={sessions.error} /> : (sessions.data ?? []).length === 0 ? (
        <EmptyState icon={<History aria-hidden />} title="Todavía no hay análisis">Abre una imagen y analiza uno de sus volúmenes.</EmptyState>
      ) : (
        <div className="card table-wrap">
          <table className="table">
            <thead><tr><th>Fecha</th><th>Volumen</th><th>Estado</th><th>Hallazgos</th><th>Reconstrucción</th><th /></tr></thead>
            <tbody>
              {sessions.data!.map((s) => (
                <tr key={s.id}>
                  <td className="nowrap">{formatDate(s.startedAt)}</td>
                  <td>
                    <Link to={`/sesion/${s.id}`}><strong>{s.volumeName ?? "Volumen sin nombre"}</strong></Link> <FsBadge fs={s.fsType} />
                    <div className="path">{s.sourcePath}</div>
                  </td>
                  <td>{sessionStatusLabel[s.status] ?? s.status}{s.error && <div className="subtle">{s.error}</div>}</td>
                  <td><SeverityCountBadges counts={s.counts} /></td>
                  <td>{s.latestRebuildId ? <Link to={`/reconstruccion/${s.latestRebuildId}`}>Ver cambios</Link> : <span className="subtle">—</span>}</td>
                  <td><button className="btn btn--ghost btn--sm" onClick={() => report(s.id)}><FileText aria-hidden /> Informe</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
