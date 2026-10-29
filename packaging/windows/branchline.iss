; Inno Setup script for branchline's Windows installer. Built by the release workflow:
;   iscc /DAppVersion=0.2.0 /DOutputName=branchline-v0.2.0-windows-x86_64-setup packaging\windows\branchline.iss
; Installs per user (no administrator rights needed) unless the user picks "all users".

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef OutputName
  #define OutputName "branchline-setup"
#endif
#define Root "..\.."

[Setup]
AppId={{B728B913-A5DB-4775-9E37-AF5BE4B597B3}
AppName=branchline
AppVersion={#AppVersion}
AppVerName=branchline {#AppVersion}
AppPublisher=ThyrellTTV
AppPublisherURL=https://github.com/ThyrellTTV/Branchline
AppSupportURL=https://github.com/ThyrellTTV/Branchline/issues
AppUpdatesURL=https://github.com/ThyrellTTV/Branchline/releases
DefaultDirName={autopf}\branchline
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\assets\app-icon\branchline.ico
UninstallDisplayIcon={app}\branchline.exe
UninstallDisplayName=branchline
LicenseFile={#Root}\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "foldermenu"; Description: "Add ""Open in branchline"" to the right-click menu of folders"; GroupDescription: "Explorer:"

[Files]
Source: "{#Root}\target\release\branchline.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\ATTRIBUTION.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\branchline"; Filename: "{app}\branchline.exe"
Name: "{autodesktop}\branchline"; Filename: "{app}\branchline.exe"; Tasks: desktopicon

[Registry]
; "Open in branchline" on a folder and on a folder's background (HKCU or HKLM, per install mode).
Root: HKA; Subkey: "Software\Classes\Directory\shell\branchline"; ValueType: string; ValueName: ""; ValueData: "Open in branchline"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\branchline"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\branchline.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\branchline\command"; ValueType: string; ValueName: ""; ValueData: """{app}\branchline.exe"" ""%1"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\branchline"; ValueType: string; ValueName: ""; ValueData: "Open in branchline"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\branchline"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\branchline.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\branchline\command"; ValueType: string; ValueName: ""; ValueData: """{app}\branchline.exe"" ""%V"""; Tasks: foldermenu

[Run]
Filename: "{app}\branchline.exe"; Description: "{cm:LaunchProgram,branchline}"; Flags: nowait postinstall skipifsilent
