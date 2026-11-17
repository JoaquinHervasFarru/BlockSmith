# Sistema de diseño

La fuente de verdad son los tokens de `app/ui/src/styles/tokens.css`. Los componentes solo
consumen variables, nunca colores literales.

| Capa | Archivo | Contenido |
|---|---|---|
| Tokens | `tokens.css` | Color (claro/oscuro), tipografía, espaciado, radios, sombras, movimiento, layout |
| Base | `base.css` | Reset, tipografía base, foco visible, selección, scrollbars |
| Primitivas | `components.css` | Shell, botones, tarjetas, badges, stats, tablas, formularios, avisos |
| Vistas | `views.css` | Composiciones (tarjeta de origen, estados vacíos, skeleton) |

- **Paleta:** marca índigo (`--brand-*`), acento ascua (`--ember-*`) reservado para las
  acciones que escriben en disco (`.btn--write`), y semánticos `--ok`, `--warn`, `--crit`
  y `--neutral`, cada uno con su variante `-soft`.
- **Tipografía:** Inter Variable para la interfaz y JetBrains Mono para hex, offsets, hashes
  y códigos. Son autoalojadas, así que funcionan sin conexión en BlockSmith Live.
- **Tema:** claro por defecto. El oscuro se aplica por `prefers-color-scheme` o con
  `<html data-theme="dark">`, y `data-theme="light"` fuerza el claro.
- **Accesibilidad:** contraste AA, `:focus-visible` con anillo y estados con icono + texto.
  Las animaciones respetan `prefers-reduced-motion`.
