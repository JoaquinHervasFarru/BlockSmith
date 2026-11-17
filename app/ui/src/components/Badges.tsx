import { CircleCheck, CircleHelp, FlaskConical, Lock, LockOpen, ShieldAlert, TriangleAlert } from "lucide-react";
import clsx from "clsx";
import type { Capabilities, EncryptionLayer, FsType, Severity, SmartOverall, Support } from "../types/models";
import { fsLabel, schemeLabel, severityLabel, smartLabel } from "../lib/labels";

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

export function FsBadge({ fs }: { fs: FsType }) {
  return <span className="badge badge--outline badge--mono">{fsLabel[fs]}</span>;
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

export function CapabilityChips({ caps }: { caps: Capabilities }) {
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
