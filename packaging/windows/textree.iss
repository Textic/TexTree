#ifndef MyAppVersion
  #define MyAppVersion "0.3.1"
#endif
#ifndef BinaryPath
  #define BinaryPath "..\..\target\release\textree.exe"
#endif
#ifndef ServiceBinaryPath
  #define ServiceBinaryPath "..\..\target\release\textree-service.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "textree-v0.3.1-windows-x86_64-setup"
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
PrivilegesRequired=admin
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
Name: "mftservice"; Description: "Install TexTree Read-Only MFT Service (instant NTFS scans without UAC prompts)"; GroupDescription: "Performance & Privileges:"; Check: IsAdminInstallMode
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#BinaryPath}"; DestDir: "{app}"; DestName: "textree.exe"; Flags: ignoreversion
Source: "{#ServiceBinaryPath}"; DestDir: "{app}"; DestName: "textree-service.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\textree.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
Name: "{group}\TexTree"; Filename: "{app}\textree.exe"; IconFilename: "{app}\textree.exe"
Name: "{group}\{cm:UninstallProgram,TexTree}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\TexTree"; Filename: "{app}\textree.exe"; IconFilename: "{app}\textree.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\textree-service.exe"; Parameters: "--install"; StatusMsg: "Installing TexTree Read-Only MFT Service..."; Flags: runhidden waituntilterminated; Tasks: mftservice; Check: IsAdminInstallMode
Filename: "{app}\textree-service.exe"; Parameters: "--restart-if-installed"; Flags: runhidden waituntilterminated; Check: IsAdminInstallMode
Filename: "{app}\textree.exe"; Description: "{cm:LaunchProgram,TexTree}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\textree-service.exe"; Parameters: "--uninstall"; RunOnceId: "UninstallTexTreeMftService"; Flags: runhidden waituntilterminated; Check: IsAdminInstallMode

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Result := '';
  // Stop any running TexTreeMftService cleanly before replacing textree-service.exe on upgrade.
  if IsAdminInstallMode then
  begin
    Exec(ExpandConstant('{sys}\sc.exe'), 'stop TexTreeMftService', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(500);
  end;
end;
