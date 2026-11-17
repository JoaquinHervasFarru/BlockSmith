import { Activity, HardDrive, RefreshCw, ShieldAlert, Thermometer, Usb } from "lucide-react";
import { useEffect } from "react";
import { Link } from "react-router-dom";
import { backend } from "../api";
import { HealthBadge } from "../components/Badges";
import { EmptyState, ErrorCallout, Loading } from "../components/Feedback";
import { StatTile } from "../components/StatTile";
import { formatBytes, formatNumber } from "../lib/format";
import { busLabel, readStatusLabel, timeAgo } from "../lib/labels";
import { useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";
import type { DiskDto, OverallKey } from "../types/api";

export function overallOf(d: DiskDto): OverallKey {
  return d.readStatus === "ok" && d.latest ? d.latest.overall : "unknown";
}

export function Health() {
  const disks = useAsync(() => backend.healthDisks(), []);
  const status = useAsync(() => backend.healthStatus(), []);
  const job = useStartJob();
  useEffect(() => {
    let off: (() => void) | undefined;
    void backend.onHealthUpdated(() => {
      disks.reload();
      status.reload();
    }).then((f) => (off = f));
    return () => off?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const list = disks.data ?? [];
  const failing = list.filter((d) => overallOf(d) === "failing").length;
  const warning = list.filter((d) => overallOf(d) === "warning").length;
  const unread = list.filter((d) => d.readStatus !== "ok").length;
  const poll = status.data?.lastPoll;

  return (
    <>
      <div className="page-header">
        <div>
          <h1>Salud de los discos</h1>
          <p>
            Lecturas S.M.A.R.T. (SATA) y del registro de salud (NVMe).
            {poll ? ` Última lectura ${timeAgo(poll.at)} por ${poll.by === "app" ? "la app" : "el agente"}.` : " Todavía no se ha leído ningún disco."}
          </p>
        </div>
        <button className="btn btn--primary" disabled={job.starting} onClick={() => job.start("health", () => backend.startHealthRead())}>
          <RefreshCw aria-hidden /> Leer ahora
        </button>
      </div>
      {job.error && <ErrorCallout title="No se pudo iniciar la lectura" message={job.error} />}
      {poll?.error && <ErrorCallout title="La última lectura falló" message={poll.error} />}

      <div className="stat-grid">
        <StatTile label="Discos detectados" value={list.length} hint={status.data ? `Lectura con backend ${status.data.backend}` : undefined} />
        <StatTile label="Fallando" value={failing} hint={failing ? "Copia sus datos cuanto antes" : "Ninguno"} />
        <StatTile label="Requieren atención" value={warning} />
        <StatTile label="Alertas abiertas" value={status.data?.openAlerts ?? "—"} hint={<Link to="/alertas">Ver alertas</Link>} />
      </div>

      {unread > 0 && list.some((d) => d.readStatus === "permission") && (
        <div className="callout callout--warn">
          <ShieldAlert aria-hidden />
          <div>
            <strong>Algunos discos no se pudieron leer por permisos</strong>
            Leer S.M.A.R.T. requiere permisos de administrador. Instala el agente de BlockSmith como servicio del sistema
            (<code>blocksmith-agent print-service</code> muestra cómo) y la app mostrará sus lecturas.
          </div>
        </div>
      )}

      {disks.loading && !disks.data ? <Loading /> : disks.error ? <ErrorCallout message={disks.error} /> : list.length === 0 ? (
        <EmptyState icon={<Activity aria-hidden />} title="No hay lecturas todavía">Pulsa «Leer ahora» para detectar los discos y leer su salud.</EmptyState>
      ) : (
        <div className="table-wrap card">
          <table className="table">
            <thead>
              <tr><th>Disco</th><th>Bus</th><th>Estado</th><th className="num">Temperatura</th><th className="num">Horas de uso</th><th className="num">Desgaste</th><th className="num">Alertas</th></tr>
            </thead>
            <tbody>
              {list.map((d) => {
                const s = d.latest;
                const Icon = d.bus === "usb" ? Usb : HardDrive;
                return (
                  <tr key={d.id}>
                    <td>
                      <Link to={`/disco/${d.id}`} className="row disk-link">
                        <Icon aria-hidden />
                        <span>
                          <strong>{d.model ?? d.path ?? `Disco ${d.id}`}</strong>
                          {d.isBootDisk && <span className="badge badge--outline" style={{ marginLeft: 6 }}>Arranque</span>}
                          <div className="subtle mono">{d.path} · {formatBytes(d.capacityBytes)} · {d.serial ?? "sin número de serie"}</div>
                        </span>
                      </Link>
                    </td>
                    <td>{busLabel[d.bus] ?? d.bus}</td>
                    <td>
                      <HealthBadge overall={overallOf(d)} />
                      {d.readStatus !== "ok" && <div className="subtle">{readStatusLabel[d.readStatus ?? ""] ?? "Sin leer"}</div>}
                    </td>
                    <td className="num">{s?.temperatureC != null ? <span className="row" style={{ justifyContent: "flex-end" }}><Thermometer size={14} aria-hidden />{s.temperatureC} °C</span> : "—"}</td>
                    <td className="num">{s?.powerOnHours != null ? formatNumber(s.powerOnHours) : "—"}</td>
                    <td className="num">{s?.percentageUsed != null ? `${s.percentageUsed} %` : "—"}</td>
                    <td className="num">{d.openAlerts || "—"}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}
