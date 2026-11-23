#!/bin/bash
# Validación de BlockSmith contra volúmenes HFS+ creados por macOS (hdiutil/newfs_hfs).
#
#   scripts/macos-hfs-validate.sh <blocksmith-cli> <blocksmith-corruptor> [dir_de_trabajo]
#
# Para cada variante (HFS+ / HFS+ con registro / HFSX, con GPT, APM, MBR o sin mapa):
#   1. crea la imagen, la puebla con casos difíciles (miles de archivos, Unicode, nombres
#      largos, enlaces duros y simbólicos, xattrs grandes, resource forks, compresión
#      HFS de ditto, archivos fragmentados y huecos de archivos borrados) y la inventaría;
#   2. escanea con blocksmith-cli: no debe haber hallazgos críticos ni menores;
#   3. reconstruye el directorio en una imagen nueva, la valida con fsck_hfs de Apple y
#      comprueba, montándola, que todo es idéntico (contenido, xattrs, flags, CNID…).
# Además lee la imagen en los formatos UDIF de hdiutil (UDRO, UDZO, UDBZ, UDTO) y comprueba
# un error claro en los no soportados (ULFO = LZFSE, ULMO = LZMA). Por último aplica daños
# deterministas a un volumen sin mapa de particiones y verifica la reparación.
#
# Solo usa herramientas de macOS y python3. Requiere sudo sin contraseña para fsck_hfs
# (como en los runners de GitHub Actions).
set -euo pipefail

CLI=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
COR=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
W=${3:-$(mktemp -d -t blocksmith-hfs)}
HERE=$(cd "$(dirname "$0")" && pwd)
MANIFEST="python3 -I $HERE/hfs_manifest.py"
mkdir -p "$W"
W=$(cd "$W" && pwd -P)
cd "$W"

FAILS=0
SUMMARY=""

log() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
ok() { SUMMARY="$SUMMARY\n✓ $*"; echo "✓ $*"; }
fail() { SUMMARY="$SUMMARY\n✗ $*"; echo "✗ $*"; FAILS=$((FAILS + 1)); }

# Desmonta todas las imágenes de este directorio de trabajo que sigan conectadas.
attached_here() {
  hdiutil info | awk -v w="$W" '/^image-path/ {p = (index($0, w) > 0)} p && /^\/dev\/disk[0-9]+[ \t]/ {print $1}'
}
cleanup() {
  for d in $(attached_here); do hdiutil detach -force "$d" >/dev/null 2>&1 || true; done
}
trap cleanup EXIT

# attach <imagen> [opciones…] → imprime "disco_entero dispositivo_del_volumen punto_de_montaje"
attach() {
  local img=$1
  shift
  local out
  out=$(hdiutil attach -nobrowse -noverify -noautoopen "$@" "$img")
  local whole vol mnt
  whole=$(echo "$out" | awk '/^\/dev\/disk/ {print $1; exit}')
  # El volumen HFS es la entrada Apple_HFS (o la última, si hdiutil no indica el tipo).
  vol=$(echo "$out" | awk '/^\/dev\/disk/ {d=$1} /Apple_HFS/ {h=$1} END {print (h != "" ? h : d)}')
  mnt=$(echo "$out" | awk -F'\t' '/^\/dev\/disk/ && NF>=3 && $NF ~ /^\// {m=$NF} END {print m}')
  echo "$whole $vol $mnt"
}

detach() {
  hdiutil detach "$1" >/dev/null || hdiutil detach -force "$1" >/dev/null
}

# fsck_hfs de Apple, sin modificar nada (-n) y forzando la comprobación (-f).
apple_fsck() {
  local img=$1 label=$2
  shift 2
  local whole vol mnt
  read -r whole vol mnt < <(attach "$img" -nomount "$@")
  if sudo -n fsck_hfs -fn "/dev/r${vol#/dev/}" >"fsck-$label.log" 2>&1; then
    ok "$label: fsck_hfs acepta el volumen"
  else
    fail "$label: fsck_hfs rechaza el volumen"
    cat "fsck-$label.log"
  fi
  detach "$whole"
}

# Inventario de una imagen montada en solo lectura.
snapshot() {
  local img=$1 out=$2
  shift 2
  local whole vol mnt
  mkdir -p "$W/mnt-snap"
  read -r whole vol mnt < <(attach "$img" -readonly -mountpoint "$W/mnt-snap" "$@")
  [ -n "$mnt" ] || { fail "$img: no se pudo montar"; detach "$whole"; return; }
  $MANIFEST snapshot "$mnt" "$out"
  detach "$whole"
}

# scan_clean <imagen> <etiqueta> <mínimo_de_archivos>
scan_clean() {
  local img=$1 label=$2 min=$3
  if ! "$CLI" scan --json "$img" >"scan-$label.json" 2>"scan-$label.err"; then
    fail "$label: el escaneo falla"
    cat "scan-$label.err"
    return
  fi
  python3 -I - "scan-$label.json" "$label" "$min" <<'PY' && ok "$label: escaneo limpio" || fail "$label: escaneo con hallazgos"
import json, sys
r = json.load(open(sys.argv[1])); label = sys.argv[2]; minimum = int(sys.argv[3])
bad = [f for f in r["findings"] if f["severity"] != "cosmetic"]
s = r["stats"]
print(f"  {label}: {s['files']} archivos, {s['folders']} carpetas, {s['nodes_total']} nodos, "
      f"{len(r['findings'])} hallazgos ({len(bad)} no cosméticos)")
for f in r["findings"]:
    print(f"    [{f['severity']}] {f['code']}: {f['message']}")
sys.exit(1 if bad or s["files"] < minimum else 0)
PY
}

populate() {
  local m=$1
  # Muchos archivos pequeños: catálogo de varios niveles.
  mkdir -p "$m/muchos"
  for i in $(seq 1 1800); do printf 'archivo %d\n' "$i" >"$m/muchos/f$i.txt"; done
  # Huecos: crea y borra.
  mkdir -p "$m/huecos"
  for i in $(seq 1 300); do head -c $((i * 37)) /dev/urandom >"$m/huecos/h$i"; done
  for i in $(seq 1 2 300); do rm "$m/huecos/h$i"; done
  # Profundidad.
  mkdir -p "$m/a/b/c/d/e/f/g/h/i/j"
  echo profundo >"$m/a/b/c/d/e/f/g/h/i/j/fondo.txt"
  # Unicode (macOS lo guarda descompuesto) y nombres largos.
  echo cafe >"$m/café.txt"
  echo nihongo >"$m/日本語のファイル.txt"
  echo cohete >"$m/cohete 🚀.txt"
  mkdir -p "$m/Ünïcödé carpeta"
  echo dentro >"$m/Ünïcödé carpeta/ñandú.txt"
  echo largo >"$m/$(printf 'x%.0s' $(seq 1 250)).txt"
  # Mayúsculas: en HFSX conviven, en HFS+ el segundo sustituye al primero.
  echo minus >"$m/caso.txt"
  # Archivos grandes y fragmentados.
  head -c 7000000 /dev/urandom >"$m/grande.bin"
  for i in $(seq 1 48); do
    head -c 65536 /dev/urandom >>"$m/frag-a.bin"
    head -c 65536 /dev/urandom >>"$m/frag-b.bin"
  done
  : >"$m/vacio"
  mkdir -p "$m/carpeta vacía"
  # Enlaces duros (HFS+ los guarda en la carpeta privada) y simbólicos.
  echo compartido >"$m/enlace-a"
  ln "$m/enlace-a" "$m/enlace-b"
  ln "$m/enlace-a" "$m/a/b/enlace-c"
  ln -s "a/b/c/d/e/f/g/h/i/j/fondo.txt" "$m/simbolico"
  ln -s "/no/existe" "$m/simbolico-roto"
  # Atributos extendidos: en línea, grande (registro de extents en el archivo de
  # atributos) y resource fork.
  echo xattr >"$m/con-xattr"
  xattr -w dev.blocksmith.prueba "valor" "$m/con-xattr"
  xattr -w dev.blocksmith.grande "$(head -c 12000 /dev/zero | tr '\0' 'a')" "$m/con-xattr"
  xattr -wx com.apple.FinderInfo "54455854424f424a000000000000000000000000000000000000000000000000" "$m/con-xattr"
  echo datos >"$m/con-rsrc"
  head -c 200000 /dev/urandom >"$m/con-rsrc/..namedfork/rsrc"
  # Compresión HFS (decmpfs), como la de los archivos del sistema.
  mkdir -p "$W/src-comprimir"
  for i in $(seq 1 40); do
    for j in $(seq 1 400); do echo "línea $j de un texto muy repetitivo número $i"; done >"$W/src-comprimir/t$i.txt"
  done
  head -c 3000000 /dev/zero >"$W/src-comprimir/ceros.bin"
  ditto --hfsCompression "$W/src-comprimir" "$m/comprimidos"
  rm -rf "$W/src-comprimir"
  local n
  n=$(find "$m/comprimidos" -type f -flags +compressed | wc -l | tr -d ' ')
  echo "  archivos con compresión HFS: $n"
  [ "$n" -gt 0 ] || { fail "ditto no comprimió ningún archivo"; }
}

# variant <etiqueta> <fs> <layout>
variant() {
  local label=$1 fs=$2 layout=$3
  log "Variante $label ($fs, $layout)"
  rm -f "$label.dmg"
  hdiutil create -quiet -size 160m -fs "$fs" -volname "BSV $label" -layout "$layout" -type UDIF "$label.dmg"
  local whole vol mnt
  mkdir -p "$W/mnt-$label"
  read -r whole vol mnt < <(attach "$label.dmg" -mountpoint "$W/mnt-$label")
  populate "$mnt"
  if [ "$fs" = "JHFS+X" ] || [ "$fs" = "HFSX" ]; then echo MAYUS >"$mnt/CASO.txt"; fi
  sync
  detach "$whole"

  "$CLI" probe "$label.dmg" || fail "$label: probe falla"
  apple_fsck "$label.dmg" "$label-original"
  snapshot "$label.dmg" "$label-original.json"
  scan_clean "$label.dmg" "$label-original" 1900

  rm -f "$label-rebuilt.img"
  if "$CLI" rebuild "$label.dmg" --out "$label-rebuilt.img" >"rebuild-$label.log" 2>&1; then
    ok "$label: reconstrucción + verificación propia"
  else
    fail "$label: la reconstrucción falla"
    cat "rebuild-$label.log"
    return
  fi
  tail -n 3 "rebuild-$label.log"
  apple_fsck "$label-rebuilt.img" "$label-rebuilt" -imagekey diskimage-class=CRawDiskImage
  snapshot "$label-rebuilt.img" "$label-rebuilt.json" -imagekey diskimage-class=CRawDiskImage
  if $MANIFEST compare "$label-original.json" "$label-rebuilt.json"; then
    ok "$label: la reconstrucción conserva todo (contenido, xattrs, flags, CNID, fechas)"
  else
    fail "$label: la reconstrucción cambia el contenido"
  fi
}

formats() {
  local base=$1
  log "Formatos UDIF a partir de $base"
  for f in UDRO UDZO UDBZ UDTO; do
    rm -f "fmt-$f".*
    hdiutil convert -quiet "$base" -format "$f" -o "fmt-$f"
    local img
    img=$(ls fmt-"$f".* | head -n 1)
    scan_clean "$img" "formato-$f" 1900
  done
  for f in ULFO ULMO; do
    rm -f "fmt-$f".*
    if ! hdiutil convert -quiet "$base" -format "$f" -o "fmt-$f" 2>/dev/null; then
      echo "  hdiutil no admite $f en este sistema; se omite"
      continue
    fi
    local img err
    img=$(ls fmt-"$f".* | head -n 1)
    if "$CLI" scan "$img" >/dev/null 2>"err-$f.txt"; then
      fail "formato-$f: debería informar de compresión no soportada"
    else
      err=$(cat "err-$f.txt")
      if echo "$err" | grep -qE "LZFSE|LZMA"; then
        ok "formato-$f: error claro (${err##*: })"
      else
        fail "formato-$f: error poco claro: $err"
      fi
    fi
  done
}

damage() {
  log "Daños sobre un volumen HFS+ de Apple sin mapa de particiones"
  hdiutil convert -quiet "plano.dmg" -format UDTO -o plano-raw
  local raw=plano-raw.cdr applied=0
  # Daños que no deben perder contenido (pueden cambiar rutas o nombres).
  local keep="valence file-count next-cnid bitmap-leak bitmap-underalloc folder-record-lost folder-and-thread-lost key-order"
  # Daños que pueden perder datos (y se comprueba solo la coherencia).
  local lossy="broken-link node-unreadable extent-overlap extent-out-of-range"
  for k in $keep $lossy; do
    rm -f "d-$k.img" "f-$k.img"
    if ! "$COR" damage "$raw" "d-$k.img" --kind "$k" >"cor-$k.log" 2>&1; then
      echo "  $k: no aplicable a este volumen ($(tail -n 1 "cor-$k.log")); se omite"
      continue
    fi
    applied=$((applied + 1))
    if "$CLI" scan --json "d-$k.img" | python3 -I -c 'import json,sys; sys.exit(0 if any(f["severity"]!="cosmetic" for f in json.load(sys.stdin)["findings"]) else 1)'; then
      ok "daño $k: detectado"
    else
      fail "daño $k: no detectado"
    fi
    if ! "$CLI" rebuild "d-$k.img" --out "f-$k.img" >"rebuild-d-$k.log" 2>&1; then
      fail "daño $k: la reconstrucción falla"
      tail -n 20 "rebuild-d-$k.log"
      continue
    fi
    ok "daño $k: reconstruido y verificado"
    apple_fsck "f-$k.img" "daño-$k" -imagekey diskimage-class=CRawDiskImage
    case " $keep " in
      *" $k "*)
        snapshot "f-$k.img" "f-$k.json" -imagekey diskimage-class=CRawDiskImage
        if $MANIFEST compare "plano-original.json" "f-$k.json" --content-only; then
          ok "daño $k: no se pierde contenido"
        else
          fail "daño $k: se pierde contenido"
        fi
        ;;
    esac
    rm -f "d-$k.img" "f-$k.img"
  done
  if [ "$applied" -lt 6 ]; then fail "solo se pudieron aplicar $applied daños"; fi
}

sw_vers || true
variant gpt-jhfs JHFS+ GPTSPUD
variant apm-hfs HFS+ SPUD
variant mbr-hfsx JHFS+X MBRSPUD
variant plano HFS+ NONE
formats gpt-jhfs.dmg
damage

printf '\n\033[1m== Resumen\033[0m'
printf "$SUMMARY\n"
if [ "$FAILS" -gt 0 ]; then
  echo "$FAILS comprobaciones fallidas"
  exit 1
fi
echo "Todas las comprobaciones pasan."
