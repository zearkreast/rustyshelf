; RustyShelf's Windows installer (Inno Setup 6), built by build.yml and alpha.yml:
;   ISCC /DAppVersion=<version> /DExe=<path to rustyshelf.exe> /DOutName=<file name> /DOutDir=<folder> setup.iss
; Installed for the user alone, in %LOCALAPPDATA%\Programs\RustyShelf: the app keeps its data folder beside the exe,
; which Program Files wouldn't let it write. Uninstalling leaves that data folder.

[Setup]
AppId={{225EB112-B3D7-4FF3-8D99-572EB77788B4}
AppName=RustyShelf
AppVersion={#AppVersion}
AppVerName=RustyShelf {#AppVersion}
AppPublisher=RustyShelf
AppPublisherURL=https://github.com/zearkreast/rustyshelf
DefaultDirName={localappdata}\Programs\RustyShelf
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE
SetupIconFile=..\..\crates\rustyshelf\icon.ico
UninstallDisplayIcon={app}\RustyShelf.exe
OutputDir={#OutDir}
OutputBaseFilename={#OutName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Dirs]
Name: "{app}\data"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "RustyShelf.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\RustyShelf"; Filename: "{app}\RustyShelf.exe"
Name: "{autodesktop}\RustyShelf"; Filename: "{app}\RustyShelf.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\RustyShelf.exe"; Description: "{cm:LaunchProgram,RustyShelf}"; Flags: nowait postinstall skipifsilent
