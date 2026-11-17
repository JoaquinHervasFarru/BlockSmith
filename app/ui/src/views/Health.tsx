import { Thermometer } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { api, type DiskOverview } from "../api";
import { HealthBadge } from "../components/Badges";
import { busLabel } from "../lib/labels";
import { formatNumber } from "../lib/format";
import type { SmartSnapshot } from "../types/models";

export function Health() {
  const [items, setItems] = useState<DiskOverview[]>([]);
  const [history, setHistory] = useState<SmartSnapshot[]>([]);

  useEffect(() => {
    api.overview().then((all) => setItems(all.filter((i) => i.disk)));
    api.smartHistory(3).then(setHistory);
  }, []);

  const series = useMemo(
    () => history.map((s) => ({
      day: new Date(s.takenAt).toLocaleDateString("es-ES", { day: "2-digit", month: "short" }),
      realloc: s.attributes.find((a) => a.attrId === 5)?.rawValue ?? 0,
      pending: s.attributes.find((a) => a.attrId === 197)?.rawValue ?? 0,
    })),
    [history],
  );

  return (
    <>
      <div className="page-header">
        <div>
          <h1>Salud de los discos</h1>
          <p>Lecturas S.M.A.R.T. y NVMe del agente en segundo plano. Si una caja USB no transmite datos se indica como «No disponible».</p>
        </div>
      </div>

      <div className="table-wrap card">
        <table className="table">
          <thead>
            <tr><th>Disco</th><th>Bus</th><th>Estado</th><th className="num">Temperatura</th><th className="num">Horas de uso</th><th className="num">Desgaste</th><th className="num">Alertas</th></tr>
          </thead>
          <tbody>
            {items.map(({ disk, latestSmart: s, openAlerts }) => (
              <tr key={disk!.id}>
                <td><strong>{disk!.model}</strong><div className="subtle mono">{disk!.serial ?? "Número de serie no disponible"}</div></td>
                <td>{busLabel[disk!.bus]}</td>
                <td><HealthBadge overall={disk!.smartSupported ? s?.overall ?? "unknown" : "unknown"} /></td>
                <td className="num">{s?.temperatureC != null ? <span className="row" style={{ justifyContent: "flex-end" }}><Thermometer size={14} aria-hidden />{s.temperatureC} °C</span> : "—"}</td>
                <td className="num">{s?.powerOnHours != null ? formatNumber(s.powerOnHours) : "—"}</td>
                <td className="num">{s?.percentageUsed != null ? `${s.percentageUsed} %` : "—"}</td>
                <td className="num">{openAlerts || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <section className="card">
        <div className="card__header">
          <div>
            <h3>WDC WD40EFRX · últimos 30 días</h3>
            <p className="subtle">Sectores reasignados (ID 5) y pendientes (ID 197)</p>
          </div>
          <HealthBadge overall="warning" />
        </div>
        <div className="card__body" style={{ height: 260 }}>
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={series} margin={{ top: 8, right: 16, bottom: 0, left: -16 }}>
              <CartesianGrid stroke="var(--border)" vertical={false} />
              <XAxis dataKey="day" tick={{ fill: "var(--text-3)", fontSize: 11 }} tickLine={false} axisLine={false} interval={4} />
              <YAxis tick={{ fill: "var(--text-3)", fontSize: 11 }} tickLine={false} axisLine={false} allowDecimals={false} />
              <Tooltip contentStyle={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 8, fontSize: 12 }} />
              <Line type="monotone" dataKey="realloc" name="Reasignados" stroke="var(--warn)" strokeWidth={2} dot={false} />
              <Line type="monotone" dataKey="pending" name="Pendientes" stroke="var(--crit)" strokeWidth={2} dot={false} strokeDasharray="4 3" />
            </LineChart>
          </ResponsiveContainer>
        </div>
      </section>
    </>
  );
}
