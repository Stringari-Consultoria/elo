#!/usr/bin/env bash
# Gera dist/Elo-<versão>-macos.dmg com o Elo.app (universal: Apple Silicon e
# Intel). Só roda no macOS (usa lipo, sips, iconutil, codesign e hdiutil).
#
# A janela do .dmg (fundo com seta, posições dos ícones) é montada com o
# dmgbuild (instalado num venv temporário se faltar): packaging/macos/dmg.py.
#
# Variáveis opcionais:
#   UNIVERSAL=0                 só a arquitetura desta máquina (mais rápido)
#   ASSINATURA="Developer ID Application: Nome (TIME)"
#                               assina com o certificado da Apple (sem ela,
#                               assinatura ad hoc: o macOS pede "Abrir" com o
#                               botão direito na primeira vez)
#   NOTARIZAR=perfil            notariza com `xcrun notarytool` usando um perfil
#                               salvo com `notarytool store-credentials`
#   NOTARIZAR_KEYCHAIN=arquivo  keychain do perfil, se não for o padrão (CI)
#   VARIANTE=cliente            gera o Elo Cliente (Elo-Cliente-<versão>-macos.dmg)
set -euo pipefail

[[ "$(uname)" == Darwin ]] || { echo "pacote-macos.sh só roda no macOS (use o workflow de release do GitHub)."; exit 1; }

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
VERSAO="$(sed -n 's/^version = "\(.*\)"/\1/p' "$RAIZ/Cargo.toml" | head -1)"
DIST="$RAIZ/dist"
export PATH="$HOME/.cargo/bin:$PATH"
if [[ "${VARIANTE:-}" == cliente ]]; then
    APPNOME="Elo Cliente"; EXE=elo-cliente; BUNDLE=br.com.stringari.elo-cliente; ARQUIVO="Elo-Cliente"
else
    APPNOME="Elo"; EXE=elo; BUNDLE=br.com.stringari.elo; ARQUIVO="Elo"
fi
export MACOSX_DEPLOYMENT_TARGET=11.0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ---- executáveis ----
mkdir -p "$TMP/bin"
if [[ "${UNIVERSAL:-1}" == 1 ]]; then
    ALVOS=(aarch64-apple-darwin x86_64-apple-darwin)
    command -v rustup >/dev/null && rustup target add "${ALVOS[@]}" >/dev/null
    for t in "${ALVOS[@]}"; do
        cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" --target "$t" -p elo -p elo-server
    done
    for b in elo elo-server; do
        lipo -create -output "$TMP/bin/$b" "$RAIZ/target/aarch64-apple-darwin/release/$b" "$RAIZ/target/x86_64-apple-darwin/release/$b"
    done
else
    cargo build --release --locked --manifest-path "$RAIZ/Cargo.toml" -p elo -p elo-server
    cp "$RAIZ/target/release/elo" "$RAIZ/target/release/elo-server" "$TMP/bin/"
fi

# ---- Elo.app ----
APP="$TMP/dmg/$APPNOME.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 755 "$TMP/bin/elo" "$APP/Contents/MacOS/$EXE"
if [[ "$EXE" == elo ]]; then install -m 755 "$TMP/bin/elo-server" "$APP/Contents/MacOS/"; fi
sed -e "s/@VERSAO@/$VERSAO/g" -e "s#<string>Elo</string>#<string>$APPNOME</string>#g" -e "s#<string>br.com.stringari.elo</string>#<string>$BUNDLE</string>#" -e "s#<string>elo</string>#<string>$EXE</string>#" "$RAIZ/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

ICONSET="$TMP/Elo.iconset"
mkdir -p "$ICONSET"
PNG="$RAIZ/packaging/icons/elo-1024.png"
for s in 16 32 128 256 512; do
    sips -z $s $s "$PNG" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) "$PNG" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/$APPNOME.icns"

# ---- assinatura ----
if [[ -n "${ASSINATURA:-}" ]]; then
    if [[ -f "$APP/Contents/MacOS/elo-server" ]]; then codesign --force --options runtime --timestamp --sign "$ASSINATURA" "$APP/Contents/MacOS/elo-server"; fi
    codesign --force --options runtime --timestamp --sign "$ASSINATURA" "$APP"
else
    if [[ -f "$APP/Contents/MacOS/elo-server" ]]; then codesign --force --sign - "$APP/Contents/MacOS/elo-server"; fi
    codesign --force --sign - "$APP"
fi

# ---- .dmg ----
# Janela com fundo e posições (dmgbuild, sem depender do Finder). Se não der
# para instalar o dmgbuild, gera um .dmg simples com hdiutil.
mkdir -p "$DIST"
DMG="$DIST/$ARQUIVO-$VERSAO-macos.dmg"
rm -f "$DMG"
DMGBUILD="$(command -v dmgbuild || true)"
if [[ -z "$DMGBUILD" ]] && python3 -m venv "$TMP/venv" >/dev/null 2>&1 && "$TMP/venv/bin/pip" install -q dmgbuild >/dev/null 2>&1; then
    DMGBUILD="$TMP/venv/bin/dmgbuild"
fi
if [[ -n "$DMGBUILD" ]]; then
    "$DMGBUILD" -s "$RAIZ/packaging/macos/dmg.py" \
        -D app="$APP" -D fundo="$RAIZ/packaging/macos/dmg-fundo.png" -D icone="$APP/Contents/Resources/$APPNOME.icns" \
        "$APPNOME $VERSAO" "$DMG"
else
    echo "aviso: dmgbuild indisponível; gerando .dmg simples" >&2
    ln -s /Applications "$TMP/dmg/Aplicativos"
    hdiutil create -volname "$APPNOME $VERSAO" -srcfolder "$TMP/dmg" -ov -format UDZO "$DMG" >/dev/null
fi

# ---- servidor avulso (só no pacote do Elo) ----
if [[ "$EXE" == elo ]]; then
SRV="elo-server-$VERSAO-macos"
mkdir -p "$TMP/$SRV"
install -m 755 "$TMP/bin/elo-server" "$TMP/$SRV/"
if [[ -n "${ASSINATURA:-}" ]]; then
    codesign --force --options runtime --timestamp --sign "$ASSINATURA" "$TMP/$SRV/elo-server"
else
    codesign --force --sign - "$TMP/$SRV/elo-server"
fi
install -m 644 "$RAIZ/LICENSE-MIT" "$RAIZ/LICENSE-APACHE" "$RAIZ/NOTICE" "$TMP/$SRV/"
cat > "$TMP/$SRV/br.com.stringari.elo-server.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>br.com.stringari.elo-server</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/local/bin/elo-server</string>
        <string>--listen</string>
        <string>0.0.0.0:21116</string>
        <string>--data</string>
        <string>/Library/Application Support/Elo Server</string>
        <string>--self-signed</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>/Library/Logs/elo-server.log</string>
</dict>
</plist>
PLIST
cat > "$TMP/$SRV/LEIA-ME.txt" <<TXT
Servidor do Elo $VERSAO para macOS 11+ (Intel e Apple Silicon)

Só rodar:   ./elo-server --listen 0.0.0.0:21116 --data ./dados --self-signed
(se o macOS bloquear: xattr -d com.apple.quarantine elo-server)

Iniciar com o Mac (serviço do sistema):
  sudo cp elo-server /usr/local/bin/
  sudo cp br.com.stringari.elo-server.plist /Library/LaunchDaemons/
  sudo launchctl bootstrap system /Library/LaunchDaemons/br.com.stringari.elo-server.plist
Impressão digital para os apps:
  sudo /usr/local/bin/elo-server --impressao --data "/Library/Application Support/Elo Server"
Parar e remover:
  sudo launchctl bootout system/br.com.stringari.elo-server
  sudo rm /Library/LaunchDaemons/br.com.stringari.elo-server.plist /usr/local/bin/elo-server

Detalhes: https://github.com/Stringari-Consultoria/elo#servidor
TXT
tar -C "$TMP" -czf "$DIST/$SRV.tar.gz" "$SRV"
echo "Gerado: $DIST/$SRV.tar.gz"
fi

if [[ -n "${NOTARIZAR:-}" ]]; then
    [[ -n "${ASSINATURA:-}" ]] || { echo "NOTARIZAR exige ASSINATURA (Developer ID)."; exit 1; }
    codesign --force --timestamp --sign "$ASSINATURA" "$DMG"
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARIZAR" ${NOTARIZAR_KEYCHAIN:+--keychain "$NOTARIZAR_KEYCHAIN"} --wait
    xcrun stapler staple "$DMG"
fi
echo "Gerado: $DMG"
