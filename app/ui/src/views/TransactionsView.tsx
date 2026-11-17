import { CircleCheck, FastForward, Undo2 } from "lucide-react";
import { useState } from "react";
import { Link } from "react-router-dom";
import { backend } from "../api";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState, ErrorCallout, Loading, ProgressBar } from "../components/Feedback";
import { formatDate } from "../lib/format";
import { txStatusLabel } from "../lib/labels";
import { useAsync } from "../lib/useAsync";
import { useStartJob } from "../lib/useStartJob";
import type { TxDto } from "../types/api";

const tone: Record<string, string> = {
  verified: "badge--ok", finalized: "badge--warn", rolled_back: "", interrupted: "badge--crit", failed: "badge--crit",
};

export function TransactionsView() {
  const txs = useAsync(() => backend.listTransactions(), []);
  const job = useStartJob();
  const list = txs.data ?? [];
  const interrupted = list.filter((t) => t.status === "interrupted");
  const rest = list.filter((t) => t.status !== "interrupted");

  const [pending, setPending] = useState<{ tx: TxDto; action: "rollback" | "complete" } | null>(null);
  const rollback = (tx: TxDto) => setPending({ tx, action: "rollback" });
  const complete = (tx: TxDto) => setPending({ tx, action: "complete" });
  const run = () => {
    if (!pending) return;
    const { tx, action } = pending;
    setPending(null);
    void job.start(action, () => (action === "rollback" ? backend.startTxRollback(tx.id, false) : backend.startTxComplete(tx.id)));
  };

  return (
    <>
      {pending && (
        <ConfirmDialog
          title={pending.action === "complete" ? "¿Completar la transacción?" : pending.tx.status === "interrupted" ? "¿Revertir la transacción?" : "¿Deshacer la reparación?"}
          confirmLabel={pending.action === "complete" ? "Completar" : pending.tx.status === "interrupted" ? "Revertir" : "Deshacer"}
          tone={pending.action === "complete" ? "write" : "primary"}
          onConfirm={run}
          onCancel={() => setPending(null)}
        >
          {pending.action === "complete"
            ? `Se escribirán los bloques que faltan en «${pending.tx.volumeName}» y se verificará el volumen.`
            : pending.tx.status === "interrupted"
              ? `Se restaurarán los ${pending.tx.blocksWritten} bloques escritos y «${pending.tx.volumeName}» volverá a su estado anterior.`
              : `«${pending.tx.volumeName}» volverá exactamente al estado anterior a la reparación. Si el volumen se ha modificado desde entonces, BlockSmith se negará a hacerlo.`}
        </ConfirmDialog>
      )}
      <div className="page-header"><div><h1>Transacciones de escritura</h1><p>Reparaciones aplicadas directamente sobre imágenes o discos, con su copia de deshacer.</p></div></div>
      {job.error && <ErrorCallout title="No se pudo iniciar" message={job.error} />}
      {txs.loading && !txs.data ? <Loading /> : txs.error ? <ErrorCallout message={txs.error} /> : list.length === 0 ? (
        <EmptyState icon={<Undo2 aria-hidden />} title="Todavía no hay escrituras directas">Cuando repares un volumen en su origen, aparecerá aquí con su copia de deshacer.</EmptyState>
      ) : (
        <>
          {interrupted.map((t) => (
            <section key={t.id} className="card card--pad stack tx-interrupted">
              <div className="row row--wrap">
                <span className="badge badge--crit">Interrumpida</span>
                <strong className="grow">{t.volumeName ?? "Volumen"} · <span className="mono">{t.devicePath}</span></strong>
                <span className="subtle">{formatDate(t.startedAt)}</span>
              </div>
              <p>{(t.error ?? "La escritura no terminó").replace(/\.?$/, ".").replace(/^./, (c) => c.toUpperCase())} Se escribieron {t.blocksWritten} de {t.blocksTotal} bloques.</p>
              <ProgressBar value={t.blocksTotal ? t.blocksWritten / t.blocksTotal : 0} label="Bloques escritos" />
              <p className="muted">
                <strong>Revertir</strong> devuelve el volumen exactamente a como estaba antes (recomendado si no estás seguro).
                {" "}<strong>Completar</strong> termina de aplicar la reparación y la verifica.
              </p>
              {!t.undoExists && <ErrorCallout title="Falta la copia de deshacer" message={`No se encuentra ${t.undoLogPath}. Sin ella no se puede revertir ni completar.`} />}
              <div className="row row--wrap">
                <button className="btn btn--primary" disabled={!t.canRollback || job.starting} onClick={() => rollback(t)}><Undo2 aria-hidden /> Revertir</button>
                <button className="btn btn--write" disabled={!t.canComplete || job.starting} onClick={() => complete(t)}><FastForward aria-hidden /> Completar</button>
              </div>
            </section>
          ))}
          {rest.length > 0 && (
            <div className="card table-wrap">
              <table className="table">
                <thead><tr><th>Fecha</th><th>Volumen</th><th>Estado</th><th>Modo</th><th /></tr></thead>
                <tbody>
                  {rest.map((t) => (
                    <tr key={t.id}>
                      <td className="nowrap">{formatDate(t.startedAt)}</td>
                      <td><Link to={`/reconstruccion/${t.rebuildId}`}><strong>{t.volumeName ?? "Volumen"}</strong></Link><div className="path">{t.devicePath}</div></td>
                      <td>
                        <span className={"badge " + (tone[t.status] ?? "")}>{t.status === "verified" && <CircleCheck aria-hidden />}{txStatusLabel[t.status] ?? t.status}</span>
                        {t.error && <div className="subtle">{t.error}</div>}
                      </td>
                      <td>{t.mode === "helper" ? "Con permisos de administrador" : "En la app"}</td>
                      <td>{t.canRollback && t.status !== "rolled_back" && (
                        <button className="btn btn--ghost btn--sm" disabled={job.starting} onClick={() => rollback(t)}><Undo2 aria-hidden /> Deshacer</button>
                      )}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </>
  );
}
