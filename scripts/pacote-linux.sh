#!/usr/bin/env bash
# Gera os instaladores do Linux em dist/:
#   elo_<versão>_<arq>.deb             (Debian, Ubuntu e derivados: instala em /usr)
#   elo-<versão>-linux-<arq>.tar.gz    (qualquer distro: ./instalar.sh, sem root)
#
# Uso: scripts/pacote-linux.sh [pasta-dos-executáveis]
#      (padrão: compila em release em target/host)
#      VARIANTE=cliente scripts/pacote-linux.sh   gera o Elo Cliente
#      (elo-cliente_<versão>_<arq>.deb e elo-cliente-<versão>-linux-<arq>.tar.gz)
#
# O .deb exige a glibc do sistema em que foi compilado ou mais nova: para
# atender distros antigas, compile na mais antiga que quiser suportar (o CI
# usa Ubuntu 22.04).
set -euo pipefail
umask 022

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
# Elo (técnico) ou Elo Cliente: o mesmo executável, com outro nome.
if [[ "${VARIANTE:-}" == cliente ]]; then
    COMENTARIO="Receba suporte remoto da sua empresa"; APP="Elo Cliente"; EXE=elo-cliente; PACOTE=elo-cliente; ID=br.com.stringari.elo-cliente
    RESUMO="Elo Cliente: suporte remoto com aceite na tela"
else
    COMENTARIO="Acesse e controle outro computador"; APP="Elo"; EXE=elo; PACOTE=elo; ID=br.com.stringari.elo
    RESUMO="Acesso remoto com cifragem ponta a ponta"
fi
VERSAO="$(sed -n 's/^version = "\(.*\)"/\1/p' "$RAIZ/Cargo.toml" | head -1)"
DIST="$RAIZ/dist"

if [[ -n "${1:-}" ]]; then
    REL="$(cd "$1" && pwd)"
else
    export PATH="$HOME/.cargo/bin:$PATH"
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RAIZ/target/host}"
    cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" -p elo -p elo-server
    REL="$CARGO_TARGET_DIR/release"
fi

case "$(uname -m)" in
    x86_64) ARQ_DEB=amd64; ARQ=x86_64 ;;
    aarch64) ARQ_DEB=arm64; ARQ=aarch64 ;;
    *) ARQ_DEB="$(uname -m)"; ARQ="$(uname -m)" ;;
esac

# Versão mínima da glibc que os executáveis pedem.
GLIBC="$(objdump -T "$REL/elo" "$REL/elo-server" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1)"
GLIBC="${GLIBC:-2.31}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$DIST"
# Executável com o nome da variante.
install -m 755 "$REL/elo" "$TMP/$EXE"

# ---- .deb ----
PKG="$TMP/deb"
mkdir -m 755 "$PKG"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/applications" "$PKG/usr/share/icons/hicolor/scalable/apps" "$PKG/usr/share/icons/hicolor/256x256/apps" "$PKG/usr/share/doc/$PACOTE"
# O servidor avulso tem pacote próprio (elo-server); o app traz o servidor embutido.
install -m 755 "$TMP/$EXE" "$PKG/usr/bin/"
install -m 644 "$RAIZ/packaging/icons/elo.svg" "$PKG/usr/share/icons/hicolor/scalable/apps/$ID.svg"
install -m 644 "$RAIZ/packaging/icons/elo-256.png" "$PKG/usr/share/icons/hicolor/256x256/apps/$ID.png"
cat > "$PKG/usr/share/applications/$ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP
GenericName=Acesso remoto
Comment=$COMENTARIO
Exec=$EXE
Icon=$ID
Terminal=false
Categories=Network;RemoteAccess;
StartupWMClass=$ID
DESKTOP
install -m 644 "$RAIZ/LICENSE-MIT" "$RAIZ/LICENSE-APACHE" "$RAIZ/NOTICE" "$PKG/usr/share/doc/$PACOTE/"
TAM="$(du -sk "$PKG/usr" | cut -f1)"
cat > "$PKG/DEBIAN/control" <<CONTROL
Package: $PACOTE
Version: $VERSAO
Section: net
Priority: optional
Architecture: $ARQ_DEB
Maintainer: Gabriel Stringari <gabriel@gabrielstringari.com>
Installed-Size: $TAM
Depends: libc6 (>= $GLIBC), libgcc-s1, libx11-6, libxcursor1, libxrandr2, libxi6, libxkbcommon0, libxkbcommon-x11-0, libegl1 | libvulkan1, libasound2t64 | libasound2
Recommends: pipewire-bin | pulseaudio-utils
Suggests: elo-server
Homepage: https://github.com/Stringari-Consultoria/elo
Description: $RESUMO
 Um computador vê, ouve e controla a tela de outro, com chat, área de
 transferência e envio de arquivos. O servidor de conexão vem embutido
 no app; as sessões são cifradas ponta a ponta (SPAKE2 + ChaCha20-Poly1305).
CONTROL
DEB="$DIST/${PACOTE}_${VERSAO}_${ARQ_DEB}.deb"
fakeroot dpkg-deb --build -Zxz "$PKG" "$DEB" >/dev/null 2>&1 || dpkg-deb --root-owner-group --build -Zxz "$PKG" "$DEB" >/dev/null
echo "Gerado: $DEB"

# ---- .tar.gz ----
NOME="$PACOTE-$VERSAO-linux-$ARQ"
mkdir -p "$TMP/$NOME"
install -m 755 "$TMP/$EXE" "$TMP/$NOME/"
if [[ "$EXE" == elo ]]; then install -m 755 "$REL/elo-server" "$TMP/$NOME/"; fi
install -m 755 "$RAIZ/scripts/instalar-linux.sh" "$TMP/$NOME/instalar.sh"
install -m 644 "$RAIZ/packaging/icons/elo.svg" "$TMP/$NOME/elo.svg"
install -m 644 "$RAIZ/LICENSE-MIT" "$RAIZ/LICENSE-APACHE" "$RAIZ/NOTICE" "$TMP/$NOME/"
cat > "$TMP/$NOME/LEIA-ME.txt" <<TXT
$APP $VERSAO — acesso remoto

Instalar (sem root, em ~/.local):   ./instalar.sh
Remover:                            ./instalar.sh --remover
Ou só rodar, sem instalar:          ./$EXE

Requer Linux $ARQ com glibc $GLIBC ou mais nova e sessão X11.
TXT
TGZ="$DIST/$NOME.tar.gz"
tar -C "$TMP" -czf "$TGZ" "$NOME"
echo "Gerado: $TGZ"
