; Installer fuer Hedgie, frueher Vorleser (Inno Setup 6). Gebaut wird ueber release.ps1, das Version und
; Quelldatei uebergibt. Diese Datei bleibt bewusst reines ASCII (keine Umlaute), damit die
; Zeichenkodierung keine Rolle spielt.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #error SourceExe fehlt: Pfad zur fertigen vorleser.exe mit /DSourceExe=... uebergeben
#endif

[Setup]
; Die AppId darf sich nie aendern, sonst erkennt ein Update die vorhandene Installation nicht.
AppId={{8E2B6C4A-5D1F-4B7A-9C3E-2F6A1D0B7E54}
AppName=Hedgie
AppVersion={#AppVersion}
AppVerName=Hedgie {#AppVersion}
AppPublisher=beseco - Beubl IT Systems
AppPublisherURL=https://beseco.com
AppSupportURL=https://beseco.com
AppContact=info@beseco.com
VersionInfoVersion={#AppVersion}
VersionInfoCompany=beseco - Beubl IT Systems
; Installation nur fuer den angemeldeten Benutzer, ohne Administratorrechte.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Hedgie
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=..\dist
OutputBaseFilename=Hedgie-Setup-{#AppVersion}
SetupIconFile=..\res\vorleser.ico
UninstallDisplayIcon={app}\vorleser.exe
UninstallDisplayName=Hedgie
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Das laufende Programm wird im Code-Abschnitt sauber beendet.
CloseApplications=no

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
german.Autostart=Hedgie mit Windows starten
english.Autostart=Start Hedgie with Windows
german.StartNow=Hedgie jetzt starten
english.StartNow=Start Hedgie now
german.Assistant=Beseco Officeassistent einschalten (KI und Diktieren; beim ersten Start hilft ein Assistent beim Einrichten, spaeter unter Extras umschaltbar)
english.Assistant=Turn on Beseco Office Assistant (AI and dictation; a wizard helps with the setup on first start, can be changed later under Tools)

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
Name: "assistant"; Description: "{cm:Assistant}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "vorleser.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[InstallDelete]
; Verknuepfungen aus der Zeit, als das Programm noch "Vorleser" hiess.
Type: files; Name: "{autoprograms}\Vorleser.lnk"
Type: files; Name: "{autodesktop}\Vorleser.lnk"

[Icons]
Name: "{autoprograms}\Hedgie"; Filename: "{app}\vorleser.exe"
Name: "{autodesktop}\Hedgie"; Filename: "{app}\vorleser.exe"; Tasks: desktopicon

[Registry]
; Derselbe Eintrag, den das Programm ueber "Mit Windows starten" selbst setzt.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Vorleser"; ValueData: """{app}\vorleser.exe"" --tray"; Tasks: autostart

; "Oeffnen mit" im Explorer: Vorleser erscheint fuer diese Dateitypen in der Auswahl.
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "Hedgie"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\shell\open\command"; ValueType: string; ValueData: """{app}\vorleser.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".txt"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".docx"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".odt"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".rtf"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".pdf"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".epub"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".html"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\vorleser.exe\SupportedTypes"; ValueType: string; ValueName: ".htm"; ValueData: ""

; Wunsch "Officeassistent einschalten"; Vorleser uebernimmt ihn beim Start und loescht den Wert.
Root: HKCU; Subkey: "Software\Beseco\Vorleser"; ValueType: string; ValueName: "Assistant"; ValueData: "1"; Tasks: assistant; Flags: uninsdeletekeyifempty

[UninstallDelete]
; Heruntergeladene KI-Stimmen (bis mehrere hundert MB). Die Einstellungen bleiben erhalten.
Type: filesandordirs; Name: "{localappdata}\Vorleser\piper"
Type: filesandordirs; Name: "{localappdata}\Vorleser\whisper"

[Run]
Filename: "{app}\vorleser.exe"; Description: "{cm:StartNow}"; Flags: nowait postinstall skipifsilent

[Code]
// Beendet ein laufendes Vorleser-Programm ueber sein verstecktes Hauptfenster und wartet
// bis zu fuenf Sekunden, damit die EXE ersetzt oder geloescht werden kann.
procedure CloseRunning;
var
  Tries: Integer;
begin
  if FindWindowByClassName('VorleserFenster') = 0 then
    exit;
  PostMessage(FindWindowByClassName('VorleserFenster'), $0010, 0, 0);
  Tries := 0;
  while (FindWindowByClassName('VorleserFenster') <> 0) and (Tries < 50) do
  begin
    Sleep(100);
    Tries := Tries + 1;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunning;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    CloseRunning;
    // Der Autostart kann auch im Programm eingeschaltet worden sein, deshalb immer entfernen.
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Vorleser');
  end;
end;
