#!/usr/bin/env bash
# Compila o Elo em modo release e instala para o usuário atual:
#   ~/.local/bin/elo e elo-server, atalho no menu de aplicativos e ícone.
# Uso: scripts/instalar-linux.sh            (compila e instala)
#      scripts/instalar-linux.sh --remover  (desinstala; mantém as configurações)
set -euo pipefail

ID=br.com.stringari.elo
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$HOME/.local/bin"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONES="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"

if [[ "${1:-}" == "--remover" ]]; then
    rm -f "$BIN/elo" "$BIN/elo-server" "$APPS/$ID.desktop" "$ICONES/$ID.svg"
    update-desktop-database "$APPS" 2>/dev/null || true
    echo "Elo removido (as configurações continuam em ~/.config/elo)."
    exit 0
fi

export PATH="$HOME/.cargo/bin:$PATH"
command -v cargo >/dev/null || { echo "cargo não encontrado: instale o Rust (https://rustup.rs)"; exit 1; }

# Pasta própria (a mesma do Makefile): não mistura com builds feitos em outro
# ambiente (ex.: VS Code em flatpak).
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RAIZ/target/host}"
cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" -p elo -p elo-server

mkdir -p "$BIN" "$APPS" "$ICONES"
install -m 755 "$CARGO_TARGET_DIR/release/elo" "$BIN/elo"
install -m 755 "$CARGO_TARGET_DIR/release/elo-server" "$BIN/elo-server"
install -m 644 "$RAIZ/packaging/linux/$ID.svg" "$ICONES/$ID.svg"
cat > "$APPS/$ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Elo
GenericName=Acesso remoto
Comment=Acesse e controle outro computador
Exec=$BIN/elo
Icon=$ID
Terminal=false
Categories=Network;RemoteAccess;
StartupWMClass=$ID
DESKTOP
update-desktop-database "$APPS" 2>/dev/null || true

echo
echo "Instalado:"
echo "  $BIN/elo         (app, com o servidor embutido)"
echo "  $BIN/elo-server  (servidor avulso, opcional)"
echo "Abra pelo menu de aplicativos (\"Elo\") ou rodando: elo"
