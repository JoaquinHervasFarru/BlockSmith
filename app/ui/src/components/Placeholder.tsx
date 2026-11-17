import { Wrench } from "lucide-react";

export function Placeholder({ title, phase, description }: { title: string; phase: string; description: string }) {
  return (
    <>
      <div className="page-header"><div><h1>{title}</h1><p>{description}</p></div></div>
      <div className="card card--pad empty-state">
        <Wrench aria-hidden />
        <p><strong>Vista prevista para la fase {phase}.</strong></p>
        <p className="muted">La estructura, los estilos y los datos simulados ya están listos para construirla.</p>
      </div>
    </>
  );
}
