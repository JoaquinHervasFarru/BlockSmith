import { Bell, Check, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { backend } from "../api";
import { SeverityBadge } from "../components/Badges";
import { EmptyState, ErrorCallout, Loading } from "../components/Feedback";
import { formatDate } from "../lib/format";
import { ruleLabel } from "../lib/labels";
import { errorText, useAsync } from "../lib/useAsync";
import type { RuleDto } from "../types/api";

export function AlertsView() {
  const alerts = useAsync(() => backend.listAlerts(false), []);
  const rules = useAsync(() => backend.getAlertRules(), []);
  const [draft, setDraft] = useState<RuleDto[]>([]);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (rules.data) setDraft(rules.data);
  }, [rules.data]);

  const ack = async (id: number) => {
    setError(null);
    try {
      await backend.ackAlert(id);
      alerts.reload();
      window.dispatchEvent(new Event("blocksmith:alerts-changed"));
    } catch (e) {
      setError(errorText(e));
    }
  };
  const save = async () => {
    setError(null);
    setSaved(null);
    try {
      let latest = draft;
      for (const r of draft) {
        const orig = rules.data?.find((x) => x.id === r.id);
        if (JSON.stringify(orig) !== JSON.stringify(r)) latest = await backend.setAlertRule(r);
      }
      setDraft(latest);
      rules.reload();
      setSaved("Reglas guardadas. El agente las aplicará en su próxima lectura.");
    } catch (e) {
      setError(errorText(e));
    }
  };
  const update = (id: number, patch: Partial<RuleDto>) => setDraft(draft.map((r) => (r.id === id ? { ...r, ...patch } : r)));

  const open = (alerts.data ?? []).filter((a) => !a.acknowledgedAt);
  const closed = (alerts.data ?? []).filter((a) => a.acknowledgedAt);

  return (
    <>
      <div className="page-header"><div><h1>Alertas y reglas</h1><p>Avisos generados por las lecturas de salud y las reglas que los disparan.</p></div></div>
      {error && <ErrorCallout message={error} />}

      <section className="stack">
        <h2 className="section-title">Abiertas ({open.length})</h2>
        {alerts.loading && !alerts.data ? <Loading /> : open.length === 0 ? (
          <EmptyState icon={<Bell aria-hidden />} title="No hay alertas abiertas">Las lecturas recientes no han disparado ninguna regla.</EmptyState>
        ) : (
          <div className="card">
            <ul className="list">
              {open.map((a) => (
                <li key={a.id} className="list__row">
                  <SeverityBadge severity={a.severity} />
                  <span className="grow">
                    <strong>{a.ruleLabel}</strong> · <Link to={`/disco/${a.diskId}`}>{a.diskModel ?? a.diskPath}</Link>
                    <div>{a.message}</div>
                    <div className="subtle">{formatDate(a.createdAt)}</div>
                  </span>
                  <button className="btn btn--secondary btn--sm" onClick={() => ack(a.id)}><Check aria-hidden /> Confirmar</button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {closed.length > 0 && (
          <details className="card">
            <summary className="card__header summary">Confirmadas ({closed.length})</summary>
            <ul className="list">
              {closed.map((a) => (
                <li key={a.id} className="list__row muted">
                  <SeverityBadge severity={a.severity} />
                  <span className="grow">{a.ruleLabel} · {a.diskModel ?? a.diskPath} · {a.message}<div className="subtle">confirmada {formatDate(a.acknowledgedAt!)}</div></span>
                </li>
              ))}
            </ul>
          </details>
        )}
      </section>

      <section className="card">
        <div className="card__header"><h3>Reglas</h3><button className="btn btn--primary btn--sm" onClick={save}><Save aria-hidden /> Guardar reglas</button></div>
        {saved && <p className="card__body subtle" role="status">{saved}</p>}
        {!rules.data ? <Loading /> : (
          <div className="table-wrap">
            <table className="table rules">
              <thead><tr><th>Activa</th><th>Regla</th><th>Umbral</th><th>Comprobar cada</th></tr></thead>
              <tbody>
                {draft.map((r) => {
                  const meta = ruleLabel[r.rule] ?? { title: r.rule, help: "", fields: [] };
                  return (
                    <tr key={r.id}>
                      <td><input type="checkbox" checked={r.enabled} onChange={(e) => update(r.id, { enabled: e.target.checked })} aria-label={`Activar ${meta.title}`} /></td>
                      <td><strong>{meta.title}</strong><div className="subtle">{meta.help}</div></td>
                      <td>
                        {meta.fields.length === 0 ? <span className="subtle">—</span> : meta.fields.map((f) => (
                          <label key={f.key} className="row inline-field">
                            <span className="subtle">{f.label}</span>
                            <input className="input input--num" type="number" min={0} value={r.threshold[f.key] ?? 0}
                              onChange={(e) => update(r.id, { threshold: { ...r.threshold, [f.key]: Number(e.target.value) } })} aria-label={`${meta.title}: ${f.label}`} />
                            <span className="subtle">{f.unit}</span>
                          </label>
                        ))}
                      </td>
                      <td>
                        <label className="row inline-field">
                          <input className="input input--num" type="number" min={15} max={10080} value={r.pollIntervalMin}
                            onChange={(e) => update(r.id, { pollIntervalMin: Number(e.target.value) })} aria-label={`${meta.title}: intervalo`} />
                          <span className="subtle">min</span>
                        </label>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <p className="card__body subtle">El agente lee los discos con el intervalo más corto de las reglas activas (mínimo 15 minutos).</p>
      </section>
    </>
  );
}
