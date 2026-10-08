; Tauri NSIS Installer Hooks
; Instala o Visual C++ Redistributable 2015-2022 (x64) se nao estiver presente.
; MSVCP140.dll e necessaria para executar o whisper_app.exe.

!macro NSIS_HOOK_PREINSTALL
  ; Verificar se o VC++ Redistributable x64 ja esta instalado
  ReadRegDWord $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ${If} $0 != 1
    DetailPrint "Instalando Visual C++ Redistributable 2015-2022..."
    SetOutPath "$TEMP"
    File "${BUILDRESOURCES_DIR}\vc_redist.x64.exe"
    ExecWait '"$TEMP\vc_redist.x64.exe" /install /passive /norestart' $0
    Delete "$TEMP\vc_redist.x64.exe"
    ${If} $0 != 0
      DetailPrint "VCRedist instalado com codigo: $0 (pode ja estar instalado)"
    ${Else}
      DetailPrint "Visual C++ Redistributable instalado com sucesso."
    ${EndIf}
  ${Else}
    DetailPrint "Visual C++ Redistributable ja esta instalado."
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
