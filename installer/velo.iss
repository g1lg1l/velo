; Velo's Windows installer (Inno Setup 6). From Ravune's, trimmed. The release workflow builds it:
;   iscc /DVersion=0.1.0 installer\velo.iss   (after cargo build --release)
; → dist\velo-<version>-windows-x64-setup.exe

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{1E5DE42C-B97B-49BB-8A0C-8EBA9B6C7CE6}
AppName=Velo
AppVersion={#Version}
AppVerName=Velo {#Version}
AppPublisher=The Velo contributors
AppPublisherURL=https://github.com/g1lg1l/velo
AppSupportURL=https://github.com/g1lg1l/velo/issues
; for this user by default (AppData\Local\Programs, no admin): Velo can then update itself by
; swapping its files. "All users" (Program Files) stays a choice; those copies update by
; running the next installer.
DefaultDirName={autopf}\Velo
DefaultGroupName=Velo
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE-GPL
SetupIconFile=..\assets\icons\windows\velo.ico
UninstallDisplayIcon={app}\velo.exe
OutputDir=..\dist
OutputBaseFilename=velo-{#Version}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "association"; Description: "Open .velo projects with Velo"

[Files]
Source: "..\target\release\velo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\velo-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"
Source: "..\LICENSE-GPL"; DestDir: "{app}"
Source: "..\LICENSE-MIT"; DestDir: "{app}"
Source: "..\LICENSE-APACHE"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\Velo"; Filename: "{app}\velo.exe"
Name: "{autodesktop}\Velo"; Filename: "{app}\velo.exe"; Tasks: desktopicon

[Registry]
Root: HKA; Subkey: "Software\Classes\.velo"; ValueType: string; ValueName: ""; ValueData: "Velo.Project"; Flags: uninsdeletevalue; Tasks: association
Root: HKA; Subkey: "Software\Classes\Velo.Project"; ValueType: string; ValueName: ""; ValueData: "Velo project"; Flags: uninsdeletekey; Tasks: association
Root: HKA; Subkey: "Software\Classes\Velo.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\velo.exe,0"; Tasks: association
Root: HKA; Subkey: "Software\Classes\Velo.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\velo.exe"" ""%1"""; Tasks: association

[Run]
Filename: "{app}\velo.exe"; Description: "{cm:LaunchProgram,Velo}"; Flags: nowait postinstall skipifsilent
