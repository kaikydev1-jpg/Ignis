#!/usr/bin/env bash
# Gera o pacote Flatpak (ignis.flatpak) a partir do .deb do Ignis.
# Uso: ./build.sh Ignis_0.1.0_amd64.deb [saida.flatpak]
set -euo pipefail
DEB="$(realpath "$1")"
SAIDA="$(realpath -m "${2:-Ignis.flatpak}")"
cd "$(dirname "$0")"
cp "$DEB" ignis.deb
flatpak-builder --user --force-clean --install-deps-from=flathub --repo=repo build-dir io.github.kaikydev1jpg.Ignis.yml
flatpak build-bundle repo "$SAIDA" io.github.kaikydev1jpg.Ignis --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
rm -f ignis.deb
echo "Pronto: $SAIDA"
echo "Instalar: flatpak install --user $SAIDA"
