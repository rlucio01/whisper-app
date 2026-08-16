# Wrapper para rodar `npm run tauri dev` com o ambiente completo do Windows.
#
# Por que este script existe:
#   - `whisper-rs` compila o whisper.cpp (C++) via CMake + Ninja, e precisa
#     do compilador MSVC no PATH (`cl.exe`) + libclang do LLVM (`LIBCLANG_PATH`).
#   - Esses paths não existem por padrão num PowerShell "normal" — precisam ser
#     ativados via DevShell do Visual Studio 2019 Build Tools.
#   - Depois de compilado uma vez, subsequente `cargo check` não precisa mais
#     desse env, mas o `cargo build` de release/dev do Tauri sim.
#
# Uso:
#   .\scripts\dev.ps1        → roda `npm run tauri dev`
#   .\scripts\dev.ps1 build  → roda `npm run tauri build` (release bundle)
#
# Caso você mova as instalações de VS/LLVM/Ninja pra outros paths, ajuste
# as constantes abaixo.

$ErrorActionPreference = "Stop"

# Paths detectados no seu ambiente. Ajuste se precisar.
$VsInstallPath   = "C:\Program Files (x86)\Microsoft Visual Studio\2019\BuildTools"
$LlvmPath        = "C:\Program Files\LLVM"
$CMakePath       = "C:\Program Files\CMake"
$NinjaPath       = "C:\Users\rafae\AppData\Local\Microsoft\WinGet\Packages\Ninja-build.Ninja_Microsoft.Winget.Source_8wekyb3d8bbwe"
$CargoBin        = Join-Path $env:USERPROFILE ".cargo\bin"

# 1. Ativa o ambiente do compilador MSVC (define cl.exe, INCLUDE, LIB etc.).
Import-Module (Join-Path $VsInstallPath "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $VsInstallPath -DevCmdArguments "-arch=x64 -host_arch=x64" -SkipAutomaticLocation | Out-Null

# 2. O DevShell seta VSINSTALLDIR, que CMake interpreta como "instance" — e
#    o generator Ninja não aceita instance. Removemos para não conflitar.
Remove-Item Env:VSINSTALLDIR -ErrorAction SilentlyContinue
Remove-Item Env:CMAKE_GENERATOR_INSTANCE -ErrorAction SilentlyContinue

# 3. Adiciona ao PATH: Cargo, CMake, LLVM (libclang.dll) e Ninja.
$env:PATH = "$CargoBin;$CMakePath\bin;$LlvmPath\bin;$NinjaPath;$env:PATH"

# 4. Env vars usadas pelos build scripts do whisper-rs-sys.
$env:LIBCLANG_PATH = "$LlvmPath\bin"
$env:CMAKE_GENERATOR = "Ninja"

# 5. Volta para a raiz do projeto (independentemente de onde o script foi chamado).
Set-Location (Split-Path -Parent $PSScriptRoot)

# 6. Decide entre dev (default) e build.
$mode = if ($args.Count -gt 0) { $args[0] } else { "dev" }

Write-Host ""
Write-Host "==> Env configurado: MSVC 2019 + LLVM + Ninja" -ForegroundColor Cyan
Write-Host "==> Rodando: npm run tauri $mode" -ForegroundColor Cyan
Write-Host ""

if ($mode -eq "build") {
    npm run tauri build
} else {
    npm run tauri dev
}
