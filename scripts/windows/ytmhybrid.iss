#define AppName "YTM Hybrid"
#define AppVersion "0.1.0"
#define AppPublisher "Egehan Şımarık"
#define AppExeName "ytm-hybrid.exe"

; Proje kök dizinine göre dosya yolları
#define SourceExe "D:\Projeler\ytm_hybrid_client\target\release\ytm-hybrid.exe"
#define OutputDir "target\release"
#define SetupName "YTMHybrid-Setup"
#define AppIcon "D:\Projeler\ytm_hybrid_client\assets\windows\YTMHybridClientlogo(RUST).ico"

[Setup]
AppId={{C9E4B1A2-82D5-4E82-9B7C-123456789ABC}}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir={#OutputDir}
OutputBaseFilename={#SetupName}
SetupIconFile={#AppIcon}
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
WizardStyle=modern
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "Masaüstü kısayolu oluştur"; GroupDescription: "Ek kısayollar:"

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion
Source: "COPYING"; DestDir: "{app}"; DestName: "LICENSE"; Flags: ignoreversion
Source: "THIRD-PARTY.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; Windows Uygulama Kaydı
Root: HKA; Subkey: "Software\YTMHybrid"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\YTMHybrid\Capabilities"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\YTMHybrid\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\YTMHybrid\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Native YouTube Music Client built with Rust and GPUI"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\YTMHybrid\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: "{app}\{#AppExeName},0"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppName}"; ValueData: "Software\YTMHybrid\Capabilities"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExeName}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExeName}"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExeName}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""

; YouTube Bağlantı Protokolü Kaydı
Root: HKA; Subkey: "Software\YTMHybrid\Capabilities\UrlAssociations"; ValueType: string; ValueName: "youtube"; ValueData: "YTMHybridYouTube"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\YTMHybridYouTube"; ValueType: string; ValueName: ""; ValueData: "YouTube Link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\YTMHybridYouTube"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\YTMHybridYouTube\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExeName},0"
Root: HKA; Subkey: "Software\Classes\YTMHybridYouTube\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExeName}"" ""%1"""

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{#AppName} uygulamasını çalıştır"; Flags: nowait postinstall skipifsilent