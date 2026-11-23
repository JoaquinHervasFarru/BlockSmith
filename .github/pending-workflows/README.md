# Workflows pendientes de activar

GitHub no permite que una integración sin el permiso `workflow` cree archivos en
`.github/workflows/`. Para activar la CI, mueve estos dos archivos a esa carpeta
desde una sesión con ese permiso (la web de GitHub o tu propio equipo):

- `ci.yml` → `.github/workflows/ci.yml`
- `macos-hfs.yml` → `.github/workflows/macos-hfs.yml`

Desde la web: abre el archivo, pulsa el lápiz («Edit this file») y, en el nombre
de arriba, borra `pending-workflows/` y escribe `workflows/`; luego «Commit changes».

Desde tu equipo:

```bash
git pull && git mv .github/pending-workflows/ci.yml .github/pending-workflows/macos-hfs.yml .github/workflows/ && git commit -m "Activar CI" && git push
```

Cuando ambos estén activos, esta carpeta se puede borrar.
