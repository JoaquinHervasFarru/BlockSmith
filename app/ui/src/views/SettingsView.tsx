import { FolderOpen, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { backend } from "../api";
import { ErrorCallout, Loading } from "../components/Feedback";
import { errorText, useAsync } from "../lib/useAsync";
import { useUi } from "../store/ui";
import type { SettingsDto } from "../types/api";

export function SettingsView() {
  const loaded = useAsync(() => backend.getSettings(), []);
  const info = useAsync(() => backend.appInfo(), []);
  const setTheme = useUi((s) => s.setTheme);
  const [draft, setDraft] = useState<SettingsDto | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (loaded.data) setDraft(loaded.data);
  }, [loaded.data]);
  if (loaded.error) return <ErrorCallout message={loaded.error} />;
  if (!draft) return <Loading />;

  const update = (patch: Partial<SettingsDto>) => {
    setStatus(null);
    setDraft({ ...draft, ...patch });
  };
  const save = async () => {
    setError(null);
    try {
      const s = await backend.setSettings(draft);
      setDraft(s);
      setTheme(s.theme);
      setStatus("Ajustes guardados.");
    } catch (e) {
      setError(errorText(e));
    }
  };
  const pick = async () => {
    const p = await backend.pickFolder(draft.outputDir).catch(() => null);
    if (p) update({ outputDir: p });
  };

  return (
    <>
      <div className="page-header"><div><h1>Ajustes</h1><p>Preferencias de esta instalación.</p></div></div>
      <section className="card card--pad stack form">
        <label className="field">
          <span>Carpeta de salida</span>
          <small className="subtle">Imágenes reparadas e informes se guardan aquí por defecto.</small>
          <div className="row">
            <input className="input grow mono" value={draft.outputDir} onChange={(e) => update({ outputDir: e.target.value })} aria-label="Carpeta de salida" />
            <button className="btn btn--secondary" onClick={pick}><FolderOpen aria-hidden /> Elegir…</button>
          </div>
        </label>
        <div className="field">
          <span>Tema</span>
          <div className="segmented" role="group" aria-label="Tema">
            {([["system", "Sistema"], ["light", "Claro"], ["dark", "Oscuro"]] as const).map(([k, l]) => (
              <button key={k} aria-pressed={draft.theme === k} onClick={() => update({ theme: k })}>{l}</button>
            ))}
          </div>
        </div>
        <label className="field">
          <span>Nivel de registro</span>
          <select className="select" value={draft.logLevel} onChange={(e) => update({ logLevel: e.target.value as SettingsDto["logLevel"] })}>
            <option value="error">Solo errores</option>
            <option value="warn">Avisos</option>
            <option value="info">Información</option>
            <option value="debug">Depuración</option>
          </select>
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.healthCheckOnStart} onChange={(e) => update({ healthCheckOnStart: e.target.checked })} />
          <span>Leer la salud de los discos al abrir la app <small className="subtle">(si la última lectura es más antigua que el intervalo de las reglas)</small></span>
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.exportEncryptedByDefault} disabled />
          <span>Exportar volúmenes cifrados manteniendo el cifrado <small className="subtle">(se aplicará cuando llegue el soporte de cifrado)</small></span>
        </label>
        <div className="row">
          <button className="btn btn--primary" onClick={save}><Save aria-hidden /> Guardar</button>
          {status && <span className="subtle" role="status">{status}</span>}
        </div>
        {error && <ErrorCallout title="No se pudieron guardar los ajustes" message={error} />}
      </section>
      {info.data && <p className="subtle">BlockSmith {info.data.version} · {info.data.platform} · fase {info.data.phase}</p>}
    </>
  );
}
