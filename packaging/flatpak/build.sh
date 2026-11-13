#!/usr/bin/env bash
# Gera o pacote Flatpak (ignis.flatpak) a partir do .deb do Ignis.
# Uso: ./build.sh Ignis_x.y.z_amd64.deb [saida.flatpak]
set -euo pipefail
DEB="$(realpath "$1")"
SAIDA="$(realpath -m "${2:-Ignis.flatpak}")"
cd "$(dirname "$0")"
cp "$DEB" ignis.deb
# A versão do Flatpak (flatpak info, loja de apps) vem do metainfo: garante a do .deb.
VERSAO=$(dpkg-deb -f ignis.deb Version 2>/dev/null || basename "$DEB" | sed -E 's/^Ignis_([0-9.]+)_.*/\1/')
VERSAO="${VERSAO%%-*}"
META=io.github.kaikydev1jpg.Ignis.metainfo.xml
if ! grep -q "<release version=\"$VERSAO\"" "$META"; then
  sed -i "s|<releases>|<releases>\n    <release version=\"$VERSAO\" date=\"$(date -u +%F)\"/>|" "$META"
fi
echo "Versão no metainfo: $VERSAO"
flatpak-builder --user --force-clean --install-deps-from=flathub --repo=repo build-dir io.github.kaikydev1jpg.Ignis.yml
flatpak build-bundle repo "$SAIDA" io.github.kaikydev1jpg.Ignis --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
rm -f ignis.deb
echo "Pronto: $SAIDA"
echo "Instalar: flatpak install --user $SAIDA"
