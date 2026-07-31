; Install the bundled OpenMeter CLI on the current user's PATH. The helper
; performs an idempotent, exact path update so upgrades never duplicate it.
!macro NSIS_HOOK_POSTINSTALL
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\resources\path.ps1" -Action Install -BinDir "$INSTDIR\resources"' $0
  ${If} $0 != 0
    Abort "OpenMeter was installed, but its command-line PATH registration failed (exit $0)."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\resources\path.ps1" -Action Remove -BinDir "$INSTDIR\resources"' $0
!macroend
