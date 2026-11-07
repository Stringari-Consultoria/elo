; Instalador do Elo para Windows (Inno Setup 6).
; Gerado por scripts/pacote-windows.ps1, que passa:
;   /DAppVersion=0.1.0  /DBinDir=<pasta com elo.exe>  /DRaiz=<raiz do projeto>
;   e /DCliente para o Elo Cliente (elo-cliente.exe, outro AppId: os dois
;   podem ficar instalados na mesma maquina).
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

#ifdef Cliente
  #define AppNome "Elo Cliente"
  #define Exe "elo-cliente"
  #define Arquivo "Elo-Cliente"
  #define Guid "{{6F1E2C9A-3B7D-4E51-9A0C-2D8F4B6E7C13}"
#else
  #define AppNome "Elo"
  #define Exe "elo"
  #define Arquivo "Elo"
  #define Guid "{{ECA0F1DD-872B-4675-9906-F71D422B4FAE}"
#endif

[Setup]
AppId={#Guid}
AppName={#AppNome}
AppVersion={#AppVersion}
AppVerName={#AppNome} {#AppVersion}
AppPublisher=Stringari Consultoria
AppPublisherURL=https://github.com/Stringari-Consultoria/elo
AppSupportURL=https://github.com/Stringari-Consultoria/elo/issues
DefaultDirName={autopf}\{#AppNome}
DefaultGroupName={#AppNome}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#Raiz}\dist
OutputBaseFilename={#Arquivo}-{#AppVersion}-windows-x64-setup
SetupIconFile={#Raiz}\packaging\icons\elo.ico
UninstallDisplayIcon={app}\{#Exe}.exe
UninstallDisplayName={#AppNome}
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
Source: "{#BinDir}\{#Exe}.exe"; DestDir: "{app}"; Flags: ignoreversion
#ifndef Cliente
Source: "{#BinDir}\elo-server.exe"; DestDir: "{app}"; Flags: ignoreversion
#endif
Source: "{#Raiz}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#Raiz}\NOTICE"; DestDir: "{app}"; DestName: "NOTICE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppNome}"; Filename: "{app}\{#Exe}.exe"
Name: "{autodesktop}\{#AppNome}"; Filename: "{app}\{#Exe}.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\{#Exe}.exe"; Description: "{cm:LaunchProgram,{#AppNome}}"; Flags: nowait postinstall skipifsilent
; Atualizacao automatica (instalacao silenciosa): reabre o Elo.
Filename: "{app}\{#Exe}.exe"; Flags: nowait; Check: WizardSilent
