# Pruebas de extremo a extremo

| Script | Qué prueba | Requisitos |
|---|---|---|
| `demo.js` | Flujo completo en el navegador con el motor simulado: abrir, analizar, planificar, comparar, exportar, historial, ajustes y cancelación | Playwright y `npm run dev` en marcha |
| `real_health.py` | Salud de discos en la **app real** con el backend de reproducción (`replay-demo.json`): lectura al abrir, «Leer ahora» con alertas nuevas, detalle, confirmación y disco USB sin S.M.A.R.T. | Lo mismo que `real_app.py`, con `BLOCKSMITH_HEALTH_REPLAY=$PWD/app/e2e/replay-demo.json` exportada antes de lanzar `tauri-driver` |
| `real_repair.py` | Escritura directa en la **app real**: asistente, corte simulado tras la 2.ª escritura, «muerte» de la app, aviso al reabrir, completar (con `fsck.hfsplus`) y deshacer hasta la imagen original exacta (SHA-256) | Lo mismo que `real_app.py`, con `BLOCKSMITH_TX_FAIL_AFTER=2` exportada antes de lanzar `tauri-driver` (solo funciona en compilaciones de desarrollo) |
| `real_app.py` | Flujo completo en la **app de escritorio real** (Tauri + WebKitGTK) contra el motor real, y `fsck.hfsplus` sobre la imagen exportada | Linux con `tauri-driver`, `WebKitWebDriver`, Xvfb, `hfsprogs`, la app compilada y `npm run dev` en marcha (la app en modo debug carga la interfaz desde ahí) |

El diálogo nativo de archivos no se puede automatizar, así que `real_app.py` abre la imagen con el mismo
comando IPC que usa el botón «Abrir imagen…». El resto de la interacción son clics reales.

Ejecutar la prueba de la app real en un Linux sin pantalla:

```bash
cargo build -p blocksmith-app && cargo run -p blocksmith-corruptor -- gen --out fixtures/hfsplus
```

```bash
Xvfb :99 -screen 0 1360x900x24 & export DISPLAY=:99 LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
```

```bash
dbus-run-session -- tauri-driver &
```

```bash
python3 app/e2e/real_app.py "$PWD/target/debug/blocksmith" "$PWD/fixtures/hfsplus/dañado-folder-and-thread-lost.img" /tmp/capturas
```
