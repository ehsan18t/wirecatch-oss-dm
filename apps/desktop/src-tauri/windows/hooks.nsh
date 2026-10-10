; Wirecatch hooks for the Tauri NSIS installer (per-user, no administrator rights).
;
; The app registers the browser bridge for the current user on every start (D19), so the
; installer itself registers nothing. Uninstalling removes that registration before the files
; go, so no browser is left pointing at a bridge that no longer exists.

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\wirecatch-bridge.exe" 0 +2
    nsExec::Exec '"$INSTDIR\wirecatch-bridge.exe" uninstall'
!macroend
