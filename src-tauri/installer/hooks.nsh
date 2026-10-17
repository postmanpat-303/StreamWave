; StreamWave NSIS hooks (Tauri bundle.windows.nsis.installerHooks)

; Tell Explorer that icons changed and refresh the per-user icon cache, so shortcuts
; don't keep showing an old cached icon from a previous build.
!macro StreamWaveRefreshIcons
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)' ; SHCNE_ASSOCCHANGED, SHCNF_FLUSH
  ${If} ${FileExists} "$SYSDIR\ie4uinit.exe"
    nsExec::Exec '"$SYSDIR\ie4uinit.exe" -show'
    Pop $0
  ${EndIf}
!macroend

; Re-point an existing shortcut at the exe with an explicit icon (fixes shortcuts from
; earlier installs that had IconLocation ",0").
!macro StreamWaveFixLnk LNK
  ${If} ${FileExists} "${LNK}"
    CreateShortcut "${LNK}" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\streamwave.ico" 0
    !insertmacro SetLnkAppUserModelId "${LNK}"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro StreamWaveFixLnk "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro StreamWaveFixLnk "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !insertmacro StreamWaveFixLnk "$DESKTOP\${PRODUCTNAME}.lnk"
  !insertmacro StreamWaveRefreshIcons
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro StreamWaveRefreshIcons
!macroend
