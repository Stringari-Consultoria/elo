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

AQUI="$(cd "$(dirname "$0")" && pwd)"
# Pacote do Elo Cliente: tem o executável elo-cliente (e não o elo).
if [[ -x "$AQUI/elo-cliente" || "${VARIANTE:-}" == cliente ]]; then
    COMENTARIO="Receba suporte remoto da sua empresa"; APP="Elo Cliente"; EXE=elo-cliente; ID=br.com.stringari.elo-cliente
else
    COMENTARIO="Acesse e controle outro computador"; APP="Elo"; EXE=elo; ID=br.com.stringari.elo
fi
BIN="$HOME/.local/bin"
DADOS="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DADOS/applications"
ICONES="$DADOS/icons/hicolor/scalable/apps"

if [[ "${1:-}" == "--remover" ]]; then
    rm -f "$BIN/$EXE" "$APPS/$ID.desktop" "$ICONES/$ID.svg"
    if [[ "$EXE" == elo ]]; then rm -f "$BIN/elo-server"; fi
    update-desktop-database "$APPS" 2>/dev/null || true
    echo "$APP removido (as configurações continuam em ~/.config)."
    exit 0
fi

if [[ -x "$AQUI/$EXE" && -f "$AQUI/elo.svg" ]]; then
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

if [[ -x /usr/bin/$EXE ]]; then
    echo "Aviso: o $APP também está instalado pelo .deb (/usr/bin/$EXE)."
    echo "A cópia em $BIN e o atalho dela passam na frente. Para usar só o .deb:"
    echo "  $0 --remover"
    echo
fi
mkdir -p "$BIN" "$APPS" "$ICONES"
if [[ -x "$ORIGEM/$EXE" ]]; then install -m 755 "$ORIGEM/$EXE" "$BIN/$EXE"; else install -m 755 "$ORIGEM/elo" "$BIN/$EXE"; fi
if [[ "$EXE" == elo ]]; then install -m 755 "$ORIGEM/elo-server" "$BIN/elo-server"; fi
install -m 644 "$ICONE" "$ICONES/$ID.svg"
cat > "$APPS/$ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP
GenericName=Acesso remoto
Comment=$COMENTARIO
Exec=$BIN/$EXE
Icon=$ID
Terminal=false
Categories=Network;RemoteAccess;
StartupWMClass=$ID
DESKTOP
update-desktop-database "$APPS" 2>/dev/null || true

echo
echo "Instalado:"
echo "  $BIN/$EXE"
if [[ "$EXE" == elo ]]; then echo "  $BIN/elo-server  (servidor avulso, opcional)"; fi
echo "Abra pelo menu de aplicativos (\"$APP\") ou rodando: $EXE"
