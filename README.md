# BlockSmith

Verificación, recuperación y reconstrucción de directorios de disco para HFS+, APFS, NTFS y ext4,
con capa de cifrado, monitorización S.M.A.R.T. y medio de arranque externo.

Plan técnico aprobado: [`docs/BlockSmith-Plan-Tecnico-v3.pdf`](docs/BlockSmith-Plan-Tecnico-v3.pdf).
Estado actual: **F5 completada** — además de analizar, reparar en una imagen nueva (F1–F3) y vigilar la
salud de los discos (F4), BlockSmith puede **reparar en el propio origen** (imagen o disco) de forma
transaccional, con copia de deshacer, y recuperar transacciones interrumpidas.

[![CI](https://github.com/JoaquinHervasFarru/BlockSmith/actions/workflows/ci.yml/badge.svg)](https://github.com/JoaquinHervasFarru/BlockSmith/actions/workflows/ci.yml)
[![HFS+ en macOS](https://github.com/JoaquinHervasFarru/BlockSmith/actions/workflows/macos-hfs.yml/badge.svg)](https://github.com/JoaquinHervasFarru/BlockSmith/actions/workflows/macos-hfs.yml)
[![Licencia: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/licencia-PolyForm%20Noncommercial%201.0.0-blue)](LICENSE)

## Requisitos

- Rust ≥ 1.83 y Node.js ≥ 20
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

## Reparar en el propio origen

Desde la comparación de una reconstrucción, «Reparar en el origen…» abre un asistente con comprobaciones
previas (no montado, no es el disco de arranque, journal limpio, salud del disco, copia de deshacer en
otro disco y espacio suficiente) y una confirmación escribiendo el nombre del volumen.

- Antes de escribir se guarda una **copia de deshacer** con los bytes originales y nuevos de cada bloque.
- Las estructuras nuevas van a espacio libre y la cabecera del volumen se actualiza al final: un corte
  antes deja la versión anterior intacta (probado cortando en cada escritura posible).
- Al reabrir la app, una transacción interrumpida aparece en «Transacciones» para **revertirla** o
  **completarla**. También se puede deshacer una reparación terminada si el volumen no ha cambiado.

Si el usuario puede escribir en el destino (una imagen suya), todo ocurre dentro de la app. Si no, se usa
`blocksmith-helper` con permisos de administrador:

| Plataforma | Elevación | Instalación |
|---|---|---|
| Linux | `pkexec` (polkit) | Copiar `blocksmith-helper` a `/usr/local/bin` y `docs/packaging/dev.blocksmith.helper.policy` a `/usr/share/polkit-1/actions/` |
| macOS | Diálogo de administrador (`osascript`) | `blocksmith-helper` junto a la app |
| Windows | Pendiente | La escritura directa en discos aún no está disponible |

El helper repite todas las comprobaciones por su cuenta, solo acepta un trabajo cuyo hash coincide con el
que aprobó la app y verifica el volumen al terminar.

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

## Licencia

BlockSmith se publica bajo la [PolyForm Noncommercial License 1.0.0](LICENSE).

- **Uso no comercial libre**: uso personal, estudio, investigación, organizaciones sin ánimo de
  lucro, centros educativos y organismos públicos pueden usarlo, modificarlo y compartirlo.
  Las copias deben incluir la licencia y la línea `Required Notice:` de [`NOTICE`](NOTICE).
- **Uso comercial** (venderlo, incluirlo en un producto o servicio de pago, usarlo para dar
  servicio a clientes…): requiere una licencia comercial aparte, con reconocimiento de autoría y
  remuneración acordados. Ver [`COMMERCIAL.md`](COMMERCIAL.md).
- Contribuciones: ver [`CONTRIBUTING.md`](CONTRIBUTING.md).
- Componentes de terceros: [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md), generado y
  comprobado en la CI con `python3 scripts/licenses.py --check`.
