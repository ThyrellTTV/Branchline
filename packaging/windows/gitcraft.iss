; Inno Setup script for gitcraft's Windows installer. Built by the release workflow:
;   iscc /DAppVersion=0.2.0 /DOutputName=gitcraft-v0.2.0-windows-x86_64-setup packaging\windows\gitcraft.iss
; Installs per user (no administrator rights needed) unless the user picks "all users".

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef OutputName
  #define OutputName "gitcraft-setup"
#endif
#define Root "..\.."

[Setup]
AppId={{86F6EE51-215B-489E-BB29-402DC3170FC5}
AppName=gitcraft
AppVersion={#AppVersion}
AppVerName=gitcraft {#AppVersion}
AppPublisher=Iván
AppPublisherURL=https://github.com/Vancete/gitcraft
AppSupportURL=https://github.com/Vancete/gitcraft/issues
AppUpdatesURL=https://github.com/Vancete/gitcraft/releases
DefaultDirName={autopf}\gitcraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Root}\dist
OutputBaseFilename={#OutputName}
SetupIconFile={#Root}\assets\app-icon\gitcraft.ico
UninstallDisplayIcon={app}\gitcraft.exe
UninstallDisplayName=gitcraft
LicenseFile={#Root}\LICENSE-MIT
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=no

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "foldermenu"; Description: "Add ""Open in gitcraft"" to the right-click menu of folders"; GroupDescription: "Explorer:"

[Files]
Source: "{#Root}\target\release\gitcraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\ATTRIBUTION.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\gitcraft"; Filename: "{app}\gitcraft.exe"
Name: "{autodesktop}\gitcraft"; Filename: "{app}\gitcraft.exe"; Tasks: desktopicon

[Registry]
; "Open in gitcraft" on a folder and on a folder's background (HKCU or HKLM, per install mode).
Root: HKA; Subkey: "Software\Classes\Directory\shell\gitcraft"; ValueType: string; ValueName: ""; ValueData: "Open in gitcraft"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\gitcraft"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\gitcraft.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\gitcraft\command"; ValueType: string; ValueName: ""; ValueData: """{app}\gitcraft.exe"" ""%1"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\gitcraft"; ValueType: string; ValueName: ""; ValueData: "Open in gitcraft"; Tasks: foldermenu; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\gitcraft"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\gitcraft.exe"""; Tasks: foldermenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\gitcraft\command"; ValueType: string; ValueName: ""; ValueData: """{app}\gitcraft.exe"" ""%V"""; Tasks: foldermenu

[Run]
Filename: "{app}\gitcraft.exe"; Description: "{cm:LaunchProgram,gitcraft}"; Flags: nowait postinstall skipifsilent
