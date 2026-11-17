# Gera o pacote da Microsoft Store em dist\:
#   Elo-<versao>-windows-x64.msix
#
# Sem assinatura: a loja assina o pacote ao publicar. Para instalar fora da
# loja (teste), assine com um certificado proprio: MSIX_PFX e MSIX_PFX_SENHA,
# com o mesmo "CN=..." de MSIX_EDITOR.
#
# A identidade vem da reserva do nome no Partner Center (Produto > Gerenciamento
# do produto > Identidade do produto):
#   MSIX_NOME         Package/Identity/Name           (ex.: 12345StringariConsultoria.Elo)
#   MSIX_EDITOR       Package/Identity/Publisher      (ex.: CN=1A2B3C4D-...)
#   MSIX_EDITOR_NOME  Package/Properties/PublisherDisplayName
#   MSIX_NOME_EXIBIDO o nome reservado (padrao: Elo)
# Sem MSIX_NOME e MSIX_EDITOR, sai um pacote de teste (a loja recusa).
#
# Requer o Windows SDK (makeappx e makepri; ja vem no windows-latest do CI).
# Usa o target\release\elo.exe de scripts\pacote-windows.ps1 (ou compila).
# Uso: pwsh scripts/pacote-msix.ps1
$ErrorActionPreference = "Stop"

$raiz = Split-Path -Parent $PSScriptRoot
$versao = (Select-String -Path "$raiz\Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$dist = Join-Path $raiz "dist"
$rel = Join-Path $raiz "target\release"

if (-not (Test-Path "$rel\elo.exe")) {
    Push-Location $raiz
    try { cargo build --release --locked -p elo } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "falha ao compilar" }
}

# Ferramentas do Windows SDK (a versao mais nova instalada).
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1
if (-not $sdk) { throw "Windows SDK nao encontrado (makeappx.exe)" }
$bin = $sdk.DirectoryName

$nome = $env:MSIX_NOME
$editor = $env:MSIX_EDITOR
if (-not $nome -or -not $editor) {
    Write-Warning "MSIX_NOME/MSIX_EDITOR vazios: pacote so para teste local"
    $nome = "StringariConsultoria.Elo.Teste"
    $editor = "CN=Elo Teste"
}
$editorNome = if ($env:MSIX_EDITOR_NOME) { $env:MSIX_EDITOR_NOME } else { "Stringari Consultoria" }
$exibido = if ($env:MSIX_NOME_EXIBIDO) { $env:MSIX_NOME_EXIBIDO } else { "Elo" }

# Pasta do pacote.
$layout = Join-Path $raiz "target\msix"
if (Test-Path $layout) { Remove-Item -Recurse -Force $layout }
New-Item -ItemType Directory $layout | Out-Null
Copy-Item "$rel\elo.exe" $layout
Copy-Item "$raiz\LICENSE" "$layout\LICENSE.txt"
Copy-Item "$raiz\NOTICE" "$layout\NOTICE.txt"
Copy-Item -Recurse "$raiz\packaging\windows\msix\Assets" "$layout\Assets"
$esc = { param($s) [Security.SecurityElement]::Escape($s) }
(Get-Content -Raw -Encoding UTF8 "$raiz\packaging\windows\msix\AppxManifest.xml").
    Replace("@NOME@", (& $esc $nome)).
    Replace("@EDITOR@", (& $esc $editor)).
    Replace("@EDITOR_NOME@", (& $esc $editorNome)).
    Replace("@NOME_EXIBIDO@", (& $esc $exibido)).
    Replace("@VERSAO@", "$versao.0") |
    Set-Content -Encoding UTF8 "$layout\AppxManifest.xml"

# resources.pri: escolhe o icone do tamanho certo (barra de tarefas, Iniciar).
$tmp = Join-Path ([IO.Path]::GetTempPath()) "elo-msix-pri"
New-Item -ItemType Directory -Force $tmp | Out-Null
& "$bin\makepri.exe" createconfig /cf "$tmp\priconfig.xml" /dq pt-BR /pv 10.0.0 /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "falha no makepri createconfig" }
& "$bin\makepri.exe" new /pr $layout /cf "$tmp\priconfig.xml" /mn "$layout\AppxManifest.xml" /of "$layout\resources.pri" /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "falha no makepri new" }

New-Item -ItemType Directory -Force $dist | Out-Null
$msix = Join-Path $dist "Elo-$versao-windows-x64.msix"
& "$bin\makeappx.exe" pack /d $layout /p $msix /o
if ($LASTEXITCODE -ne 0) { throw "falha no makeappx" }

if ($env:MSIX_PFX) {
    & "$bin\signtool.exe" sign /fd SHA256 /f $env:MSIX_PFX /p $env:MSIX_PFX_SENHA $msix
    if ($LASTEXITCODE -ne 0) { throw "falha ao assinar" }
}
Write-Host "Gerado: $msix ($nome, $editor)"
