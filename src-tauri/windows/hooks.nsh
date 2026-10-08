; Tauri NSIS Installer Hooks
; Instala o Visual C++ Redistributable 2015-2022 (x64) se nao estiver presente.
; MSVCP140.dll e necessaria para executar o whisper_app.exe.
;
; O arquivo vc_redist.x64.exe e incluido como resource em tauri.conf.json e
; fica disponivel no installer NSIS em $INSTDIR\vc_redist.x64.exe.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Verificar se o VC++ Redistributable x64 ja esta instalado
  ReadRegDWord $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ${If} $0 != 1
    DetailPrint "Instalando Visual C++ Redistributable 2015-2022..."
    ${If} ${FileExists} "$INSTDIR\vc_redist.x64.exe"
      ExecWait '"$INSTDIR\vc_redist.x64.exe" /install /passive /norestart' $1
      Delete "$INSTDIR\vc_redist.x64.exe"
      ${If} $1 == 0
        DetailPrint "Visual C++ Redistributable instalado com sucesso."
      ${Else}
        DetailPrint "VCRedist: codigo de retorno $1."
      ${EndIf}
    ${EndIf}
  ${Else}
    DetailPrint "Visual C++ Redistributable ja esta instalado. Nenhuma acao necessaria."
    ; Remove o instalador temporario do diretorio final do app para economizar espaco
    ${If} ${FileExists} "$INSTDIR\vc_redist.x64.exe"
      Delete "$INSTDIR\vc_redist.x64.exe"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
!macroend
