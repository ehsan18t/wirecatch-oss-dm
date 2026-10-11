; Wirecatch hooks for the Tauri NSIS installer (per-user, no administrator rights).
;
; The app registers the browser bridge for the current user on every start (D19), so the
; installer itself registers nothing. A real uninstall removes that registration before the
; files go, so no browser is left pointing at a bridge that no longer exists, and removes the
; start-at-sign-in entry the app may have made. An update runs the old uninstaller in update
; mode; then nothing is removed, because the new version registers itself again on its next
; start and keeps the person's start-at-sign-in choice.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\wirecatch-bridge.exe"
      nsExec::Exec '"$INSTDIR\wirecatch-bridge.exe" uninstall'
      ; nsExec leaves the exit code on the stack; a failure only leaves a dead registration.
      Pop $0
    ${EndIf}
    ; Written by tauri-plugin-autostart under the product name.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wirecatch"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Wirecatch"
  ${EndIf}
!macroend
