; Installeur Windows de DBphoque (Inno Setup 6).
; Compilé par la CI : iscc /DAppVersion=<version> /DSourceExe=<chemin du .exe> dbphoque.iss
; Installation par utilisateur par défaut : aucun droit administrateur requis.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\dbphoque.exe"
#endif

[Setup]
; Identifiant stable : ne jamais le changer, il relie les mises à jour à l'installation existante.
AppId={{108A4203-037E-4154-8CBE-272ACF2DECFC}
AppName=DBphoque
AppVersion={#AppVersion}
AppPublisher=gsoulat
AppPublisherURL=https://github.com/gsoulat/quarry
DefaultDirName={autopf}\DBphoque
DefaultGroupName=DBphoque
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=DBphoque-Setup
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\DBphoque.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "DBphoque.exe"; Flags: ignoreversion
Source: "..\..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\DBphoque"; Filename: "{app}\DBphoque.exe"; IconFilename: "{app}\icon.ico"
Name: "{autodesktop}\DBphoque"; Filename: "{app}\DBphoque.exe"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\DBphoque.exe"; Description: "{cm:LaunchProgram,DBphoque}"; Flags: nowait postinstall skipifsilent
