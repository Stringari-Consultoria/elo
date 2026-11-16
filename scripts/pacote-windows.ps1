# Gera os instaladores do Windows em dist\:
#   Elo-<versao>-windows-x64-setup.exe  (instalador, com atalho e desinstalador)
#   Elo-<versao>-windows-x64.zip        (portatil: so extrair e abrir o elo.exe)
#
# Requer Rust (rustup) e, para o instalador, Inno Setup 6
# (choco install innosetup). Sem o Inno Setup, gera so o .zip.
# Uso: pwsh scripts/pacote-windows.ps1
#      $env:VARIANTE="cliente"; pwsh scripts/pacote-windows.ps1   (Elo Cliente)
$ErrorActionPreference = "Stop"
$cliente = $env:VARIANTE -eq "cliente"
$exe = if ($cliente) { "elo-cliente" } else { "elo" }
$prefixo = if ($cliente) { "Elo-Cliente" } else { "Elo" }

$raiz = Split-Path -Parent $PSScriptRoot
$versao = (Select-String -Path "$raiz\Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$dist = Join-Path $raiz "dist"
$rel = Join-Path $raiz "target\release"

# Da raiz: o Cargo lê o .cargo\config.toml a partir da pasta atual (runtime
# do Visual C++ estático).
Push-Location $raiz
try { cargo build --release --locked -p elo -p elo-server } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw "falha ao compilar" }
# Num Windows recém-instalado não existe VCRUNTIME140.dll: se o executável
# depender dela, o Elo nem abre. Os runners do CI têm a DLL, então os testes
# não pegam isso; esta checagem pega.
function Get-PeImports([string]$arq) {
    $d = [IO.File]::ReadAllBytes($arq)
    $pe = [BitConverter]::ToInt32($d, 0x3c)
    $nsec = [BitConverter]::ToUInt16($d, $pe + 6)
    $opt = $pe + 24
    $optsz = [BitConverter]::ToUInt16($d, $pe + 20)
    $impRva = [BitConverter]::ToUInt32($d, $opt + 120)   # PE32+: diretório 1 (imports)
    $secs = 0..($nsec - 1) | ForEach-Object {
        $s = $opt + $optsz + $_ * 40
        [pscustomobject]@{ va = [BitConverter]::ToUInt32($d, $s + 12); tam = [Math]::Max([BitConverter]::ToUInt32($d, $s + 8), [BitConverter]::ToUInt32($d, $s + 16)); raw = [BitConverter]::ToUInt32($d, $s + 20) }
    }
    $off = { param($rva) foreach ($s in $secs) { if ($rva -ge $s.va -and $rva -lt $s.va + $s.tam) { return [int]($rva - $s.va + $s.raw) } } }
    $o = & $off $impRva
    $nomes = @()
    while ($true) {
        $nomeRva = [BitConverter]::ToUInt32($d, $o + 12)
        if ($nomeRva -eq 0) { break }
        $n = & $off $nomeRva
        $fim = [Array]::IndexOf($d, [byte]0, $n)
        $nomes += [Text.Encoding]::ASCII.GetString($d, $n, $fim - $n)
        $o += 20
    }
    $nomes
}
foreach ($b in "elo.exe", "elo-server.exe") {
    $ruins = Get-PeImports "$rel\$b" | Where-Object { $_ -match '(?i)^(vcruntime|msvcp)\d+' }
    if ($ruins) { throw "$b depende do runtime do Visual C++ ($($ruins -join ', ')): falta +crt-static" }
    Write-Host "$b sem dependência do runtime do Visual C++"
}
New-Item -ItemType Directory -Force $dist | Out-Null
# Elo Cliente: o mesmo executavel, com outro nome.
if ($cliente) { Copy-Item "$rel\elo.exe" "$rel\elo-cliente.exe" -Force }

# ---- .zip portatil ----
$nome = "$prefixo-$versao-windows-x64"
$tmp = Join-Path ([IO.Path]::GetTempPath()) $nome
if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
New-Item -ItemType Directory $tmp | Out-Null
# (Lista montada item a item: no PowerShell, um "if" com um item só vira texto.)
$itens = @("$rel\$exe.exe", "$raiz\LICENSE", "$raiz\NOTICE")
if (-not $cliente) { $itens += "$rel\elo-server.exe" }
Copy-Item $itens $tmp
Copy-Item "$raiz\NOTICE" "$tmp\NOTICE.txt"
$zip = Join-Path $dist "$nome.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$tmp\*" -DestinationPath $zip
Remove-Item -Recurse -Force $tmp
Write-Host "Gerado: $zip"

# ---- servidor avulso (so no pacote do Elo) ----
if (-not $cliente) {
$snome = "elo-server-$versao-windows-x64"
$stmp = Join-Path ([IO.Path]::GetTempPath()) $snome
if (Test-Path $stmp) { Remove-Item -Recurse -Force $stmp }
New-Item -ItemType Directory $stmp | Out-Null
Copy-Item "$rel\elo-server.exe", "$raiz\LICENSE", "$raiz\NOTICE" $stmp
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
}

# ---- instalador ----
$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) { $iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe" }
if (-not (Test-Path $iscc)) {
    Write-Warning "Inno Setup 6 nao encontrado: so o .zip foi gerado (choco install innosetup)"
    exit 0
}
$extra = @()
if ($cliente) { $extra += "/DCliente" }
& $iscc /Q "/DAppVersion=$versao" "/DBinDir=$rel" "/DRaiz=$raiz" @extra "$raiz\packaging\windows\elo.iss"
if ($LASTEXITCODE -ne 0) { throw "falha no Inno Setup" }
Write-Host "Gerado: $(Join-Path $dist "$nome-setup.exe")"
