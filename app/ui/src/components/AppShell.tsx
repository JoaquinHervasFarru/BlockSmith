import {
  Activity, Bell, Database, FileSearch, GitCompareArrows, HardDrive, HardDriveDownload, History,
  KeyRound, LifeBuoy, Monitor, Moon, Settings, Sun, Undo2,
} from "lucide-react";
import type { ReactNode } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { usingMocks } from "../api";
import { useUi, type ThemePref } from "../store/ui";

interface Item { to: string; label: string; icon: ReactNode; count?: number }

const groups: { label: string; items: Item[] }[] = [
  {
    label: "Discos",
    items: [
      { to: "/", label: "Inicio", icon: <HardDrive /> },
      { to: "/resultados", label: "Resultados", icon: <FileSearch /> },
      { to: "/comparacion", label: "Comparación", icon: <GitCompareArrows /> },
      { to: "/recuperacion", label: "Recuperar archivos", icon: <LifeBuoy /> },
    ],
  },
  {
    label: "Salud",
    items: [
      { to: "/salud", label: "Salud de los discos", icon: <Activity /> },
      { to: "/alertas", label: "Alertas", icon: <Bell />, count: 3 },
    ],
  },
  {
    label: "Herramientas",
    items: [
      { to: "/transacciones", label: "Transacciones", icon: <Undo2 />, count: 1 },
      { to: "/cifrado", label: "Cabeceras de cifrado", icon: <KeyRound /> },
      { to: "/arranque", label: "Medio de arranque", icon: <HardDriveDownload /> },
      { to: "/historial", label: "Historial", icon: <History /> },
      { to: "/ajustes", label: "Ajustes", icon: <Settings /> },
    ],
  },
];

const themes: { value: ThemePref; label: string; icon: ReactNode }[] = [
  { value: "system", label: "Sistema", icon: <Monitor /> },
  { value: "light", label: "Claro", icon: <Sun /> },
  { value: "dark", label: "Oscuro", icon: <Moon /> },
];

export function AppShell() {
  const { theme, setTheme } = useUi();
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar__brand">
          <img src="/favicon.svg" alt="" />
          <div>BlockSmith<small>Verificación y reconstrucción de discos</small></div>
        </div>
        {groups.map((g) => (
          <nav key={g.label} aria-label={g.label}>
            <div className="nav-group__label">{g.label}</div>
            {g.items.map((it) => (
              <NavLink key={it.to} to={it.to} end={it.to === "/"} className="nav-link">
                {it.icon}
                {it.label}
                {it.count ? <span className="badge badge--crit count">{it.count}</span> : null}
              </NavLink>
            ))}
          </nav>
        ))}
      </aside>

      <div className="main">
        <header className="topbar">
          {usingMocks && (
            <span className="badge badge--ember" title="La UI usa src/mocks/data.ts">
              <Database aria-hidden /> Datos simulados
            </span>
          )}
          <div className="topbar__spacer" />
          <div className="segmented" role="group" aria-label="Tema">
            {themes.map((t) => (
              <button key={t.value} aria-pressed={theme === t.value} onClick={() => setTheme(t.value)} title={t.label}>
                {t.label}
              </button>
            ))}
          </div>
        </header>
        <main className="content">
          <div className="content__inner">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  );
}
