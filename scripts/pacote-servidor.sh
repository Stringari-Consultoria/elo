#!/usr/bin/env bash
# Gera o servidor avulso do Elo para Linux em dist/:
#   elo-server_<versão>_<arq>.deb             (Debian/Ubuntu: serviço systemd)
#   elo-server-<versão>-linux-<arq>.tar.gz    (qualquer distro: sudo ./instalar.sh)
# e copia o executável para docker-bin/ (imagem Docker no CI).
#
# Uso: scripts/pacote-servidor.sh [pasta-do-executável]
#      (padrão: compila em release em target/host)
set -euo pipefail
umask 022

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
VERSAO="$(sed -n 's/^version = "\(.*\)"/\1/p' "$RAIZ/Cargo.toml" | head -1)"
DIST="$RAIZ/dist"

if [[ -n "${1:-}" ]]; then
    REL="$(cd "$1" && pwd)"
else
    export PATH="$HOME/.cargo/bin:$PATH"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RAIZ/target/host}"
    cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" -p elo-server
    REL="$CARGO_TARGET_DIR/release"
fi

case "$(uname -m)" in
    x86_64) ARQ_DEB=amd64; ARQ=x86_64 ;;
    aarch64 | arm64) ARQ_DEB=arm64; ARQ=aarch64 ;;
    *) ARQ_DEB="$(uname -m)"; ARQ="$(uname -m)" ;;
esac

GLIBC="$(objdump -T "$REL/elo-server" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)"
GLIBC="${GLIBC:-2.31}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$DIST" "$RAIZ/docker-bin"
install -m 755 "$REL/elo-server" "$RAIZ/docker-bin/elo-server-$ARQ_DEB"

# ---- .deb ----
PKG="$TMP/deb"
mkdir -m 755 "$PKG"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/lib/systemd/system" "$PKG/etc/default" "$PKG/usr/share/doc/elo-server"
install -m 755 "$REL/elo-server" "$PKG/usr/bin/"
install -m 644 "$RAIZ/packaging/servidor/elo-server.service" "$PKG/lib/systemd/system/elo-server.service"
install -m 644 "$RAIZ/packaging/servidor/elo-server.default" "$PKG/etc/default/elo-server"
install -m 644 "$RAIZ/LICENSE" "$RAIZ/NOTICE" "$PKG/usr/share/doc/elo-server/"
echo "/etc/default/elo-server" > "$PKG/DEBIAN/conffiles"
TAM="$(du -sk "$PKG" | cut -f1)"
# (Replaces/Breaks: até a 0.1.3, o .deb do app também trazia /usr/bin/elo-server.)
cat > "$PKG/DEBIAN/control" <<CONTROL
Package: elo-server
Version: $VERSAO
Section: net
Priority: optional
Architecture: $ARQ_DEB
Maintainer: Gabriel Stringari <gabriel@gabrielstringari.com>
Installed-Size: $TAM
Depends: libc6 (>= $GLIBC), libgcc-s1
Replaces: elo (<< 0.1.4)
Breaks: elo (<< 0.1.4)
Homepage: https://github.com/Stringari-Consultoria/elo
Description: Servidor do Elo (acesso remoto), para hospedar numa máquina dedicada
 Guarda os IDs dos computadores, entrega os pedidos de conexão e repassa as
 sessões, que são cifradas ponta a ponta: o servidor nunca tem a chave.
 Roda como serviço do systemd na porta 21116, com TLS autoassinado.
CONTROL
cat > "$PKG/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ "$1" = configure ] && [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
    if [ -z "$2" ]; then
        systemctl enable --now elo-server.service >/dev/null 2>&1 || true
    else
        systemctl try-restart elo-server.service >/dev/null 2>&1 || true
    fi
    # Espera o certificado e mostra o que os apps precisam.
    i=0
    while [ $i -lt 20 ] && ! FP="$(elo-server --impressao --data /var/lib/elo-server 2>/dev/null)"; do
        sleep 0.25
        i=$((i + 1))
    done
    IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
    echo
    echo "Servidor do Elo rodando (serviço elo-server, porta 21116)."
    echo "Nos apps, em Configurações → Servidor, desligue \"Rodar o servidor neste computador\" e use:"
    echo "  Endereço:  ${IP:-<ip-deste-servidor>}:21116   (TLS ligado)"
    [ -n "${FP:-}" ] && echo "  Impressão digital:  $FP"
    echo "Opções em /etc/default/elo-server (depois: sudo systemctl restart elo-server)."
    echo
fi
exit 0
POSTINST
cat > "$PKG/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e
if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
    systemctl disable --now elo-server.service >/dev/null 2>&1 || true
fi
exit 0
PRERM
cat > "$PKG/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e
if [ -d /run/systemd/system ]; then
    systemctl daemon-reload >/dev/null 2>&1 || true
fi
if [ "$1" = purge ]; then
    rm -rf /var/lib/private/elo-server /var/lib/elo-server
fi
exit 0
POSTRM
chmod 755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm" "$PKG/DEBIAN/postrm"
DEB="$DIST/elo-server_${VERSAO}_${ARQ_DEB}.deb"
fakeroot dpkg-deb --build -Zxz "$PKG" "$DEB" >/dev/null 2>&1 || dpkg-deb --root-owner-group --build -Zxz "$PKG" "$DEB" >/dev/null
echo "Gerado: $DEB"

# ---- .tar.gz ----
NOME="elo-server-$VERSAO-linux-$ARQ"
mkdir -p "$TMP/$NOME"
install -m 755 "$REL/elo-server" "$TMP/$NOME/"
install -m 755 "$RAIZ/scripts/instalar-servidor.sh" "$TMP/$NOME/instalar.sh"
install -m 644 "$RAIZ/packaging/servidor/elo-server.service" "$RAIZ/packaging/servidor/elo-server.default" "$TMP/$NOME/"
install -m 644 "$RAIZ/LICENSE" "$RAIZ/NOTICE" "$TMP/$NOME/"
cat > "$TMP/$NOME/LEIA-ME.txt" <<TXT
Servidor do Elo $VERSAO (Linux $ARQ, glibc $GLIBC ou mais nova)

Instalar como serviço (systemd, sobe com a máquina):   sudo ./instalar.sh
Remover:                                               sudo ./instalar.sh --remover
Ou só rodar:   ./elo-server --listen 0.0.0.0:21116 --data ./dados --self-signed

Nos apps, em Configurações → Servidor: desligue "Rodar o servidor neste
computador", use <ip-deste-servidor>:21116 e cole a impressão digital
(elo-server --impressao --data <pasta-dos-dados>).

Libere a porta 21116/TCP no firewall. Detalhes:
https://github.com/Stringari-Consultoria/elo#servidor
TXT
TGZ="$DIST/$NOME.tar.gz"
tar -C "$TMP" -czf "$TGZ" "$NOME"
echo "Gerado: $TGZ"
