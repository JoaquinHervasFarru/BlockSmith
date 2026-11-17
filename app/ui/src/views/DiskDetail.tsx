import { ArrowLeft, CircleCheck, ShieldAlert } from "lucide-react";
import { useMemo } from "react";
import { Link, useParams } from "react-router-dom";
import { CartesianGrid, Legend, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { backend } from "../api";
import { HealthBadge, SeverityBadge } from "../components/Badges";
import { ErrorCallout, Loading } from "../components/Feedback";
import { StatTile } from "../components/StatTile";
import { formatBytes, formatDate, formatNumber } from "../lib/format";
import { busLabel, readStatusLabel } from "../lib/labels";
import { useAsync } from "../lib/useAsync";
import type { AtaAttributeDto, SnapshotDto } from "../types/api";
import { overallOf } from "./Health";

const CRITICAL = new Set([5, 187, 197, 198]);
const counter = (raw: number) => raw % 2 ** 32;

function attrState(a: AtaAttributeDto): "failing" | "warning" | "ok" {
  if (a.threshold > 0 && a.value > 0 && a.value <= a.threshold) return "failing";
  if (CRITICAL.has(a.id) && counter(a.raw) > 0) return "warning";
  return "ok";
}

function Trend({ data, lines, unit }: { data: Record<string, number | string | null>[]; lines: { key: string; name: string; color: string; dashed?: boolean }[]; unit?: string }) {
  return (
    <ResponsiveContainer width="100%" height={220}>
      <LineChart data={data} margin={{ top: 8, right: 16, bottom: 0, left: -16 }}>
        <CartesianGrid stroke="var(--border)" vertical={false} />
        <XAxis dataKey="day" tick={{ fill: "var(--text-3)", fontSize: 11 }} tickLine={false} axisLine={false} minTickGap={28} />
        <YAxis tick={{ fill: "var(--text-3)", fontSize: 11 }} tickLine={false} axisLine={false} allowDecimals={false} unit={unit} />
        <Tooltip contentStyle={{ background: "var(--surface)", border: "1px solid var(--border)", borderRadius: 8, fontSize: 12 }} />
        <Legend wrapperStyle={{ fontSize: 12 }} />
        {lines.map((l) => (
          <Line key={l.key} type="monotone" dataKey={l.key} name={l.name} stroke={l.color} strokeWidth={2} dot={false}
            strokeDasharray={l.dashed ? "4 3" : undefined} isAnimationActive={false} connectNulls />
        ))}
      </LineChart>
    </ResponsiveContainer>
  );
}

export function DiskDetail() {
  const id = Number(useParams().id);
  const detail = useAsync(() => backend.healthDisk(id), [id]);
  const series = useMemo(() => {
    const h = detail.data?.history ?? [];
    // Con menos de dos días de histórico, la fecha sola no distingue las lecturas: se usa la hora.
    const span = h.length > 1 ? Date.parse(h.at(-1)!.takenAt) - Date.parse(h[0]!.takenAt) : 0;
    const fmt: Intl.DateTimeFormatOptions = span < 2 * 86_400_000 ? { hour: "2-digit", minute: "2-digit" } : { day: "2-digit", month: "short" };
    return h.map((s: SnapshotDto) => ({
      day: new Date(s.takenAt).toLocaleString("es-ES", fmt),
      temp: s.temperatureC,
      realloc: s.attributes.find((a) => a.id === 5) ? counter(s.attributes.find((a) => a.id === 5)!.raw) : null,
      pending: s.attributes.find((a) => a.id === 197) ? counter(s.attributes.find((a) => a.id === 197)!.raw) : null,
      used: s.percentageUsed,
      media: s.nvme?.mediaErrors ?? null,
    }));
  }, [detail.data]);
  if (detail.error) return <ErrorCallout message={detail.error} />;
  if (!detail.data) return <Loading />;
  const { disk: d, history, alerts } = detail.data;
  const s = d.latest;
  const overall = overallOf(d);
  const nvme = s?.nvme;

  return (
    <>
      <Link to="/salud" className="back-link"><ArrowLeft aria-hidden /> Salud de los discos</Link>
      <div className="page-header">
        <div>
          <div className="row"><h1>{d.model ?? d.path ?? `Disco ${d.id}`}</h1><HealthBadge overall={overall} /></div>
          <p className="mono">{d.path} · {busLabel[d.bus] ?? d.bus} · {formatBytes(d.capacityBytes)} · {d.serial ?? "sin número de serie"}{d.firmware ? ` · firmware ${d.firmware}` : ""}</p>
        </div>
      </div>

      {d.readStatus !== "ok" && (
        <div className="callout callout--warn">
          <ShieldAlert aria-hidden />
          <div><strong>{readStatusLabel[d.readStatus ?? ""] ?? "Sin leer"}</strong>{d.readNote ?? "Todavía no se ha leído este disco."} El estado se muestra como «No disponible», nunca como sano.</div>
        </div>
      )}
      {s && s.reasons.length > 0 && (
        <div className={"callout " + (overall === "failing" ? "callout--crit" : "callout--warn")}>
          <ShieldAlert aria-hidden />
          <div>
            <strong>{overall === "failing" ? "Este disco puede fallar pronto" : "Este disco requiere atención"}</strong>
            {s.reasons.join(" · ")}.{overall === "failing" && " Copia o clona sus datos antes de cualquier reparación; BlockSmith no escribirá en él."}
          </div>
        </div>
      )}
      {s && s.reasons.length === 0 && (
        <div className="callout callout--info"><CircleCheck aria-hidden /><div><strong>Sin indicios de deterioro</strong>Última lectura {formatDate(s.takenAt)}.</div></div>
      )}

      {s && (
        <div className="stat-grid">
          <StatTile label="Temperatura" value={s.temperatureC != null ? `${s.temperatureC} °C` : "—"} />
          <StatTile label="Horas de uso" value={s.powerOnHours != null ? formatNumber(s.powerOnHours) : "—"} />
          <StatTile label={nvme ? "Vida útil consumida" : "Sectores reasignados"}
            value={nvme ? `${nvme.percentageUsed} %` : formatNumber(series.at(-1)?.realloc ?? 0)} />
          <StatTile label={nvme ? "Errores de integridad" : "Sectores pendientes"}
            value={nvme ? formatNumber(nvme.mediaErrors) : formatNumber(series.at(-1)?.pending ?? 0)} />
        </div>
      )}

      {history.length > 1 && (
        <div className="split">
          <section className="card">
            <div className="card__header"><h3>Temperatura</h3><span className="subtle">{history.length} lecturas</span></div>
            <div className="card__body"><Trend data={series} unit="°" lines={[{ key: "temp", name: "°C", color: "var(--accent)" }]} /></div>
          </section>
          <section className="card">
            <div className="card__header"><h3>{nvme ? "Desgaste y errores" : "Sectores reasignados y pendientes"}</h3></div>
            <div className="card__body">
              {nvme ? (
                <Trend data={series} lines={[{ key: "used", name: "Vida consumida (%)", color: "var(--warn)" }, { key: "media", name: "Errores de integridad", color: "var(--crit)", dashed: true }]} />
              ) : (
                <Trend data={series} lines={[{ key: "realloc", name: "Reasignados", color: "var(--warn)" }, { key: "pending", name: "Pendientes", color: "var(--crit)", dashed: true }]} />
              )}
            </div>
          </section>
        </div>
      )}

      {nvme && (
        <section className="card">
          <div className="card__header"><h3>Registro de salud NVMe</h3></div>
          <dl className="kv kv--grid">
            {([
              ["Aviso crítico", nvme.criticalWarning === 0 ? "Ninguno" : `0x${nvme.criticalWarning.toString(16)}`],
              ["Reserva disponible", `${nvme.availableSpare} % (umbral ${nvme.spareThreshold} %)`],
              ["Vida útil consumida", `${nvme.percentageUsed} %`],
              ["Datos escritos", formatBytes(nvme.dataUnitsWritten * 512_000)],
              ["Datos leídos", formatBytes(nvme.dataUnitsRead * 512_000)],
              ["Ciclos de encendido", formatNumber(nvme.powerCycles)],
              ["Apagados no seguros", formatNumber(nvme.unsafeShutdowns)],
              ["Errores de integridad", formatNumber(nvme.mediaErrors)],
            ] as [string, string][]).map(([k, v]) => (<div key={k}><dt>{k}</dt><dd>{v}</dd></div>))}
          </dl>
        </section>
      )}

      {s && s.attributes.length > 0 && (
        <section className="card">
          <div className="card__header"><h3>Atributos S.M.A.R.T.</h3><span className="subtle">Fuente: {s.source ?? "—"}</span></div>
          <div className="table-wrap">
            <table className="table">
              <thead><tr><th className="num">ID</th><th>Atributo</th><th className="num">Valor</th><th className="num">Peor</th><th className="num">Umbral</th><th className="num">Bruto</th><th>Estado</th></tr></thead>
              <tbody>
                {s.attributes.map((a) => {
                  const st = attrState(a);
                  return (
                    <tr key={a.id} className={st !== "ok" ? `row--${st}` : undefined}>
                      <td className="num mono">{a.id}</td>
                      <td className="mono">{a.name}</td>
                      <td className="num">{a.value}</td>
                      <td className="num">{a.worst}</td>
                      <td className="num">{a.threshold || "—"}</td>
                      <td className="num mono">{formatNumber(counter(a.raw))}</td>
                      <td>{st === "failing" ? <span className="badge badge--crit">Bajo el umbral</span> : st === "warning" ? <span className="badge badge--warn">Vigilar</span> : <span className="subtle">Correcto</span>}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>
      )}

      <section className="card">
        <div className="card__header"><h3>Alertas de este disco</h3><Link to="/alertas" className="subtle">Gestionar alertas</Link></div>
        {alerts.length === 0 ? <p className="card__body muted">Sin alertas.</p> : (
          <ul className="list">
            {alerts.map((a) => (
              <li key={a.id} className="list__row">
                <SeverityBadge severity={a.severity} />
                <span className="grow"><strong>{a.ruleLabel}</strong> · {a.message}<div className="subtle">{formatDate(a.createdAt)}{a.acknowledgedAt ? " · confirmada" : ""}</div></span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
