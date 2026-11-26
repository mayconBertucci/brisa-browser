#define AppVersion "1.0.5"

[Setup]
AppId={{BB6FD759-7F2E-4520-AC26-CCD7100E0DFB}
AppName=Brisa
AppVersion={#AppVersion}
AppPublisher=Brisa
AppPublisherURL=https://mayconbertucci.github.io/brisa-browser/
AppUpdatesURL=https://github.com/mayconBertucci/brisa-browser/releases
DefaultDirName={autopf}\Brisa
DefaultGroupName=Brisa
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\brisa.exe
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UsePreviousPrivileges=no
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
WizardStyle=modern
SetupIconFile=..\assets\brisa.ico
OutputDir=..\target\distribution
OutputBaseFilename=Brisa-{#AppVersion}-Windows-x64
Compression=lzma2
SolidCompression=yes
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName=Brisa
VersionInfoCompany=Brisa
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; Flags: unchecked

[Files]
Source: "..\.toolchain\webview2\MicrosoftEdgeWebview2Setup.exe"; Flags: dontcopy
Source: "..\target\release\brisa.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\.toolchain\translation\translate.mjs"; DestDir: "{app}\translation"; Flags: ignoreversion
Source: "..\.toolchain\translation\translation-worker.mjs"; DestDir: "{app}\translation"; Flags: ignoreversion
Source: "..\.toolchain\translation\translation-models.json"; DestDir: "{app}\translation"; Flags: ignoreversion
Source: "..\.toolchain\translation\node_modules\@browsermt\bergamot-translator\*"; DestDir: "{app}\translation\node_modules\@browsermt\bergamot-translator"; Flags: ignoreversion recursesubdirs
Source: "..\target\distribution\notices\*"; DestDir: "{app}\Licencas"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Brisa"; Filename: "{app}\brisa.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\Brisa"; Filename: "{app}\brisa.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\brisa.exe"; Description: "Abrir o Brisa"; Flags: nowait postinstall runasoriginaluser skipifsilent unchecked

[Code]
function HasWebView2(): Boolean;
var
  Version: String;
  Key: String;
begin
  Key := 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  Result :=
    (RegQueryStringValue(HKLM32, Key, 'pv', Version) and
      (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, Key, 'pv', Version) and
      (Version <> '') and (Version <> '0.0.0.0'));
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Result := '';
  if HasWebView2() then
    Exit;
  WizardForm.PreparingLabel.Caption := 'Preparando o Microsoft Edge WebView2.';
  ExtractTemporaryFile('MicrosoftEdgeWebview2Setup.exe');
  if not Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'),
    '/silent /install', '', SW_HIDE, ewWaitUntilTerminated, ResultCode) or
    not HasWebView2() then
    Result := 'O Microsoft Edge WebView2 é necessário. Conecte-se à internet e tente novamente.';
end;
