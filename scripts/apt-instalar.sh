#!/usr/bin/env bash
# Instala pacotes do Ubuntu no CI com tempo limite e 3 tentativas.
# Se o espelho padrão do GitHub não responder, troca para outro espelho oficial.
set -u
export DEBIAN_FRONTEND=noninteractive
APT=(-o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o DPkg::Lock::Timeout=120)
ESPELHO="http://mirrors.edge.kernel.org/ubuntu/"

trocar_espelho() {
  echo "Trocando o espelho do Ubuntu para $ESPELHO"
  for f in /etc/apt/apt-mirrors.txt /etc/apt/sources.list /etc/apt/sources.list.d/*.sources /etc/apt/sources.list.d/*.list; do
    [ -f "$f" ] || continue
    sudo sed -i -E "s#https?://(azure\.)?archive\.ubuntu\.com/ubuntu/?#${ESPELHO}#g; s#https?://security\.ubuntu\.com/ubuntu/?#${ESPELHO}#g" "$f"
  done
}

for t in 1 2 3; do
  [ "$t" -ge 2 ] && trocar_espelho
  sudo dpkg --configure -a || true
  if sudo -E timeout 300 apt-get "${APT[@]}" update && sudo -E timeout 900 apt-get "${APT[@]}" install -y "$@"; then
    exit 0
  fi
  echo "apt travou ou falhou (tentativa $t)"
  sleep 15
done
exit 1
