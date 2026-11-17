import { CircleCheck, CircleHelp, FlaskConical, Lock, LockOpen, ShieldAlert, TriangleAlert } from "lucide-react";
import clsx from "clsx";
import type { EncryptionLayer, Severity, SmartOverall, Support } from "../types/models";
import type { SeverityCounts } from "../types/api";
import { fsName, schemeLabel, severityLabel, smartLabel } from "../lib/labels";

const severityTone: Record<Severity, string> = { critical: "badge--crit", minor: "badge--warn", cosmetic: "" };

/** El estado nunca depende solo del color: siempre hay icono y texto. */
export function SeverityBadge({ severity }: { severity: Severity }) {
  const Icon = severity === "critical" ? ShieldAlert : severity === "minor" ? TriangleAlert : CircleHelp;
  return (
    <span className={clsx("badge", severityTone[severity])}>
      <Icon aria-hidden /> {severityLabel[severity]}
    </span>
  );
}

const healthTone: Record<SmartOverall, string> = {
  passed: "badge--ok", warning: "badge--warn", failing: "badge--crit", unknown: "",
};

export function HealthBadge({ overall }: { overall: SmartOverall }) {
  const Icon = overall === "passed" ? CircleCheck : overall === "unknown" ? CircleHelp : overall === "warning" ? TriangleAlert : ShieldAlert;
  return (
    <span className={clsx("badge", healthTone[overall])}>
      <Icon aria-hidden /> {smartLabel[overall]}
    </span>
  );
}

export function FsBadge({ fs }: { fs: string }) {
  return <span className="badge badge--outline badge--mono">{fsName(fs)}</span>;
}

/** Recuento compacto de hallazgos por severidad; «Sin problemas» si no hay ninguno. */
export function SeverityCountBadges({ counts }: { counts: SeverityCounts }) {
  if (counts.critical + counts.minor + counts.cosmetic === 0) {
    return <span className="badge badge--ok"><CircleCheck aria-hidden /> Sin problemas</span>;
  }
  return (
    <span className="row">
      {counts.critical > 0 && <span className="badge badge--crit"><ShieldAlert aria-hidden /> {counts.critical}</span>}
      {counts.minor > 0 && <span className="badge badge--warn"><TriangleAlert aria-hidden /> {counts.minor}</span>}
      {counts.cosmetic > 0 && <span className="badge"><CircleHelp aria-hidden /> {counts.cosmetic}</span>}
    </span>
  );
}

export function EncryptionBadge({ layer }: { layer: EncryptionLayer | null }) {
  if (!layer) return null;
  if (layer.scheme === "hardware") {
    return <span className="badge" title="Cifrado gestionado por el propio disco o el chip de seguridad"><Lock aria-hidden /> Hardware</span>;
  }
  const Icon = layer.unlocked ? LockOpen : Lock;
  return (
    <span className={clsx("badge", layer.unlocked ? "badge--ok" : "badge--info")}>
      <Icon aria-hidden /> {schemeLabel[layer.scheme]}
    </span>
  );
}

const supportText: Record<Support, string> = { yes: "", experimental: " (experimental)", no: " · no disponible" };

interface CapabilitySet { scan: Support; recover: Support; rebuild: Support; writeInPlace: Support }

export function CapabilityChips({ caps }: { caps: CapabilitySet }) {
  const items: [string, Support][] = [
    ["Escanear", caps.scan], ["Recuperar", caps.recover], ["Reconstruir", caps.rebuild], ["Escritura directa", caps.writeInPlace],
  ];
  return (
    <div className="row row--wrap">
      {items.map(([label, s]) => (
        <span key={label} className={clsx("badge", s === "yes" && "badge--info", s === "experimental" && "badge--ember")} title={label + supportText[s]}>
          {s === "experimental" && <FlaskConical aria-hidden />}
          {label}{s === "no" && " ✕"}
        </span>
      ))}
    </div>
  );
}
