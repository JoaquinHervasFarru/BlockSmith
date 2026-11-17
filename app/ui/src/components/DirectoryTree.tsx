import { ChevronRight, File, Folder, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import clsx from "clsx";
import { backend } from "../api";
import { formatBytes, formatNumber } from "../lib/format";
import type { TreeNodeDto } from "../types/api";
import { ChangeBadge } from "./ChangeBadge";

type Side = "original" | "rebuilt";

/** Árbol de directorio con carga perezosa por carpeta. */
export function DirectoryTree({ side, id, onlyChanges = false }: { side: Side; id: number; onlyChanges?: boolean }) {
  const [root, setRoot] = useState<TreeNodeDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setRoot(null);
    backend.treeChildren(side, id, null).then(setRoot).catch((e) => setError(String(e)));
  }, [side, id]);
  if (error) return <p className="subtle">{error}</p>;
  if (!root) return <p className="subtle">Cargando árbol…</p>;
  return (
    <ul className="tree" role="tree">
      {root.map((n) => <TreeItem key={n.id} node={n} side={side} id={id} depth={0} startOpen onlyChanges={onlyChanges} />)}
    </ul>
  );
}

function TreeItem({ node, side, id, depth, startOpen = false, onlyChanges }: {
  node: TreeNodeDto; side: Side; id: number; depth: number; startOpen?: boolean; onlyChanges: boolean;
}) {
  const [open, setOpen] = useState(startOpen);
  const [children, setChildren] = useState<TreeNodeDto[] | null>(null);
  useEffect(() => {
    if (open && node.isFolder && children === null) {
      backend.treeChildren(side, id, node.id).then(setChildren).catch(() => setChildren([]));
    }
  }, [open, node, side, id, children]);
  const expandable = node.isFolder && node.childCount > 0;
  const visible = (children ?? []).filter((c) => !onlyChanges || c.change || c.isFolder);
  const Icon = node.isFolder ? (open ? FolderOpen : Folder) : File;
  return (
    <li role="treeitem" aria-expanded={expandable ? open : undefined}>
      <div className={clsx("tree__row", node.change && "tree__row--changed")} style={{ paddingLeft: 8 + depth * 18 }}>
        {expandable ? (
          <button className="tree__toggle" onClick={() => setOpen(!open)} aria-label={open ? "Contraer" : "Expandir"}>
            <ChevronRight className={clsx(open && "is-open")} aria-hidden />
          </button>
        ) : (
          <span className="tree__toggle" />
        )}
        <Icon className={node.isFolder ? "tree__icon tree__icon--folder" : "tree__icon"} aria-hidden />
        <span className="tree__name truncate" title={node.name}>{node.name}</span>
        {node.change && <ChangeBadge change={node.change} compact />}
        <span className="tree__meta">
          {node.isFolder ? `${formatNumber(node.childCount)} elem.` : formatBytes(node.sizeBytes)}
        </span>
      </div>
      {open && expandable && (
        <ul role="group">
          {children === null && <li className="subtle" style={{ paddingLeft: 30 + depth * 18 }}>Cargando…</li>}
          {visible.map((c) => <TreeItem key={c.id} node={c} side={side} id={id} depth={depth + 1} onlyChanges={onlyChanges} />)}
        </ul>
      )}
    </li>
  );
}
