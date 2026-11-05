#!/usr/bin/env bash
# Gera dist/Elo-<versão>-macos.dmg com o Elo.app (universal: Apple Silicon e
# Intel). Só roda no macOS (usa lipo, sips, iconutil, codesign e hdiutil).
#
# Variáveis opcionais:
#   UNIVERSAL=0                 só a arquitetura desta máquina (mais rápido)
#   ASSINATURA="Developer ID Application: Nome (TIME)"
#                               assina com o certificado da Apple (sem ela,
#                               assinatura ad hoc: o macOS pede "Abrir" com o
#                               botão direito na primeira vez)
#   NOTARIZAR=perfil            notariza com `xcrun notarytool` usando um perfil
#                               salvo com `notarytool store-credentials`
set -euo pipefail

[[ "$(uname)" == Darwin ]] || { echo "pacote-macos.sh só roda no macOS (use o workflow de release do GitHub)."; exit 1; }

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
VERSAO="$(sed -n 's/^version = "\(.*\)"/\1/p' "$RAIZ/Cargo.toml" | head -1)"
DIST="$RAIZ/dist"
export PATH="$HOME/.cargo/bin:$PATH"
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
APP="$TMP/dmg/Elo.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 755 "$TMP/bin/elo" "$TMP/bin/elo-server" "$APP/Contents/MacOS/"
sed "s/@VERSAO@/$VERSAO/g" "$RAIZ/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

ICONSET="$TMP/Elo.iconset"
mkdir -p "$ICONSET"
PNG="$RAIZ/packaging/icons/elo-1024.png"
for s in 16 32 128 256 512; do
    sips -z $s $s "$PNG" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s * 2)) $((s * 2)) "$PNG" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Elo.icns"

# ---- assinatura ----
if [[ -n "${ASSINATURA:-}" ]]; then
    codesign --force --options runtime --timestamp --sign "$ASSINATURA" "$APP/Contents/MacOS/elo-server"
    codesign --force --options runtime --timestamp --sign "$ASSINATURA" "$APP"
else
    codesign --force --sign - "$APP/Contents/MacOS/elo-server"
    codesign --force --sign - "$APP"
fi

# ---- .dmg ----
ln -s /Applications "$TMP/dmg/Aplicativos"
mkdir -p "$DIST"
DMG="$DIST/Elo-$VERSAO-macos.dmg"
rm -f "$DMG"
hdiutil create -volname "Elo $VERSAO" -srcfolder "$TMP/dmg" -ov -format UDZO "$DMG" >/dev/null

if [[ -n "${NOTARIZAR:-}" ]]; then
    [[ -n "${ASSINATURA:-}" ]] || { echo "NOTARIZAR exige ASSINATURA (Developer ID)."; exit 1; }
    codesign --force --timestamp --sign "$ASSINATURA" "$DMG"
    xcrun notarytool submit "$DMG" --keychain-profile "$NOTARIZAR" --wait
    xcrun stapler staple "$DMG"
fi
echo "Gerado: $DMG"
