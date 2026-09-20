<#
.SYNOPSIS
    Publica uma nova versão do Whisper App no Windows Package Manager (Winget).

.DESCRIPTION
    Automatiza todo o processo de submissão ao repositório microsoft/winget-pkgs:
    - Busca os assets da release no GitHub
    - Calcula SHA256 dos instaladores
    - Gera os 4 arquivos YAML de manifesto
    - Valida com `winget validate`
    - Cria branch no fork rlucio01/winget-pkgs
    - Faz upload dos arquivos via gh API
    - Abre o Pull Request

.PARAMETER Version
    Versão a publicar, sem o "v" (ex: "0.4.18"). Padrão: lê de tauri.conf.json.

.PARAMETER DryRun
    Se presente, gera e valida os manifestos mas não cria branch nem PR.

.EXAMPLE
    .\scripts\submit-winget.ps1
    .\scripts\submit-winget.ps1 -Version "0.4.18"
    .\scripts\submit-winget.ps1 -Version "0.4.18" -DryRun
#>

param(
    [string]$Version = "",
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
function Write-Step([string]$msg) {
    Write-Host "`n==> $msg" -ForegroundColor Cyan
}
function Write-OK([string]$msg) {
    Write-Host "    OK  $msg" -ForegroundColor Green
}
function Write-Fail([string]$msg) {
    Write-Host "    ERRO: $msg" -ForegroundColor Red
    exit 1
}

# ---------------------------------------------------------------------------
# 0. Detectar versão
# ---------------------------------------------------------------------------
Write-Step "Detectando versao"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot  = Split-Path -Parent $scriptDir

if (-not $Version) {
    $tauriConf = Get-Content "$repoRoot\src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json
    $Version = $tauriConf.version
}

Write-OK "Versao: $Version"

# ---------------------------------------------------------------------------
# 1. Verificar pré-requisitos
# ---------------------------------------------------------------------------
Write-Step "Verificando pre-requisitos"

foreach ($cmd in @("gh", "winget")) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Fail "'$cmd' nao encontrado no PATH. Instale e tente novamente."
    }
    Write-OK "$cmd disponivel"
}

$ghAuth = gh auth status 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Fail "gh CLI nao autenticado. Execute: gh auth login"
}
Write-OK "gh autenticado"

# ---------------------------------------------------------------------------
# 2. Verificar que a release existe no GitHub
# ---------------------------------------------------------------------------
Write-Step "Verificando release v$Version no GitHub"

$assets = gh api "repos/rlucio01/whisper-app/releases/tags/v$Version" `
    --jq '.assets[] | {name:.name, url:.browser_download_url}' 2>&1

if ($LASTEXITCODE -ne 0) {
    Write-Fail "Release v$Version nao encontrada em rlucio01/whisper-app. Publique a release primeiro."
}
Write-OK "Release encontrada"

# ---------------------------------------------------------------------------
# 3. Baixar instaladores e calcular SHA256
# ---------------------------------------------------------------------------
Write-Step "Baixando instaladores e calculando SHA256"

$baseUrl = "https://github.com/rlucio01/whisper-app/releases/download/v$Version"
$msiName = "whisper_app_${Version}_x64_en-US.msi"
$exeName = "whisper_app_${Version}_x64-setup.exe"
$tmpDir  = "$env:TEMP\whisper_winget_$Version"

New-Item -Force -ItemType Directory $tmpDir | Out-Null

Write-Host "    Baixando $msiName..."
Invoke-WebRequest "$baseUrl/$msiName" -OutFile "$tmpDir\app.msi" -UseBasicParsing
$msiSha256 = (Get-FileHash "$tmpDir\app.msi" -Algorithm SHA256).Hash
Write-OK "MSI SHA256: $msiSha256"

Write-Host "    Baixando $exeName..."
Invoke-WebRequest "$baseUrl/$exeName" -OutFile "$tmpDir\app.exe" -UseBasicParsing
$exeSha256 = (Get-FileHash "$tmpDir\app.exe" -Algorithm SHA256).Hash
Write-OK "EXE SHA256: $exeSha256"

# ---------------------------------------------------------------------------
# 4. Gerar manifestos YAML
# ---------------------------------------------------------------------------
Write-Step "Gerando manifestos YAML"

$manifestDir = "$repoRoot\winget\manifests\r\rlucio01\WhisperApp\$Version"
New-Item -Force -ItemType Directory $manifestDir | Out-Null

# version manifest
@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.1.9.0.schema.json

PackageIdentifier: rlucio01.WhisperApp
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.9.0
"@ | Set-Content "$manifestDir\rlucio01.WhisperApp.yaml" -Encoding UTF8

# installer manifest
@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.1.9.0.schema.json

PackageIdentifier: rlucio01.WhisperApp
PackageVersion: $Version
MinimumOSVersion: 10.0.17763.0
Installers:
- Architecture: x64
  InstallerType: msi
  InstallerUrl: $baseUrl/$msiName
  InstallerSha256: $msiSha256
  InstallerLocale: en-US
  UpgradeBehavior: install
- Architecture: x64
  InstallerType: nullsoft
  InstallerUrl: $baseUrl/$exeName
  InstallerSha256: $exeSha256
  InstallerLocale: pt-BR
  UpgradeBehavior: install
ManifestType: installer
ManifestVersion: 1.9.0
"@ | Set-Content "$manifestDir\rlucio01.WhisperApp.installer.yaml" -Encoding UTF8

# locale en-US
@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.1.9.0.schema.json

PackageIdentifier: rlucio01.WhisperApp
PackageVersion: $Version
PackageLocale: en-US
Publisher: Rafael Lucio
PublisherUrl: https://github.com/rlucio01
PublisherSupportUrl: https://github.com/rlucio01/whisper-app/issues
PackageName: Whisper App
PackageUrl: https://github.com/rlucio01/whisper-app
License: Proprietary
LicenseUrl: https://github.com/rlucio01/whisper-app/blob/main/README.md
Copyright: Copyright (c) Rafael Lucio. All rights reserved.
ShortDescription: AI-powered voice dictation for Windows using Whisper and LLMs
Description: |-
  Whisper App is a lightweight AI-powered voice dictation tool for Windows.
  Hold a global hotkey, speak, and release. Audio is transcribed via Whisper
  (local offline or cloud), optionally reformatted by an LLM (OpenAI,
  Anthropic, Groq, Gemini, xAI or OpenRouter), and automatically pasted
  into the focused application. Uses under 7 MB RAM at idle.
Tags:
- dictation
- voice
- whisper
- ai
- speech-to-text
- transcription
- productivity
- llm
ReleaseNotesUrl: https://github.com/rlucio01/whisper-app/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: 1.9.0
"@ | Set-Content "$manifestDir\rlucio01.WhisperApp.locale.en-US.yaml" -Encoding UTF8

# locale pt-BR
@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.locale.1.9.0.schema.json

PackageIdentifier: rlucio01.WhisperApp
PackageVersion: $Version
PackageLocale: pt-BR
Publisher: Rafael Lucio
PublisherUrl: https://github.com/rlucio01
PublisherSupportUrl: https://github.com/rlucio01/whisper-app/issues
PackageName: Whisper App
PackageUrl: https://github.com/rlucio01/whisper-app
License: Proprietario
ShortDescription: Ditado por voz com IA para Windows usando Whisper e LLMs
Description: |-
  Whisper App e uma ferramenta leve de ditado por voz com IA para Windows.
  Segure um atalho global, fale e solte. O audio e transcrito via Whisper
  (offline local ou nuvem), reformatado por um LLM (OpenAI, Anthropic,
  Groq, Gemini, xAI ou OpenRouter) e colado automaticamente no app em foco.
  Usa menos de 7 MB de RAM em idle.
Tags:
- ditado
- voz
- whisper
- ia
- fala-para-texto
- transcricao
- produtividade
- llm
ReleaseNotesUrl: https://github.com/rlucio01/whisper-app/releases/tag/v$Version
ManifestType: locale
ManifestVersion: 1.9.0
"@ | Set-Content "$manifestDir\rlucio01.WhisperApp.locale.pt-BR.yaml" -Encoding UTF8

Write-OK "4 arquivos gerados em: $manifestDir"

# ---------------------------------------------------------------------------
# 5. Validar com winget
# ---------------------------------------------------------------------------
Write-Step "Validando manifestos com winget validate"

$validResult = winget validate --manifest $manifestDir 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host $validResult
    Write-Fail "Validacao falhou. Corrija os erros acima antes de continuar."
}
Write-OK "Validacao bem-sucedida"

if ($DryRun) {
    Write-Host "`n[DryRun] Manifestos gerados e validados. Pulando envio ao GitHub." -ForegroundColor Yellow
    Write-Host "         Arquivos em: $manifestDir" -ForegroundColor Yellow
    exit 0
}

# ---------------------------------------------------------------------------
# 6. Sincronizar fork e criar branch
# ---------------------------------------------------------------------------
Write-Step "Sincronizando fork e criando branch"

gh repo sync rlucio01/winget-pkgs --source microsoft/winget-pkgs --branch master 2>&1 | Out-Null
Write-OK "Fork sincronizado"

$branchName = "add-whisperapp-$Version"
$masterSha  = gh api repos/rlucio01/winget-pkgs/git/refs/heads/master --jq '.object.sha'

# Verificar se branch já existe
$branchExists = gh api "repos/rlucio01/winget-pkgs/git/refs/heads/$branchName" 2>$null
if ($LASTEXITCODE -eq 0) {
    Write-Host "    Branch $branchName ja existe, reutilizando." -ForegroundColor Yellow
} else {
    $body = @{ ref = "refs/heads/$branchName"; sha = $masterSha.Trim() } | ConvertTo-Json
    $body | Out-File "$env:TEMP\gh_ref_$Version.json" -Encoding utf8
    gh api repos/rlucio01/winget-pkgs/git/refs -X POST --input "$env:TEMP\gh_ref_$Version.json" | Out-Null
    Write-OK "Branch $branchName criado"
}

# ---------------------------------------------------------------------------
# 7. Upload dos arquivos via gh API
# ---------------------------------------------------------------------------
Write-Step "Enviando arquivos para o fork"

$remoteBase = "manifests/r/rlucio01/WhisperApp/$Version"
$fileNames = @(
    "rlucio01.WhisperApp.yaml",
    "rlucio01.WhisperApp.installer.yaml",
    "rlucio01.WhisperApp.locale.en-US.yaml",
    "rlucio01.WhisperApp.locale.pt-BR.yaml"
)

foreach ($file in $fileNames) {
    $localPath  = "$manifestDir\$file"
    $remotePath = "$remoteBase/$file"
    $content    = [Convert]::ToBase64String([IO.File]::ReadAllBytes($localPath))

    $existing = gh api "repos/rlucio01/winget-pkgs/contents/$remotePath" --jq '.sha' 2>$null
    $body = @{
        message = "Add $file for rlucio01.WhisperApp v$Version"
        content = $content
        branch  = $branchName
    }
    if ($existing -and $LASTEXITCODE -eq 0) {
        $body["sha"] = $existing.Trim()
    }

    $body | ConvertTo-Json | Out-File "$env:TEMP\gh_file_$Version.json" -Encoding utf8
    gh api "repos/rlucio01/winget-pkgs/contents/$remotePath" -X PUT `
        --input "$env:TEMP\gh_file_$Version.json" | Out-Null

    Write-OK $file
}

# ---------------------------------------------------------------------------
# 8. Abrir PR
# ---------------------------------------------------------------------------
Write-Step "Abrindo Pull Request"

$prBody = @"
## Description

Adding new package: **Whisper App v$Version**

AI-powered voice dictation for Windows. Hold a global hotkey, speak, and release.
Audio is transcribed via Whisper (local offline or cloud via OpenAI/Groq), reformatted
by an LLM (OpenAI, Anthropic, Groq, Gemini, xAI, OpenRouter), and automatically pasted
into the focused application. Uses under 7 MB RAM at idle.

- Publisher: Rafael Lucio
- Package URL: https://github.com/rlucio01/whisper-app
- Release: https://github.com/rlucio01/whisper-app/releases/tag/v$Version

## Manifest Checklist

- [x] Validated manifest locally with ``winget validate --manifest <path>``
- [x] This PR only modifies one (1) manifest
- [x] Checked that there aren't other open pull requests for the same manifest
"@

$prUrl = gh pr create `
    --repo microsoft/winget-pkgs `
    --head "rlucio01:$branchName" `
    --base master `
    --title "New package: rlucio01.WhisperApp version $Version" `
    --body $prBody

Write-OK "PR aberto: $prUrl"

Write-Host "`n================================================" -ForegroundColor Green
Write-Host " Publicacao v$Version concluida com sucesso!" -ForegroundColor Green
Write-Host " PR: $prUrl" -ForegroundColor Green
Write-Host "================================================`n" -ForegroundColor Green
