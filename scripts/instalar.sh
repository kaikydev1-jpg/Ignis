#!/usr/bin/env bash
# Instalador do Ignis para Linux.
# Descobre o sistema, baixa a versão certa da página oficial do GitHub,
# confere o SHA-256 e instala:
#   Matsu OS (Matsu 27 OS, 28…)  -> Ignis-Matsu-OS.flatpak
#   Fedora Atomic e outras ostree -> Ignis-Flatpak.flatpak
#   Fedora                        -> Ignis-Fedora.rpm (dnf)
#   Ubuntu, Debian, Mint…         -> Ignis-Ubuntu-Debian.deb (apt)
#   outras com Flatpak            -> Ignis-Flatpak.flatpak
#   outras                        -> Ignis-Linux.AppImage em ~/Applications
#
# Uso:
#   curl -fsSL https://github.com/kaikydev1-jpg/Ignis/releases/latest/download/instalar.sh | bash
# Opções: --simular (mostra o que faria, sem baixar nem instalar)
#         IGNIS_VERSAO=0.1.1 (uma versão específica; padrão: a mais recente)
set -euo pipefail

REPO="kaikydev1-jpg/Ignis"
APP_ID="io.github.kaikydev1jpg.Ignis"
VERSAO="${IGNIS_VERSAO:-latest}"
if [ "$VERSAO" = "latest" ]; then
  BASE="https://github.com/$REPO/releases/latest/download"
else
  BASE="https://github.com/$REPO/releases/download/v${VERSAO#v}"
fi
SIMULAR=0
[ "${1:-}" = "--simular" ] && SIMULAR=1
OS_RELEASE="${IGNIS_OS_RELEASE:-/etc/os-release}"

diz() { printf '\033[1;38;5;208m▸\033[0m %s\n' "$*"; }
erro() { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }
rodar() { if [ "$SIMULAR" = 1 ]; then echo "  (simulação) $*"; else "$@"; fi; }

[ "$(uname -s)" = "Linux" ] || erro "Este instalador é para Linux. No Windows, baixe o Ignis-Windows.exe em https://github.com/$REPO/releases/latest"
[ "$(uname -m)" = "x86_64" ] || erro "O Ignis ainda só tem versão para processadores x86_64 (este é $(uname -m))."

# ---------- qual sistema é ----------
campo() { grep -m1 "^$1=" "$OS_RELEASE" 2>/dev/null | cut -d= -f2- | tr -d '"'"'" || true; }
ID=$(campo ID | tr '[:upper:]' '[:lower:]')
ID_LIKE=$(campo ID_LIKE | tr '[:upper:]' '[:lower:]')
NOME=$(campo NAME)
BONITO=$(campo PRETTY_NAME)
VARIANTE=$(campo VARIANT_ID | tr '[:upper:]' '[:lower:]')
[ -n "$BONITO" ] || BONITO="${NOME:-Linux}"

junto=$(printf '%s %s %s' "$ID" "$NOME" "$BONITO" | tr '[:upper:]' '[:lower:]' | tr -d ' _-')
parecido() { case " $ID $ID_LIKE " in *" $1 "*) return 0 ;; esac; return 1; }
ostree=0
if [ -n "${IGNIS_OSTREE:-}" ]; then ostree="$IGNIS_OSTREE"; elif [ -e /run/ostree-booted ]; then ostree=1; fi
case "$VARIANTE" in silverblue|kinoite|sericea|onyx|cosmic-atomic|atomic) ostree=1 ;; esac

if [[ "$junto" == *matsu* ]]; then
  SISTEMA="$BONITO"; ARQUIVO="Ignis-Matsu-OS.flatpak"; MODO=flatpak
elif [ "$ostree" = 1 ]; then
  SISTEMA="$BONITO (atômico)"; ARQUIVO="Ignis-Flatpak.flatpak"; MODO=flatpak
elif parecido fedora; then
  SISTEMA="$BONITO"; ARQUIVO="Ignis-Fedora.rpm"; MODO=dnf
elif parecido debian || parecido ubuntu; then
  SISTEMA="$BONITO"; ARQUIVO="Ignis-Ubuntu-Debian.deb"; MODO=apt
elif command -v flatpak >/dev/null 2>&1; then
  SISTEMA="$BONITO"; ARQUIVO="Ignis-Flatpak.flatpak"; MODO=flatpak
else
  SISTEMA="$BONITO"; ARQUIVO="Ignis-Linux.AppImage"; MODO=appimage
fi
diz "Sistema: $SISTEMA"
diz "Versão do Ignis para ele: $ARQUIVO"

# ---------- baixar e conferir ----------
baixar() {
  if command -v curl >/dev/null 2>&1; then curl -fL --retry 3 --progress-bar -o "$2" "$1"
  elif command -v wget >/dev/null 2>&1; then wget -q --show-progress -O "$2" "$1"
  else erro "Instale o curl ou o wget e rode de novo."; fi
}
PASTA=$(mktemp -d)
trap 'rm -rf "$PASTA"' EXIT
if [ "$SIMULAR" = 1 ]; then
  echo "  (simulação) baixar $BASE/$ARQUIVO e conferir com $BASE/SHA256SUMS.txt"
else
  diz "Baixando…"
  baixar "$BASE/$ARQUIVO" "$PASTA/$ARQUIVO"
  baixar "$BASE/SHA256SUMS.txt" "$PASTA/SHA256SUMS.txt" 2>/dev/null
  esperado=$(grep " $ARQUIVO\$" "$PASTA/SHA256SUMS.txt" | cut -d' ' -f1)
  [ -n "$esperado" ] || erro "Não achei $ARQUIVO na lista de somas SHA-256."
  obtido=$(sha256sum "$PASTA/$ARQUIVO" | cut -d' ' -f1)
  [ "$esperado" = "$obtido" ] || erro "O arquivo baixado não confere (SHA-256 diferente). Nada foi instalado."
  diz "Arquivo conferido (SHA-256 ok)."
fi

# ---------- instalar ----------
case "$MODO" in
  flatpak)
    command -v flatpak >/dev/null 2>&1 || erro "Este sistema precisa do Flatpak para instalar o Ignis."
    rodar flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
    rodar flatpak install --user -y --noninteractive "$PASTA/$ARQUIVO"
    COMO_ABRIR="pelo menu de aplicativos, ou: flatpak run $APP_ID"
    ;;
  dnf)
    rodar sudo dnf install -y "$PASTA/$ARQUIVO"
    COMO_ABRIR="pelo menu de aplicativos, ou: ignis"
    ;;
  apt)
    rodar sudo apt-get install -y "$PASTA/$ARQUIVO"
    COMO_ABRIR="pelo menu de aplicativos, ou: ignis"
    ;;
  appimage)
    destino="$HOME/Applications/Ignis-Linux.AppImage"
    rodar mkdir -p "$HOME/Applications"
    rodar install -m 755 "$PASTA/$ARQUIVO" "$destino"
    COMO_ABRIR="com: $destino"
    ;;
esac

diz "Pronto! Abra o Ignis $COMO_ABRIR"
diz "Na primeira vez, clique em \"Preparar o Ignis\"."
