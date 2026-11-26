#!/usr/bin/env python3
"""Diagnóstico: compara el Volume Header y el bitmap de asignación de dos volúmenes HFS+.

    hfs_bitmap_diff.py <volumen_a> <tamaño_a> <volumen_b> <tamaño_b>

Cada volumen es un dispositivo o archivo que empieza en el propio volumen (p. ej.
/dev/rdisk4s1) y su tamaño en bytes. Muestra las diferencias de los campos principales
de la cabecera (principal y copias alternativas) y los tramos de bloques cuyo estado
en el bitmap cambia de A a B, con los primeros bytes de cada tramo en A, para saber
qué contienen. Solo lee. Las lecturas van alineadas a 512 bytes (dispositivos raw).
"""
from __future__ import annotations

import struct
import sys

FIELDS = [
    ("signature", 0, ">2s"), ("version", 2, ">H"), ("attributes", 4, ">I"),
    ("lastMountedVersion", 8, ">4s"), ("journalInfoBlock", 12, ">I"),
    ("fileCount", 32, ">I"), ("folderCount", 36, ">I"), ("blockSize", 40, ">I"),
    ("totalBlocks", 44, ">I"), ("freeBlocks", 48, ">I"), ("nextAllocation", 52, ">I"),
    ("nextCatalogID", 64, ">I"), ("writeCount", 68, ">I"),
]
FORKS = [("allocationFile", 112), ("extentsFile", 192), ("catalogFile", 272),
         ("attributesFile", 352), ("startupFile", 432)]


def read(f, off: int, n: int) -> bytes:
    start = off - off % 512
    end = -(-(off + n) // 512) * 512
    f.seek(start)
    data = f.read(end - start)
    return data[off - start:off - start + n]


def header(b: bytes) -> dict:
    out = {name: struct.unpack_from(fmt, b, o)[0] for name, o, fmt in FIELDS}
    for name, o in FORKS:
        logical, clump, total = struct.unpack_from(">QII", b, o)
        exts = [struct.unpack_from(">II", b, o + 16 + 8 * i) for i in range(8)]
        out[name] = (logical, total, [e for e in exts if e[1]])
    return out


def bitmap(f, vh: dict) -> bytes:
    bs, total = vh["blockSize"], vh["totalBlocks"]
    need = -(-total // 8)
    data = b""
    for start, count in vh["allocationFile"][2]:
        data += read(f, start * bs, count * bs)
    return data[:need]


def ranges(bits: list[int]) -> list[tuple[int, int]]:
    out = []
    for b in bits:
        if out and out[-1][1] == b:
            out[-1] = (out[-1][0], b + 1)
        else:
            out.append((b, b + 1))
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    fa, fb = open(argv[0], "rb"), open(argv[2], "rb")
    size_a, size_b = int(argv[1]), int(argv[3])
    va, vb = header(read(fa, 1024, 512)), header(read(fb, 1024, 512))
    print("== Volume Header (A → B)")
    for k in va:
        mark = "" if va[k] == vb[k] else "   ← cambia"
        print(f"  {k:20} {va[k]!s:60.60} → {vb[k]!s:60.60}{mark}")
    for label, f, size, vh in (("A", fa, size_a, va), ("B", fb, size_b, vb)):
        for where, off in (("final de la partición", size - 1024),
                           ("final del último bloque", vh["totalBlocks"] * vh["blockSize"] - 1024)):
            if 0 < off <= size - 512:
                alt = read(f, off, 512)
                ok = alt[:2] in (b"H+", b"HX")
                same = ok and header(alt) == vh
                print(f"  alternativa de {label} en {where} ({off}): "
                      f"{'presente' if ok else 'ausente'}{', igual a la principal' if same else ''}")
    if (va["blockSize"], va["totalBlocks"]) != (vb["blockSize"], vb["totalBlocks"]):
        print("Los volúmenes tienen geometría distinta; no se comparan los bitmaps.")
        return 1
    ba, bb = bitmap(fa, va), bitmap(fb, vb)
    total, bs = va["totalBlocks"], va["blockSize"]
    bit = lambda m, i: (m[i // 8] >> (7 - i % 8)) & 1
    freed = [i for i in range(total) if bit(ba, i) and not bit(bb, i)]
    taken = [i for i in range(total) if not bit(ba, i) and bit(bb, i)]
    print(f"== Bitmap: {len(freed)} bloques pasan de usados a libres, {len(taken)} de libres a usados")
    for title, blocks in (("usados en A y libres en B", freed), ("libres en A y usados en B", taken)):
        rs = ranges(blocks)
        print(f"  {title}: {len(rs)} tramos")
        for s, e in rs[:25]:
            head = read(fa, s * bs, 32)
            print(f"    {s}–{e - 1} ({e - s} bloques)  A: {head.hex(' ', 4)}  {head[:16]!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
