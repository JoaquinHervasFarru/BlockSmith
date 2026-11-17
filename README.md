# BlockSmith

Verificación, recuperación y reconstrucción de directorios de disco para HFS+, APFS, NTFS y ext4,
con capa de cifrado, monitorización S.M.A.R.T. y medio de arranque externo.

Plan técnico aprobado: [`docs/BlockSmith-Plan-Tecnico-v3.pdf`](docs/BlockSmith-Plan-Tecnico-v3.pdf).
Estado actual: **F4 completada** — además de analizar y reparar imágenes HFS+/HFSX (F1–F3), la app y el
agente `blocksmith-agent` vigilan la salud de los discos (S.M.A.R.T. y NVMe), guardan el histórico y
generan alertas configurables.

## Requisitos

- Rust ≥ 1.80 y Node.js ≥ 20
- Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libssl-dev`
- macOS: Xcode Command Line Tools · Windows: WebView2 + MSVC Build Tools

## Puesta en marcha

Instalar las dependencias del frontend:

```bash
cd app && npm install
```

Abrir la UI en el navegador en **modo demostración** (motor simulado, sin Tauri; http://localhost:1420):

```bash
cd app && npm run dev
```

Abrir la app de escritorio con el motor real (Tauri):

```bash
cd app && npm run tauri dev
```

Compilar y probar el motor:

```bash
cargo build --workspace && cargo test --workspace
```

Identificar una imagen desde la CLI:

```bash
cargo run -p blocksmith-cli -- probe ruta/a/imagen.dmg
```

Escanear un volumen HFS+ (solo lectura; `--json` para salida estructurada, `--tree` para ver el árbol):

```bash
cargo run -p blocksmith-cli -- scan ruta/a/imagen.img
```

Reconstruir el directorio en una imagen nueva (el origen nunca se modifica; `--dry-run` muestra
los cambios sin escribir):

```bash
cargo run -p blocksmith-cli -- rebuild ruta/a/imagen.img --out ruta/a/reparada.img
```

Generar las fixtures de prueba (volumen sano, variante HFSX y una imagen por cada tipo de daño):

```bash
cargo run -p blocksmith-corruptor -- gen --out fixtures/hfsplus
```

Los tests de `fs-hfsplus` validan las fixtures con `fsck.hfsplus` si está instalado
(paquete `hfsprogs` en Debian/Ubuntu), como verificador independiente.

## Salud de los discos

La app lee la salud al abrirse (si la última lectura es antigua) y con «Leer ahora». Para vigilarla de
forma continua, instala el agente como servicio del sistema: las lecturas necesitan permisos de
administrador. El agente escribe en la misma base de datos que la app.

Ver qué haría el agente una vez:

```bash
sudo blocksmith-agent once
```

Generar la definición del servicio (systemd en Linux, launchd en macOS, `sc.exe` en Windows):

```bash
blocksmith-agent print-service
```

| Plataforma | Lectura |
|---|---|
| Linux | Nativa (`SG_IO` para SATA, ioctl NVMe); si un disco no la admite, `smartctl` si está instalado |
| macOS | `smartctl` (smartmontools) como programa externo |
| Windows | `smartctl` por ahora; la lectura nativa está pendiente |

Para demostraciones y pruebas sin hardware, `BLOCKSMITH_HEALTH_REPLAY=app/e2e/replay-demo.json` sustituye
los discos reales por lecturas definidas en ese archivo.

## Estructura

| Ruta | Contenido |
|---|---|
| `crates/` | Motor en Rust: io, partition, crypto-layers, smart, fs-core, fs-hfsplus, fs-apfs, fs-ntfs, fs-ext4, analyzer, rebuilder, txwriter, cli |
| `helper/` | Proceso privilegiado para la escritura directa |
| `agent/` | Agente de salud de discos (servicio del sistema) |
| `crates/store`, `crates/health` | Base de datos compartida y lógica de salud (reglas, recolector) |
| `app/src-tauri/` | App de escritorio: comandos IPC, trabajos, SQLite (`migrations/`) |
| `app/ui/` | React + TS: `styles/` (tokens), `mocks/`, `api/`, `components/`, `views/` |
| `live/` | Receta del USB de arranque |
| `fixtures/`, `tools/corruptor/` | Imágenes de prueba y generador de daños |
| `docs/` | Plan, sistema de diseño y registro clean-room |

## Datos simulados

`app/ui/src/mocks/data.ts` cubre 5 discos, 6 orígenes, 7 volúmenes (todos los formatos),
5 capas de cifrado, sesiones, hallazgos, reconstrucciones, diff, una transacción interrumpida,
30 días de S.M.A.R.T., alertas, informes y medios de arranque. La UI los usa automáticamente
fuera de Tauri. Dentro de Tauri se fuerzan con `VITE_USE_MOCKS=true`.
