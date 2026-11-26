#!/usr/bin/env python3
"""Inventario de un volumen montado, para comparar el original con la reconstrucción.

    hfs_manifest.py snapshot <punto_de_montaje> <salida.json>
    hfs_manifest.py compare <a.json> <b.json> [--content-only]

`snapshot` registra, por ruta: tipo, modo, flags (p. ej. UF_COMPRESSED), número de inodo
(el CNID en HFS+), número de enlaces, tamaño, SHA-256 del contenido, destino de los
enlaces simbólicos, fechas de modificación y creación, y el SHA-256 de cada atributo
extendido (incluido com.apple.ResourceFork). En macOS los xattr se leen con libc
mediante ctypes; en otros sistemas, con os.listxattr si existe.

`compare` falla si hay cualquier diferencia. Con --content-only solo comprueba que no
falte ningún contenido de archivo del original (para daños que cambian rutas o nombres).
"""
from __future__ import annotations

import ctypes
import ctypes.util
import hashlib
import json
import os
import stat
import sys

XATTR_NOFOLLOW = 0x0001
_libc = None
if sys.platform == "darwin":
    _libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
    _libc.listxattr.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_size_t, ctypes.c_int]
    _libc.listxattr.restype = ctypes.c_ssize_t
    _libc.getxattr.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_void_p, ctypes.c_size_t,
                               ctypes.c_uint32, ctypes.c_int]
    _libc.getxattr.restype = ctypes.c_ssize_t


def _darwin_xattrs(path: str) -> dict[str, str]:
    p = os.fsencode(path)
    n = _libc.listxattr(p, None, 0, XATTR_NOFOLLOW)
    if n < 0:
        raise OSError(ctypes.get_errno(), "listxattr", path)
    if n == 0:
        return {}
    buf = ctypes.create_string_buffer(n)
    n = _libc.listxattr(p, buf, n, XATTR_NOFOLLOW)
    out = {}
    for name in buf.raw[:n].split(b"\0"):
        if not name:
            continue
        size = _libc.getxattr(p, name, None, 0, 0, XATTR_NOFOLLOW)
        if size < 0:
            raise OSError(ctypes.get_errno(), "getxattr", path)
        val = ctypes.create_string_buffer(max(size, 1))
        got = _libc.getxattr(p, name, val, size, 0, XATTR_NOFOLLOW)
        out[name.decode("utf-8", "surrogateescape")] = hashlib.sha256(val.raw[:got]).hexdigest()
    return out


def xattrs(path: str) -> dict[str, str]:
    if _libc is not None:
        return _darwin_xattrs(path)
    if hasattr(os, "listxattr"):
        return {n: hashlib.sha256(os.getxattr(path, n, follow_symlinks=False)).hexdigest()
                for n in os.listxattr(path, follow_symlinks=False)}
    return {}


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


SKIP = {".fseventsd", ".Spotlight-V100", ".Trashes", ".TemporaryItems", ".DocumentRevisions-V100"}


def snapshot(root: str) -> dict:
    entries = {}
    for dirpath, dirnames, filenames in os.walk(root):
        if dirpath == root:
            dirnames[:] = [d for d in dirnames if d not in SKIP]
            filenames = [f for f in filenames if f not in SKIP]
        for name in dirnames + filenames:
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, root)
            st = os.lstat(full)
            e = {
                "mode": stat.filemode(st.st_mode),
                "flags": getattr(st, "st_flags", 0),
                "ino": st.st_ino,
                "nlink": st.st_nlink if not stat.S_ISDIR(st.st_mode) else None,
                "size": st.st_size if stat.S_ISREG(st.st_mode) else None,
                "mtime": st.st_mtime_ns,
                "birth": getattr(st, "st_birthtime", None),
                "xattrs": xattrs(full),
            }
            if stat.S_ISREG(st.st_mode):
                e["sha256"] = sha256_file(full)
            elif stat.S_ISLNK(st.st_mode):
                e["target"] = os.readlink(full)
            entries[rel] = e
    return entries


def compare(a: dict, b: dict, content_only: bool) -> list[str]:
    if content_only:
        # Ningún contenido de A puede faltar en B (contando repeticiones). B puede tener
        # más: un daño puede dejar visible una carpeta de sistema que estaba oculta.
        from collections import Counter
        ca = Counter(e["sha256"] for e in a.values() if "sha256" in e)
        cb = Counter(e["sha256"] for e in b.values() if "sha256" in e)
        missing = ca - cb
        extra = cb - ca
        if extra:
            paths = [k for k, e in b.items() if e.get("sha256") in extra]
            print(f"  (aviso) {sum(extra.values())} contenidos que no estaban en el original: {paths[:10]}")
        if not missing:
            return []
        paths = [k for k, e in a.items() if e.get("sha256") in missing]
        return [f"faltan {sum(missing.values())} contenidos del original: {paths[:20]}"]
    errs = []
    for k in sorted(set(a) - set(b)):
        errs.append(f"falta: {k}")
    for k in sorted(set(b) - set(a)):
        errs.append(f"sobra: {k}")
    for k in sorted(set(a) & set(b)):
        for field in sorted(set(a[k]) | set(b[k])):
            if a[k].get(field) != b[k].get(field):
                errs.append(f"{k}: {field} {a[k].get(field)!r} → {b[k].get(field)!r}")
    return errs


def main(argv: list[str]) -> int:
    if len(argv) >= 3 and argv[0] == "snapshot":
        data = snapshot(argv[1])
        with open(argv[2], "w") as f:
            json.dump(data, f, indent=1, sort_keys=True)
        files = sum(1 for e in data.values() if "sha256" in e)
        print(f"inventario: {len(data)} entradas, {files} archivos → {argv[2]}")
        return 0
    if len(argv) >= 3 and argv[0] == "compare":
        a, b = (json.load(open(p)) for p in argv[1:3])
        errs = compare(a, b, "--content-only" in argv[3:])
        if errs:
            print(f"✗ {len(errs)} diferencias:")
            for e in errs[:60]:
                print("   ", e)
            return 1
        print(f"✓ idénticos ({len(a)} entradas)")
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
