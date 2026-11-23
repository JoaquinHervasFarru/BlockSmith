#!/usr/bin/env python3
"""Inventario de licencias de terceros.

    python3 scripts/licenses.py           # regenera THIRD_PARTY_NOTICES.md
    python3 scripts/licenses.py --check   # falla si hay una licencia no admitida
                                          # o si el archivo no está al día

Fuentes: `cargo metadata --locked` (todas las plataformas) y app/package-lock.json.
Solo usa la biblioteca estándar.
"""
from __future__ import annotations

import collections
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_NOTICES.md"

# Licencias compatibles con distribuir BlockSmith bajo PolyForm Noncommercial
# (y bajo licencias comerciales aparte). MPL-2.0 es copyleft por archivo: se admite
# mientras esos archivos no se modifiquen. LGPL/GPL/AGPL no se admiten.
ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
    "BSD-3-Clause", "ISC", "0BSD", "Zlib", "Unicode-3.0", "Unlicense", "CC0-1.0",
    "MPL-2.0", "OFL-1.1", "BSL-1.0",
}


def satisfiable(expr: str) -> bool:
    """¿Se puede elegir una rama de la expresión SPDX cuyas licencias estén todas admitidas?"""
    e = expr.replace("/", " OR ").replace("(", " ").replace(")", " ")
    # Las expresiones reales son sumas de productos sin anidar ORs dentro de ANDs,
    # salvo "(A OR B) AND C"; ese caso se trata como producto de alternativas.
    if " AND " in e and " OR " in expr.split(" AND ")[0]:
        return all(satisfiable(part) for part in re.split(r"\s+AND\s+", expr.replace("(", "").replace(")", "")))
    for branch in re.split(r"\s+OR\s+", e.strip()):
        parts = [p.strip() for p in re.split(r"\s+AND\s+", branch.strip()) if p.strip()]
        if parts and all(p in ALLOWED for p in parts):
            return True
    return False


def rust_packages() -> list[tuple[str, str, str]]:
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    used = {n["id"] for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    out = []
    for p in meta["packages"]:
        if p["id"] in used and p["id"] not in members:
            out.append((p["name"], p["version"], p.get("license") or "UNKNOWN"))
    return sorted(set(out))


def npm_packages() -> list[tuple[str, str, str, bool]]:
    lock = json.loads((ROOT / "app" / "package-lock.json").read_text())
    out = []
    for path, p in lock["packages"].items():
        if "node_modules/" not in path or p.get("link"):
            continue
        name = path.rsplit("node_modules/", 1)[1]
        if name.startswith("@blocksmith/"):
            continue
        lic = p.get("license") or "UNKNOWN"
        out.append((name, p.get("version", "?"), lic, bool(p.get("dev"))))
    return sorted(set(out))


def render(rust, npm) -> str:
    lines = [
        "# Avisos de terceros",
        "",
        "<!-- Generado por scripts/licenses.py: no editar a mano. -->",
        "",
        "BlockSmith (ver `LICENSE` y `NOTICE`) incluye o enlaza componentes de terceros, cada uno",
        "bajo su propia licencia. El código fuente de cada componente está disponible en",
        "<https://crates.io/crates/NOMBRE> (Rust) o <https://www.npmjs.com/package/NOMBRE> (npm),",
        "en la versión indicada. Ninguno de estos componentes se ha modificado.",
        "",
        "- Los componentes bajo **MPL-2.0** se usan sin modificar; su código fuente está en los",
        "  enlaces anteriores, conforme a la sección 3.2 de esa licencia.",
        "- Las fuentes tipográficas **Inter** y **JetBrains Mono** se distribuyen bajo la",
        "  SIL Open Font License 1.1, cuyo texto completo está en `docs/licenses/OFL-1.1.txt`.",
        "- Los paquetes npm marcados como *(solo compilación)* no se incluyen en la aplicación.",
        "",
        f"## Rust ({len(rust)} paquetes)",
        "",
    ]
    by = collections.defaultdict(list)
    for name, ver, lic in rust:
        by[lic].append(f"{name} {ver}")
    for lic in sorted(by, key=lambda k: (-len(by[k]), k)):
        lines += [f"### {lic} ({len(by[lic])})", "", ", ".join(by[lic]), ""]
    lines += [f"## JavaScript ({len(npm)} paquetes)", ""]
    by = collections.defaultdict(list)
    for name, ver, lic, dev in npm:
        by[lic].append(f"{name} {ver}" + (" *(solo compilación)*" if dev else ""))
    for lic in sorted(by, key=lambda k: (-len(by[k]), k)):
        lines += [f"### {lic} ({len(by[lic])})", "", ", ".join(by[lic]), ""]
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    rust, npm = rust_packages(), npm_packages()
    bad = [f"rust {n} {v}: {l}" for n, v, l in rust if not satisfiable(l)]
    bad += [f"npm {n} {v}: {l}" for n, v, l, _ in npm if not satisfiable(l)]
    if bad:
        print("Licencias no admitidas:\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    text = render(rust, npm)
    if check:
        if not OUT.exists() or OUT.read_text() != text:
            print("THIRD_PARTY_NOTICES.md no está al día: ejecuta python3 scripts/licenses.py",
                  file=sys.stderr)
            return 1
        print(f"Licencias OK: {len(rust)} paquetes Rust, {len(npm)} npm.")
        return 0
    OUT.write_text(text)
    print(f"Escrito {OUT.relative_to(ROOT)}: {len(rust)} Rust, {len(npm)} npm.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
