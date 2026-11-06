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

# ---- servidor avulso ----
$snome = "elo-server-$versao-windows-x64"
$stmp = Join-Path ([IO.Path]::GetTempPath()) $snome
if (Test-Path $stmp) { Remove-Item -Recurse -Force $stmp }
New-Item -ItemType Directory $stmp | Out-Null
Copy-Item "$rel\elo-server.exe", "$raiz\LICENSE-MIT", "$raiz\LICENSE-APACHE" $stmp
Copy-Item "$raiz\NOTICE" "$stmp\NOTICE.txt"
@"
Servidor do Elo $versao para Windows 10/11 e Windows Server (64 bits)

So rodar (num terminal, nesta pasta):
  .\elo-server.exe --listen 0.0.0.0:21116 --data .\dados --self-signed

Iniciar com o Windows (PowerShell como administrador; ajuste a pasta):
  schtasks /Create /TN "Elo Servidor" /SC ONSTART /RU SYSTEM /RL HIGHEST ``
    /TR "\"C:\Elo\elo-server.exe\" --listen 0.0.0.0:21116 --data C:\Elo\dados --self-signed"
Liberar a porta no firewall:
  New-NetFirewallRule -DisplayName "Elo Servidor" -Direction Inbound -Protocol TCP -LocalPort 21116 -Action Allow
Impressao digital para os apps:
  C:\Elo\elo-server.exe --impressao --data C:\Elo\dados

Detalhes: https://github.com/Stringari-Consultoria/elo#servidor
"@ | Set-Content -Encoding UTF8 "$stmp\LEIA-ME.txt"
$szip = Join-Path $dist "$snome.zip"
if (Test-Path $szip) { Remove-Item $szip }
Compress-Archive -Path "$stmp\*" -DestinationPath $szip
Remove-Item -Recurse -Force $stmp
Write-Host "Gerado: $szip"

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
