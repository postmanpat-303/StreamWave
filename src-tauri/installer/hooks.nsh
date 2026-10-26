; StreamWave NSIS hooks (Tauri bundle.windows.nsis.installerHooks), used with the stock Tauri template.
; Kept deliberately minimal: plain NSIS instructions only (no plugins, no external programs).
!include LogicLib.nsh

; Point existing shortcuts at an explicit icon file. A new IconLocation also makes Explorer
; drop any stale cached icon from earlier builds.
!macro StreamWaveFixLnk LNK
  ${If} ${FileExists} "${LNK}"
    CreateShortcut "${LNK}" "$INSTDIR\${MAINBINARYNAME}.exe" "" "$INSTDIR\streamwave.ico" 0
  ${EndIf}
!macroend

; 0.11.2 and older installed into %LOCALAPPDATA%\StreamWave, for one user. There is one
; StreamWave now, in Program Files: remove the old copy so two don't sit side by side.
; Settings, presets and recordings live outside that folder and are kept.
!macro NSIS_HOOK_PREINSTALL
  ${If} "$INSTDIR" != "$LOCALAPPDATA\${PRODUCTNAME}"
  ${AndIf} ${FileExists} "$LOCALAPPDATA\${PRODUCTNAME}"
    RMDir /r "$LOCALAPPDATA\${PRODUCTNAME}"
  ${EndIf}
  DeleteRegKey HKCU "${UNINSTKEY}"
  DeleteRegKey HKCU "${MANUPRODUCTKEY}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro StreamWaveFixLnk "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !insertmacro StreamWaveFixLnk "$DESKTOP\${PRODUCTNAME}.lnk"
!macroend
