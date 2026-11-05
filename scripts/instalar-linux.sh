#!/usr/bin/env bash
# Instala o Elo para o usuário atual: ~/.local/bin/elo e elo-server, atalho no
# menu de aplicativos e ícone. Não precisa de root.
#
# Funciona de dois jeitos:
#   - no código-fonte (scripts/instalar-linux.sh): compila em release e instala;
#   - dentro do pacote .tar.gz (./instalar.sh): instala os executáveis prontos.
#
# Uso: instalar-linux.sh            instala (ou atualiza)
#      instalar-linux.sh --remover  desinstala (mantém as configurações)
set -euo pipefail

ID=br.com.stringari.elo
AQUI="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
DADOS="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DADOS/applications"
ICONES="$DADOS/icons/hicolor/scalable/apps"

if [[ "${1:-}" == "--remover" ]]; then
    rm -f "$BIN/elo" "$BIN/elo-server" "$APPS/$ID.desktop" "$ICONES/$ID.svg"
    update-desktop-database "$APPS" 2>/dev/null || true
    echo "Elo removido (as configurações continuam em ~/.config/elo)."
    exit 0
fi

if [[ -x "$AQUI/elo" && -f "$AQUI/elo.svg" ]]; then
    # Pacote pronto.
    ORIGEM="$AQUI"
    ICONE="$AQUI/elo.svg"
else
    # Código-fonte: compila numa pasta própria (a mesma do Makefile), que não
    # se mistura com builds feitos em outro ambiente (ex.: VS Code em flatpak).
    RAIZ="$(cd "$AQUI/.." && pwd)"
    [[ -f "$RAIZ/Cargo.toml" ]] || { echo "Não encontrei os executáveis nem o código-fonte do Elo."; exit 1; }
    export PATH="$HOME/.cargo/bin:$PATH"
    command -v cargo >/dev/null || { echo "cargo não encontrado: instale o Rust (https://rustup.rs)"; exit 1; }
    export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RAIZ/target/host}"
    cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" -p elo -p elo-server
    ORIGEM="$CARGO_TARGET_DIR/release"
    ICONE="$RAIZ/packaging/icons/elo.svg"
fi

mkdir -p "$BIN" "$APPS" "$ICONES"
install -m 755 "$ORIGEM/elo" "$BIN/elo"
install -m 755 "$ORIGEM/elo-server" "$BIN/elo-server"
install -m 644 "$ICONE" "$ICONES/$ID.svg"
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
