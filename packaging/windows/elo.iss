; Instalador do Elo para Windows (Inno Setup 6).
; Gerado por scripts/pacote-windows.ps1, que passa:
;   /DAppVersion=0.1.0  /DBinDir=<pasta com elo.exe>  /DRaiz=<raiz do projeto>
; Instala por usuario, sem pedir administrador (pode escolher "todos os
; usuarios" no inicio).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Raiz
  #define Raiz "..\.."
#endif
#ifndef BinDir
  #define BinDir Raiz + "\target\release"
#endif

[Setup]
AppId={{ECA0F1DD-872B-4675-9906-F71D422B4FAE}
AppName=Elo
AppVersion={#AppVersion}
AppVerName=Elo {#AppVersion}
AppPublisher=Stringari Consultoria
AppPublisherURL=https://github.com/Stringari-Consultoria/elo
AppSupportURL=https://github.com/Stringari-Consultoria/elo/issues
DefaultDirName={autopf}\Elo
DefaultGroupName=Elo
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Raiz}\dist
OutputBaseFilename=Elo-{#AppVersion}-windows-x64-setup
SetupIconFile={#Raiz}\packaging\icons\elo.ico
UninstallDisplayIcon={app}\elo.exe
UninstallDisplayName=Elo
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinDir}\elo.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\elo-server.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Raiz}\NOTICE"; DestDir: "{app}"; DestName: "NOTICE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Elo"; Filename: "{app}\elo.exe"
Name: "{autodesktop}\Elo"; Filename: "{app}\elo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\elo.exe"; Description: "{cm:LaunchProgram,Elo}"; Flags: nowait postinstall skipifsilent
; Atualizacao automatica (instalacao silenciosa): reabre o Elo.
Filename: "{app}\elo.exe"; Flags: nowait; Check: WizardSilent
