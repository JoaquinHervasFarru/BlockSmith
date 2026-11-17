# Registro clean-room

DirForge es un producto independiente. Comparte una idea funcional con otras utilidades de
disco (reconstruir el directorio en lugar de parchearlo), pero su expresión es original:
código, interfaz, textos, nombre, logotipo y paleta.

## Reglas

1. **Sin ingeniería inversa** de productos de terceros. Del instalador aportado como referencia
   solo se identificó el formato contenedor (UDIF con un volumen HFS+), con la propia CLI
   de DirForge. No se extrajo, desensambló ni reutilizó ningún binario, recurso, texto o icono.
2. **Fuentes permitidas:** especificaciones y documentación públicas (ver tabla).
3. **Código GPL** (e2fsprogs, dislocker, smartmontools, apfsprogs…): solo como oráculo de
   pruebas o binario externo invocado. Nunca se copia ni se traduce.
4. **Marca:** no se menciona la marca de otros productos en la UI, los textos ni los recursos.
5. Cada módulo nuevo añade aquí las fuentes que consultó.

## Fuentes por módulo

| Módulo | Fuentes |
|---|---|
| io/udif | Descripción pública del trailer `koly` y la tabla `mish` |
| partition | UEFI Specification (GPT), documentación pública de APM y MBR |
| fs-hfsplus | Apple Technical Note TN1150 «HFS Plus Volume Format» |
| fs-apfs | Apple File System Reference (Apple Developer) |
| fs-ntfs | Documentación pública de la estructura de NTFS |
| fs-ext4 | Documentación del kernel Linux: ext4 Data Structures and Algorithms, jbd2 |
| crypto-layers | LUKS2 On-Disk Format Specification; documentación pública de BitLocker |
| smart | ATA/ATAPI Command Set (ACS), NVM Express Base Specification |
| UI | Diseño propio (docs/design-system.md) |

Antes de publicar se recomienda una revisión legal.
