import { createHashRouter, RouterProvider } from "react-router-dom";
import { AppShell } from "./components/AppShell";
import { Placeholder } from "./components/Placeholder";
import { Home } from "./views/Home";
import { Results } from "./views/Results";

// La vista de salud carga recharts bajo demanda.
// HashRouter: funciona igual en el navegador y en el protocolo personalizado de Tauri.
const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Home /> },
      { path: "resultados", element: <Results /> },
      { path: "salud", lazy: async () => ({ Component: (await import("./views/Health")).Health }) },
      { path: "comparacion", element: <Placeholder title="Comparación" phase="F2–F3" description="Árbol original frente al reconstruido." /> },
      { path: "recuperacion", element: <Placeholder title="Recuperar archivos" phase="F8" description="Copia de lo recuperable a otro disco; selector de checkpoint en APFS." /> },
      { path: "alertas", element: <Placeholder title="Alertas y reglas" phase="F4" description="Historial de alertas y umbrales del agente." /> },
      { path: "transacciones", element: <Placeholder title="Transacciones de escritura" phase="F5" description="Revertir o reverificar escrituras directas interrumpidas." /> },
      { path: "cifrado", element: <Placeholder title="Cabeceras de cifrado" phase="F6b" description="Estado de las copias de cabecera de BitLocker, LUKS y Core Storage." /> },
      { path: "arranque", element: <Placeholder title="Medio de arranque" phase="F9" description="Crear un USB BlockSmith Live o preparar un disco externo con macOS." /> },
      { path: "historial", element: <Placeholder title="Historial" phase="F3" description="Sesiones e informes anteriores." /> },
      { path: "ajustes", element: <Placeholder title="Ajustes" phase="F3" description="Carpeta de salida, idioma, registros y tema." /> },
    ],
  },
]);

export default function App() {
  return <RouterProvider router={router} />;
}
