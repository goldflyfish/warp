#ifndef PayloadDir
  #error PayloadDir is required
#endif
#ifndef PackageVersion
  #define PackageVersion "0.1.0"
#endif
#ifndef PackageArch
  #define PackageArch "x64"
#endif

[Setup]
AppId=goldflyfish-mywarp
AppName=MyWarp
AppVersion={#PackageVersion}
AppPublisher=MyWarp contributors
AppPublisherURL=https://github.com/goldflyfish/warp
DefaultDirName={localappdata}\Programs\MyWarp
DefaultGroupName=MyWarp
PrivilegesRequired=lowest
#if PackageArch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
MinVersion=10.0.17763
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\MyWarp.exe
SetupIconFile={#PayloadDir}\icon.ico
OutputBaseFilename=MyWarp-{#PackageVersion}-{#PackageArch}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
AppMutex=Local\MyWarpInstalled_SingleInstance
CloseApplications=yes
CloseApplicationsFilter=MyWarp.exe,opencode.exe

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\MyWarp"; Filename: "{app}\MyWarp.exe"
Name: "{autodesktop}\MyWarp"; Filename: "{app}\MyWarp.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\MyWarp.exe"; Description: "Launch MyWarp"; Flags: nowait postinstall skipifsilent

[Code]
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if FileExists(ExpandConstant('{app}\mywarp-portable')) then
    Result := 'This folder contains a portable MyWarp profile. Choose a different installation folder to keep its configuration separate.';
end;
