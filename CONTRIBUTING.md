# Contribuir a BlockSmith

Gracias por tu interés. Antes de abrir un *pull request*, lee estas condiciones.

## Licencia de las contribuciones

BlockSmith se distribuye bajo la PolyForm Noncommercial License 1.0.0 y, además, se ofrece
bajo licencias comerciales independientes (ver [`COMMERCIAL.md`](COMMERCIAL.md)). Para que eso
siga siendo posible, cada contribución debe aceptar lo siguiente.

Al enviar una contribución (código, documentación, imágenes u otro material) declaras que:

1. Es obra tuya, o tienes derecho a enviarla bajo estas condiciones.
2. Concedes al titular del proyecto (JoaquinHervasFarru) una licencia perpetua, mundial,
   no exclusiva, gratuita, irrevocable y sublicenciable para usar, reproducir, modificar,
   distribuir y **relicenciar** tu contribución, incluso bajo licencias comerciales.
3. Concedes, en los mismos términos, una licencia de las patentes tuyas que la contribución
   necesariamente infrinja.
4. Conservas tus derechos de autor sobre la contribución.

Indícalo añadiendo esta línea al final de cada mensaje de commit:

```
Signed-off-by: Tu Nombre <tu-usuario@users.noreply.github.com>
```

(`git commit -s` la añade automáticamente.) Sin esa línea el *pull request* no se puede aceptar.

## Normas técnicas

- **Clean-room**: no aportes código, estructuras ni textos obtenidos por ingeniería inversa de
  productos comerciales. Las fuentes permitidas son la documentación pública de cada formato
  (ver [`docs/CLEAN_ROOM.md`](docs/CLEAN_ROOM.md)).
- Antes de enviar, ejecuta:

  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  npm --prefix app run typecheck
  python3 scripts/licenses.py --check
  ```

- Cualquier cambio que escriba en un disco debe ir acompañado de pruebas de interrupción
  (ver `crates/txwriter/tests/crash_hfs.rs`).
- Las dependencias nuevas deben tener una licencia permisiva compatible
  (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, MPL-2.0 sin modificar). La CI lo comprueba;
  tras cambiar dependencias, regenera `THIRD_PARTY_NOTICES.md` con `python3 scripts/licenses.py`.
