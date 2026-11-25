#ifndef MyAppVersion
  #define MyAppVersion "0.2.0"
#endif
#ifndef BinaryPath
  #define BinaryPath "..\..\target\release\textree.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "textree-v0.2.0-windows-x86_64-setup"
#endif

[Setup]
AppId={{D4E8A31F-7B52-49C6-9E10-8A2C6F5B19D4}
AppName=TexTree
AppVersion={#MyAppVersion}
AppVerName=TexTree {#MyAppVersion}
AppPublisher=Textic
AppPublisherURL=https://github.com/Textic/TexTree
AppSupportURL=https://github.com/Textic/TexTree/releases
DefaultDirName={autopf}\TexTree
DefaultGroupName=TexTree
AllowNoIcons=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile=..\..\assets\textree.ico
UninstallDisplayIcon={app}\textree.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
VersionInfoVersion={#MyAppVersion}.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinaryPath}"; DestDir: "{app}"; DestName: "textree.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\textree.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
Name: "{group}\TexTree"; Filename: "{app}\textree.exe"; IconFilename: "{app}\textree.exe"
Name: "{group}\{cm:UninstallProgram,TexTree}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\TexTree"; Filename: "{app}\textree.exe"; IconFilename: "{app}\textree.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\textree.exe"; Description: "{cm:LaunchProgram,TexTree}"; Flags: nowait postinstall skipifsilent
