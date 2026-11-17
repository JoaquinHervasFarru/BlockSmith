import { createHashRouter, RouterProvider } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { Placeholder } from "./components/Placeholder";
import { ExportView } from "./views/ExportView";
import { HistoryView } from "./views/HistoryView";
import { Home } from "./views/Home";
import { Job } from "./views/Job";
import { RebuildCompare } from "./views/RebuildCompare";
import { SessionResults } from "./views/SessionResults";
import { SettingsView } from "./views/SettingsView";
import { VolumeDetail } from "./views/VolumeDetail";

// La vista de salud carga recharts bajo demanda.
// HashRouter: funciona igual en el navegador y en el protocolo personalizado de Tauri.
const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Home /> },
      { path: "volumen/:id", element: <VolumeDetail /> },
      { path: "trabajo/:id", element: <Job /> },
      { path: "sesion/:id", element: <SessionResults /> },
      { path: "reconstruccion/:id", element: <RebuildCompare /> },
      { path: "reconstruccion/:id/exportar", element: <ExportView /> },
      { path: "historial", element: <HistoryView /> },
      { path: "ajustes", element: <SettingsView /> },
      { path: "salud", lazy: async () => ({ Component: (await import("./views/Health")).Health }) },
      { path: "alertas", element: <Placeholder title="Alertas y reglas" phase="F4" description="Historial de alertas y umbrales del agente." /> },
      { path: "recuperacion", element: <Placeholder title="Recuperar archivos" phase="F8" description="Copia de lo recuperable a otro disco; selector de checkpoint en APFS." /> },
      { path: "transacciones", element: <Placeholder title="Transacciones de escritura" phase="F5" description="Revertir o reverificar escrituras directas interrumpidas." /> },
      { path: "cifrado", element: <Placeholder title="Cabeceras de cifrado" phase="F6b" description="Estado de las copias de cabecera de BitLocker, LUKS y Core Storage." /> },
      { path: "arranque", element: <Placeholder title="Medio de arranque" phase="F9" description="Crear un USB BlockSmith Live o preparar un disco externo con macOS." /> },
      { path: "*", element: <Placeholder title="Página no encontrada" phase="—" description="Esta ruta no existe." /> },
    ],
  },
]);

export default function App() {
  return <RouterProvider router={router} />;
}
