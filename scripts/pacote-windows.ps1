# Gera os instaladores do Windows em dist\:
#   Elo-<versao>-windows-x64-setup.exe  (instalador, com atalho e desinstalador)
#   Elo-<versao>-windows-x64.zip        (portatil: so extrair e abrir o elo.exe)
#
# Requer Rust (rustup) e, para o instalador, Inno Setup 6
# (choco install innosetup). Sem o Inno Setup, gera so o .zip.
# Uso: pwsh scripts/pacote-windows.ps1
$ErrorActionPreference = "Stop"

$raiz = Split-Path -Parent $PSScriptRoot
$versao = (Select-String -Path "$raiz\Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$dist = Join-Path $raiz "dist"
$rel = Join-Path $raiz "target\release"

cargo build --release --locked --manifest-path "$raiz\Cargo.toml" -p elo -p elo-server
if ($LASTEXITCODE -ne 0) { throw "falha ao compilar" }
New-Item -ItemType Directory -Force $dist | Out-Null

# ---- .zip portatil ----
$nome = "Elo-$versao-windows-x64"
$tmp = Join-Path ([IO.Path]::GetTempPath()) $nome
if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
New-Item -ItemType Directory $tmp | Out-Null
Copy-Item "$rel\elo.exe", "$rel\elo-server.exe", "$raiz\LICENSE-MIT", "$raiz\LICENSE-APACHE" $tmp
Copy-Item "$raiz\NOTICE" "$tmp\NOTICE.txt"
$zip = Join-Path $dist "$nome.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$tmp\*" -DestinationPath $zip
Remove-Item -Recurse -Force $tmp
Write-Host "Gerado: $zip"

# ---- instalador ----
$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe" }
if (-not (Test-Path $iscc)) {
    Write-Warning "Inno Setup 6 nao encontrado: so o .zip foi gerado (choco install innosetup)"
    exit 0
}
& $iscc /Q "/DAppVersion=$versao" "/DBinDir=$rel" "/DRaiz=$raiz" "$raiz\packaging\windows\elo.iss"
if ($LASTEXITCODE -ne 0) { throw "falha no Inno Setup" }
Write-Host "Gerado: $(Join-Path $dist "$nome-setup.exe")"
