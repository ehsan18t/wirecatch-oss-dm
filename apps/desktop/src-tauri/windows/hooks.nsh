; Wirecatch hooks for the Tauri NSIS installer (per-machine, in Program Files; D25).
;
; The installer runs with administrator rights, so it registers the browser bridge for every user of the computer (manifests next to the bridge, keys under HKEY_LOCAL_MACHINE). The app's own per-user registration on each start then steps aside for it, so no user is left with a registration of their own that an uninstall could not reach.
;
; A real uninstall removes the machine registration before the files go, so no browser is left pointing at a bridge that no longer exists. It also removes a per-user registration and the start-at-sign-in entry, but only for the account running the uninstaller: Windows gives an installer no way into other users' settings, and those are why the app does not register per user under a per-machine install. An update runs the old uninstaller in update mode; then nothing is removed, and the new installer registers again.

!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\wirecatch-bridge.exe"
    nsExec::Exec '"$INSTDIR\wirecatch-bridge.exe" install --all-users'
    ; nsExec leaves the exit code on the stack. A failure leaves the extension unable to reach the app until the next install or repair, and the bridge's own message is in the install log.
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\wirecatch-bridge.exe"
      nsExec::Exec '"$INSTDIR\wirecatch-bridge.exe" uninstall --all-users'
      Pop $0
      nsExec::Exec '"$INSTDIR\wirecatch-bridge.exe" uninstall'
      Pop $0
    ${EndIf}
    ; Written by tauri-plugin-autostart under the product name, for the account running the uninstaller.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wirecatch"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Wirecatch"
  ${EndIf}
!macroend
