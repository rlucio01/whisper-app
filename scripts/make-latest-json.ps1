# Gera o latest.json que o auto-updater do app consome (ver
# `plugins.updater.endpoints` em tauri.conf.json). Rode isso DEPOIS de
# `.\scripts\dev.ps1 build` (que assina os instaladores) e ANTES de subir a
# release no GitHub — o latest.json precisa ir junto como asset, com o mesmo
# nome, pra URL `.../releases/latest/download/latest.json` funcionar.
#
# Uso:
#   .\scripts\make-latest-json.ps1 -Notes "Descrição da versão"
#
# A versão e a tag são lidas de `src-tauri/tauri.conf.json` (campo
# "version") — a tag da release precisa ser "v<version>" pro link de
# download abaixo bater com o que `gh release create` publica.

param(
    [Parameter(Mandatory = $true)]
    [string]$Notes
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$confPath = "src-tauri\tauri.conf.json"
$conf = Get-Content $confPath -Raw | ConvertFrom-Json
$version = $conf.version
$tag = "v$version"
$repo = "rlucio01/whisper-app"

# O updater do Windows usa o instalador NSIS (não o MSI) — é o formato que o
# plugin sabe rodar silenciosamente. `createUpdaterArtifacts: true` faz o
# `tauri build` gerar o `.sig` ao lado do `.exe` automaticamente.
#
# A pasta acumula builds antigas entre uma release e outra (o bundler não
# limpa sozinho) — filtramos pelo nome exato da versão atual em vez de pegar
# "o primeiro .sig encontrado", que já pegou um instalador de versão errada
# uma vez (v0.3.0 stale ao lado do v0.4.0 recém-buildado).
$nsisDir = "src-tauri\target\release\bundle\nsis"
$sigFile = Get-ChildItem -Path $nsisDir -Filter "*_${version}_*.exe.sig" -ErrorAction Stop | Select-Object -First 1
if (-not $sigFile) {
    throw "Nenhum .exe.sig da versao $version encontrado em $nsisDir - rode '.\scripts\dev.ps1 build' primeiro (com a chave do updater configurada)."
}

$exeName = $sigFile.Name -replace '\.sig$', ''
$signature = Get-Content $sigFile.FullName -Raw
$downloadUrl = "https://github.com/$repo/releases/download/$tag/$exeName"

$pubDate = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")

$manifest = [ordered]@{
    version   = $version
    notes     = $Notes
    pub_date  = $pubDate
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature.Trim()
            url       = $downloadUrl
        }
    }
}

$outPath = Join-Path $nsisDir "latest.json"
$json = $manifest | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText($outPath, $json, [System.Text.UTF8Encoding]::new($false))

Write-Host "==> latest.json gerado em $outPath" -ForegroundColor Cyan
Write-Host "    versao: $version | tag esperada: $tag | instalador: $exeName" -ForegroundColor Cyan
Write-Host "    Suba esse arquivo E o $exeName como assets da release $tag." -ForegroundColor Cyan
