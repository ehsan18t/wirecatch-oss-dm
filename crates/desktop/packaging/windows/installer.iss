; The Windows installer (D25, D106): per machine, in Program Files, built with Inno Setup.
; scripts/package.mjs compiles it and passes the product identity from config/product.toml as
; /D defines, with the folder holding the staged files (Stage) and where to write the installer.

#ifndef Stage
  #error Build this installer with scripts/package.mjs.
#endif

[Setup]
; The identifier names this install for upgrades and the uninstaller, so it never changes.
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#Website}
AppSupportURL={#Repository}/issues
AppUpdatesURL={#Repository}/releases
VersionInfoVersion={#FileVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} installer
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed={#Arch}
ArchitecturesInstallIn64BitMode={#Arch}
MinVersion=10.0
; Restart Manager closes a running copy before its files are replaced; it holds no unsaved work.
CloseApplications=force
RestartApplications=no
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\{#AppExe}.exe
UninstallDisplayName={#AppName}
WizardStyle=modern dynamic
WizardImageFile=wizard-sidebar.bmp,wizard-sidebar-200.bmp
WizardSmallImageFile=wizard-small-100.bmp,wizard-small-125.bmp,wizard-small-150.bmp,wizard-small-200.bmp,wizard-small-250.bmp
WizardImageAlphaFormat=defined
Compression=lzma2/max
SolidCompression=yes
OutputDir={#Output}
OutputBaseFilename={#OutputName}

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#Stage}\{#AppExe}.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\{#BridgeExe}.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}.exe"; Tasks: desktopicon

[Run]
; Started as the person who ran the installer, never elevated (D25.4).
Filename: "{app}\{#AppExe}.exe"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';

function RunBridge(Params: String): Integer;
var
  Code: Integer;
begin
  if not Exec(ExpandConstant('{app}\{#BridgeExe}.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    Code := -1;
  Result := Code;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
begin
  if CurStep <> ssPostInstall then
    exit;
  // Every user's browsers reach the bridge in Program Files, which only administrators can change (D25.2).
  Code := RunBridge('install --all-users');
  if Code <> 0 then
  begin
    Log(Format('%s install --all-users exited with %d', ['{#BridgeExe}', Code]));
    SuppressibleMsgBox('{#AppName} is installed, but the browser extension could not be connected to it for every user of this computer. Run this installer again to try once more.', mbError, MB_OK, IDOK);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep <> usUninstall then
    exit;
  // Before the files go, so no browser is left pointing at a bridge that no longer exists. Only the
  // account running the uninstaller can be reached for the per-user parts.
  RunBridge('uninstall --all-users');
  RunBridge('uninstall');
  RegDeleteValue(HKEY_CURRENT_USER, RunKey, '{#AppName}');
  RegDeleteValue(HKEY_CURRENT_USER, ApprovedKey, '{#AppName}');
end;
