import { Check } from "lucide-react";
import clsx from "clsx";

/** Lista de fases con la actual resaltada y las anteriores completadas. */
export function PhaseStepper({ phases, current, finished }: { phases: string[]; current: string; finished: boolean }) {
  const idx = phases.indexOf(current);
  return (
    <ol className="stepper">
      {phases.map((p, i) => {
        const done = finished || (idx >= 0 && i < idx);
        const active = !finished && i === idx;
        return (
          <li key={p} className={clsx(done && "is-done", active && "is-active")} aria-current={active ? "step" : undefined}>
            <span className="stepper__dot">{done ? <Check aria-hidden /> : i + 1}</span>
            {p}
          </li>
        );
      })}
    </ol>
  );
}
