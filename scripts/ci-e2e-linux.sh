#!/bin/bash
# Pruebas de extremo a extremo en Linux sin pantalla.
#
#   scripts/ci-e2e-linux.sh [carpeta_de_capturas]
#
# 1. Navegador (Playwright + Chromium) contra la UI con el motor simulado: app/e2e/demo.js.
# 2. App de escritorio real (Tauri + WebKitGTK vía tauri-driver) contra el motor real:
#    real_app.py, real_health.py y real_repair.py, cada una con su propia base de datos.
#
# Requisitos: la app compilada en modo debug (target/debug/blocksmith), las fixtures
# (blocksmith-corruptor gen), Xvfb, dbus-run-session, tauri-driver, WebKitWebDriver,
# fsck.hfsplus, Node con las dependencias de app/ instaladas y el paquete npm playwright.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${1:-$ROOT/target/e2e}
APP=$ROOT/target/debug/blocksmith
FIX=$ROOT/fixtures/hfsplus
mkdir -p "$OUT"
WORK=$(mktemp -d)
PIDS=""

cleanup() {
  for p in $PIDS; do kill "$p" 2>/dev/null || true; done
  pkill -f WebKitWebDriver 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

wait_http() {
  for _ in $(seq 1 120); do
    if curl -fsS -o /dev/null "$1" 2>/dev/null; then return 0; fi
    sleep 0.5
  done
  echo "no responde: $1" >&2
  return 1
}

[ -x "$APP" ] || { echo "falta $APP (cargo build -p blocksmith-app)" >&2; exit 1; }
[ -f "$FIX/sano.img" ] || { echo "faltan las fixtures (blocksmith-corruptor gen)" >&2; exit 1; }

# Servidor de desarrollo de la UI: la app en modo debug carga la interfaz desde ahí.
(cd "$ROOT/app/ui" && exec npm run dev -- --strictPort) >"$OUT/vite.log" 2>&1 &
PIDS="$PIDS $!"
wait_http http://localhost:1420/

echo "== Navegador: demo.js"
mkdir -p "$OUT/demo"
BLOCKSMITH_URL=http://localhost:1420 node "$ROOT/app/e2e/demo.js" "$OUT/demo"

if [ -z "${DISPLAY:-}" ]; then
  Xvfb :99 -screen 0 1360x900x24 >/dev/null 2>&1 &
  PIDS="$PIDS $!"
  export DISPLAY=:99
  sleep 1
fi
export LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1

# run_real <nombre> <variables de entorno…> -- <argumentos del script>
run_real() {
  local name=$1
  shift
  local envs=()
  while [ "$1" != "--" ]; do envs+=("$1"); shift; done
  shift
  local data="$WORK/data-$name"
  mkdir -p "$data" "$OUT/$name"
  echo "== App real: $name"
  env XDG_DATA_HOME="$data" "${envs[@]}" dbus-run-session -- tauri-driver >"$OUT/tauri-driver-$name.log" 2>&1 &
  local drv=$!
  wait_http http://127.0.0.1:4444/status
  local rc=0
  python3 "$ROOT/app/e2e/$name.py" "$@" || rc=$?
  kill "$drv" 2>/dev/null || true
  pkill -f WebKitWebDriver 2>/dev/null || true
  pkill -f "$APP" 2>/dev/null || true
  wait "$drv" 2>/dev/null || true
  sleep 1
  if [ "$rc" -ne 0 ]; then
    echo "✗ $name falló (código $rc)"
    tail -n 40 "$OUT/tauri-driver-$name.log" || true
    return "$rc"
  fi
  echo "✓ $name"
}

cp "$FIX/dañado-folder-and-thread-lost.img" "$WORK/app.img"
run_real real_app -- "$APP" "$WORK/app.img" "$OUT/real_app"

run_real real_health BLOCKSMITH_HEALTH_REPLAY="$ROOT/app/e2e/replay-demo.json" -- "$APP" "$OUT/real_health"

cp "$FIX/dañado-folder-record-lost.img" "$WORK/repair.img"
run_real real_repair BLOCKSMITH_TX_FAIL_AFTER=2 -- \
  "$APP" "$WORK/repair.img" "$WORK/data-real_repair/dev.blocksmith.desktop/blocksmith.sqlite3" "$OUT/real_repair"

echo "Todas las pruebas de extremo a extremo pasan. Capturas en $OUT"
