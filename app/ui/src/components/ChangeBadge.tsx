import { ArrowRightLeft, CircleCheck, Copy, Eye, FolderInput, Hash, Scissors, Trash2 } from "lucide-react";
import clsx from "clsx";
import { changeLabel } from "../lib/labels";
import type { ChangeKey } from "../types/api";

const icon: Record<ChangeKey, typeof CircleCheck> = {
  recovered: CircleCheck, moved_to_recovered: FolderInput, renamed: ArrowRightLeft, valence_fixed: Hash,
  dropped: Trash2, data_truncated: Scissors, data_cloned: Copy, unchanged_flagged: Eye,
};
const tone: Record<ChangeKey, string> = {
  recovered: "badge--ok", moved_to_recovered: "badge--info", renamed: "badge--info", valence_fixed: "",
  dropped: "badge--crit", data_truncated: "badge--crit", data_cloned: "badge--warn", unchanged_flagged: "badge--warn",
};

export function ChangeBadge({ change, compact = false }: { change: ChangeKey; compact?: boolean }) {
  const Icon = icon[change];
  return (
    <span className={clsx("badge", tone[change])} title={changeLabel[change]}>
      <Icon aria-hidden />
      {!compact && changeLabel[change]}
    </span>
  );
}
