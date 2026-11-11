#!/usr/bin/env bash
# Cabeçalhos do PipeWire 1.0 (Ubuntu 24.04) para compilar no Ubuntu 22.04.
#
# O pacote do Linux é compilado no 22.04 (funciona nas distribuições mais
# antigas), mas o libspa 0.10 não compila com os cabeçalhos do PipeWire 0.3.48
# de lá. As funções que o Elo usa existem na biblioteca 0.3.48 e a ABI do
# PipeWire é compatível para frente: só os cabeçalhos vêm do 24.04; a
# biblioteca continua a do sistema.
#
# Uso: scripts/pipewire-cabecalhos.sh  (no CI grava as variáveis no GITHUB_ENV;
# fora dele, imprime os export para usar com eval).
set -euo pipefail

R="${PW_CABECALHOS:-$PWD/target/pw-dev}"
VER=1.0.5-1ubuntu3.3
# Reserva no launchpad (fica lá mesmo depois que o arquivo do Ubuntu tira a versão).
declare -A DEBS=(
    [libpipewire-0.3-dev_${VER}_amd64.deb]="https://launchpadlibrarian.net/868136190 1a0267989f02a8729cdb4df36d9acf3a5ac1a525200d428baffe8961ccb0e0ba"
    [libspa-0.2-dev_${VER}_amd64.deb]="https://launchpadlibrarian.net/868136197 93a60e7656fb35c97a78159fd0556842c62c2d8b9aff096009f9a20c64c1f272"
)
# Primeiro o arquivo do Ubuntu (o launchpad sai do ar às vezes). HTTP serve:
# o SHA-256 fixado acima confere cada pacote, venha de onde vier.
POOL=http://archive.ubuntu.com/ubuntu/pool/main/p/pipewire

mkdir -p "$R/debs" "$R/root"
for deb in "${!DEBS[@]}"; do
    read -r url sum <<<"${DEBS[$deb]}"
    get() { curl -fsSL --connect-timeout 30 --max-time 300 --retry 3 --retry-delay 10 --retry-all-errors -o "$R/debs/$deb" "$1"; }
    [ -f "$R/debs/$deb" ] || get "$POOL/$deb" || get "$url/$deb"
    echo "$sum  $R/debs/$deb" | sha256sum -c --quiet
    dpkg-deb -x "$R/debs/$deb" "$R/root"
done

LIB=/usr/lib/x86_64-linux-gnu
# Para o ligador: aponta para a biblioteca do sistema (a 0.3.48 no 22.04).
so=$(ls "$LIB"/libpipewire-0.3.so.0* | sort | tail -1)
ln -sf "$so" "$R/root$LIB/libpipewire-0.3.so"

vars=(
    "PKG_CONFIG_PATH=$R/root$LIB/pkgconfig"
    "PKG_CONFIG_SYSROOT_DIR=$R/root"
    # Sem isto o pkg-config omite o -L (parece pasta do sistema) e o ligador não acha a biblioteca.
    "PKG_CONFIG_ALLOW_SYSTEM_LIBS=1"
    "BINDGEN_EXTRA_CLANG_ARGS=-I$R/root/usr/include/pipewire-0.3 -I$R/root/usr/include/spa-0.2"
)
for v in "${vars[@]}"; do
    if [ -n "${GITHUB_ENV:-}" ]; then echo "$v" >>"$GITHUB_ENV"; else printf 'export %q\n' "$v"; fi
done
