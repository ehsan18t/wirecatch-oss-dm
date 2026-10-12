; Generated from config/templates/hooks.nsh and config/product.toml by `pnpm product:generate`. Do not edit: change those and run it again.
; Wirecatch hooks for the Tauri NSIS installer (per-machine, in Program Files; D25).
;
; The installer runs with administrator rights, so it registers the browser bridge for every user of the computer (manifests next to the bridge, keys under HKEY_LOCAL_MACHINE). The app's own per-user registration on each start then steps aside for it, so no user is left with a registration of their own that an uninstall could not reach.
;
; A real uninstall removes the machine registration before the files go, so no browser is left pointing at a bridge that no longer exists. It also removes a per-user registration and the start-at-sign-in entry, but only for the account running the uninstaller: Windows gives an installer no way into other users' settings, and those are why the app does not register per user under a per-machine install. An update runs the old uninstaller in update mode; then nothing is removed, and the new installer registers again.
;
; These macros expand inside the installer's sections, where Tauri's defines (UNINSTKEY, MANUPRODUCTKEY) exist; this file itself is included before them.

!macro NSIS_HOOK_POSTINSTALL
  ; An earlier build installed Wirecatch for one account, in its own folder. Left in place, that copy keeps registering the bridge for its account, which browsers read before the machine registration, and keeps starting at sign-in beside this one, so it is removed. Its uninstaller takes its registration and start-at-sign-in entry with it; this copy writes its own again on its first start. Only the account running this installer is reached (PACKAGING.md).
  ; That installer recorded itself under the same keys as this one, in the account's registry instead of the machine's: the uninstaller (quoted) under UNINSTKEY, the folder (plain) under MANUPRODUCTKEY.
  ReadRegStr $R0 HKCU "${UNINSTKEY}" "UninstallString"
  ReadRegStr $R1 HKCU "${MANUPRODUCTKEY}" ""
  ${If} $R0 != ""
  ${AndIf} $R1 != ""
  ${AndIf} $R1 != $INSTDIR
    DetailPrint "Removing the copy of Wirecatch installed for this account only, in $R1"
    ; `_?=` runs the uninstaller in place, so this waits for it; it then leaves only itself behind.
    ClearErrors
    ExecWait '$R0 /S _?=$R1' $0
    ${IfThen} ${Errors} ${|} StrCpy $0 "error" ${|}
    ${If} $0 == 0
      Delete "$R1\uninstall.exe"
      RMDir "$R1"
    ${Else}
      DetailPrint "The earlier copy could not be removed (exit code $0); remove it in Settings, Apps."
    ${EndIf}
  ${EndIf}

  ${If} ${FileExists} "$INSTDIR\wirecatch-bridge.exe"
    nsExec::ExecToLog '"$INSTDIR\wirecatch-bridge.exe" install --all-users'
    ; nsExec leaves the exit code on the stack, or "error" when the bridge could not run at all.
    Pop $0
    ${If} $0 != 0
      DetailPrint "The browser extension could not be connected for every user (wirecatch-bridge exit code $0)."
      MessageBox MB_OK|MB_ICONEXCLAMATION "Wirecatch is installed, but the browser extension could not be connected to it for every user of this computer. Run this installer again to try once more." /SD IDOK
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\wirecatch-bridge.exe"
      nsExec::ExecToLog '"$INSTDIR\wirecatch-bridge.exe" uninstall --all-users'
      Pop $0
      ${If} $0 != 0
        DetailPrint "The browser registration for every user could not be removed (wirecatch-bridge exit code $0)."
      ${EndIf}
      nsExec::ExecToLog '"$INSTDIR\wirecatch-bridge.exe" uninstall'
      Pop $0
    ${EndIf}
    ; Written by tauri-plugin-autostart under the product name, for the account running the uninstaller.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Wirecatch"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Wirecatch"
  ${EndIf}
!macroend
