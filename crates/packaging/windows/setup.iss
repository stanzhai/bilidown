; Inno Setup script for the Rust build (ticket 29).
;
; Same application identity as the Python installer: identical AppId,
; AppMutex (the single-instance named mutex, ticket 26 interop), publisher,
; URLs and install layout — upgrading from a Python install is an in-place
; replacement. Version comes from CI via /DMyAppVersion(=Name) flags; local
; compiles get the defaults below.
;
; Exe is the self-contained Rust binary; no runtime/, no site-packages/.
; Staged layout (relative to this script): .\bilidown\ with
; bilidown.exe, bundle\ffmpeg.exe, LICENSE.

#define MyAppName "bilidown"
#ifndef MyAppVersion
#define MyAppVersion "2.21.0"
#endif
#ifndef MyAppVersionName
#define MyAppVersionName "2.21.0"
#endif
#define MyAppPublisher "Scott Sloan"
#define MyAppURL "https://github.com/stanzhai/bilidown"
#define MyAppExeName "bilidown.exe"

[Setup]
AppId={{B096F0C1-D105-4EF9-86E1-5E87DA884EA4}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
AppCopyright=Copyright (C) 2022-2026 Scott Sloan
AppMutex=B096F0C1-D105-4EF9-86E1-5E87DA884EA4
DefaultDirName={autopf}\{#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
LicenseFile=.\LICENSE
OutputDir=.\
OutputBaseFilename=bilidown_{#MyAppVersionName}_windows_x64
SolidCompression=yes
WizardStyle=modern dynamic
ShowLanguageDialog=yes
LanguageDetectionMethod=uilanguage

[Languages]
Name: "en_US"; MessagesFile: "compiler:Default.isl"
Name: "zh_CN"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "zh_TW"; MessagesFile: "compiler:Languages\ChineseTraditional.isl"

[CustomMessages]
en_US.ConfirmUninstallOldVersion=An older version is already installed. Its installation folder will be deleted before setup can continue, so any files in that folder will also be removed. Please back up your files before installing. Delete it now?
zh_CN.ConfirmUninstallOldVersion=检测到已安装的旧版本，安装程序将在继续前直接删除其安装目录，目录下的文件也会一并移除，请在安装前做好备份。是否现在删除旧版本的安装目录？
zh_TW.ConfirmUninstallOldVersion=偵測到已安裝舊版本，安裝程式會在繼續前直接刪除其安裝目錄，目錄下的檔案也會一併移除，請在安裝前做好備份。是否現在刪除舊版本的安裝目錄？
en_US.SetupAppRunningError=Setup has detected that bilidown is currently running.%n%nPlease close all instances of it first.%nClick OK to retry, or Cancel to exit.
zh_CN.SetupAppRunningError=安装程序发现 bilidown 当前正在运行。%n%n请先关闭正在运行的程序，然后点击“确定”继续，或点击“取消”退出。
zh_TW.SetupAppRunningError=安裝程式偵測到 bilidown 目前正在執行。%n%n請先關閉正在執行的程式，然後按「確定」繼續，或按「取消」離開。
en_US.UninstallAppRunningError=Uninstall has detected that bilidown is currently running.%n%nPlease close all instances of it first.%nClick OK to retry, or Cancel to exit.
zh_CN.UninstallAppRunningError=卸载程序发现 bilidown 当前正在运行。请先关闭正在运行的程序，然后点击“确定”继续，或点击“取消”退出。
zh_TW.UninstallAppRunningError=解除安裝偵測到 bilidown 目前正在執行。請先關閉該程式後按「確定」繼續，或按「取消」離開。

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: ".\bilidown\bilidown.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: ".\bilidown\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: ".\bilidown\bundle\*"; DestDir: "{app}\bundle"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: {app}
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon; WorkingDir: {app}

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent runascurrentuser

[UninstallDelete]
Type: filesandordirs; Name: "{app}"

[Code]
const
  MUTEX_MODIFY_STATE = $00000001;
  SYNCHRONIZE = $00100000;

function OpenMutex(dwDesiredAccess, bInheritHandle: LongWord; lpName: String): LongWord;
  external 'OpenMutexW@kernel32.dll stdcall';

function CloseHandle(hObject: LongWord): LongBool;
  external 'CloseHandle@kernel32.dll stdcall';

function IsApplicationRunning(): Boolean;
var
  MutexHandle: LongWord;
begin
  MutexHandle := OpenMutex(MUTEX_MODIFY_STATE or SYNCHRONIZE, 0, 'B096F0C1-D105-4EF9-86E1-5E87DA884EA4');

  Result := MutexHandle <> 0;

  if Result then
    CloseHandle(MutexHandle);
end;

function ConfirmApplicationClosed(const MessageName: String): Boolean;
var
  ConfirmResult: Integer;
begin
  Result := False;

  while IsApplicationRunning() do
    begin
      ConfirmResult := MsgBox(CustomMessage(MessageName), mbError, MB_OKCANCEL);

      if ConfirmResult <> IDOK then
        exit;
    end;

  Result := True;
end;

function InitializeSetup(): boolean;
var
  KeyName: String;
  InstallDir: String;
  ConfirmResult: Integer;
begin
  if not ConfirmApplicationClosed('SetupAppRunningError') then
    begin
      Result := False;
      exit;
    end;

  KeyName := 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\' + '{B096F0C1-D105-4EF9-86E1-5E87DA884EA4}' + '_is1';

  if RegQueryStringValue(HKLM, KeyName, 'InstallLocation', InstallDir) then
    begin
      ConfirmResult := MsgBox(CustomMessage('ConfirmUninstallOldVersion'), mbError, MB_OKCANCEL);

      if ConfirmResult <> IDOK then
        begin
          Result := False;
          exit;
        end;

      InstallDir := RemoveQuotes(InstallDir);

      if (InstallDir <> '') and DirExists(InstallDir) then
        begin
          DelTree(InstallDir, True, True, True);
        end;
    end;
  Result := True;
end;

function InitializeUninstall(): Boolean;
begin
  if not ConfirmApplicationClosed('UninstallAppRunningError') then
    begin
      Result := False;
      exit;
    end;

  Result := True;
end;
