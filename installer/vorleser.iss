; Installer fuer Hedgie, frueher Vorleser (Inno Setup 6). Gebaut wird ueber release.ps1, das Version und
; Quelldatei uebergibt. Diese Datei bleibt bewusst reines ASCII (keine Umlaute), damit die
; Zeichenkodierung keine Rolle spielt.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #error SourceExe fehlt: Pfad zur fertigen Beseco-Vorleser.exe mit /DSourceExe=... uebergeben
#endif
#define ExeName "Beseco-Vorleser.exe"

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
; Installation fuer alle Benutzer unter C:\Program Files\Beubl-IT, dafuer mit Administratorrechten.
PrivilegesRequired=admin
DefaultDirName={autopf}\Beubl-IT\Hedgie
; Bis 0.6.3 lag Hedgie unter %LOCALAPPDATA%\Programs\Hedgie; diesen Ordner nicht wiederverwenden.
UsePreviousAppDir=no
; Autostart, "Oeffnen mit"-Liste und Officeassistent gelten bewusst fuer den angemeldeten Benutzer.
UsedUserAreasWarning=no
DisableProgramGroupPage=yes
DisableDirPage=auto
OutputDir=..\dist
OutputBaseFilename=Hedgie-Setup-{#AppVersion}
SetupIconFile=..\res\vorleser.ico
UninstallDisplayIcon={app}\{#ExeName}
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
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#ExeName}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[InstallDelete]
; Verknuepfungen aus der Zeit, als das Programm noch "Vorleser" hiess.
Type: files; Name: "{userprograms}\Vorleser.lnk"
Type: files; Name: "{userdesktop}\Vorleser.lnk"
; Verknuepfungen der Benutzer-Installation bis 0.6.3; die neuen liegen fuer alle Benutzer.
Type: files; Name: "{userprograms}\Hedgie.lnk"
Type: files; Name: "{userdesktop}\Hedgie.lnk"

[Icons]
Name: "{autoprograms}\Hedgie"; Filename: "{app}\{#ExeName}"
Name: "{autodesktop}\Hedgie"; Filename: "{app}\{#ExeName}"; Tasks: desktopicon

[Registry]
; Derselbe Eintrag, den das Programm ueber "Mit Windows starten" selbst setzt.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Vorleser"; ValueData: """{app}\{#ExeName}"" --tray"; Tasks: autostart

; "Oeffnen mit" im Explorer: Hedgie erscheint fuer diese Dateitypen in der Auswahl (fuer alle Benutzer).
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "Hedgie"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\shell\open\command"; ValueType: string; ValueData: """{app}\{#ExeName}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".txt"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".docx"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".odt"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".rtf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".pdf"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".epub"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".html"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".htm"; ValueData: ""

; Wunsch "Officeassistent einschalten"; Hedgie uebernimmt ihn beim Start und loescht den Wert.
Root: HKCU; Subkey: "Software\Beseco\Vorleser"; ValueType: string; ValueName: "Assistant"; ValueData: "1"; Tasks: assistant; Flags: uninsdeletekeyifempty

[UninstallDelete]
; Heruntergeladene KI-Stimmen (bis mehrere hundert MB). Die Einstellungen bleiben erhalten.
Type: filesandordirs; Name: "{localappdata}\Vorleser\piper"
Type: filesandordirs; Name: "{localappdata}\Vorleser\whisper"

[Run]
; Als angemeldeter Benutzer starten, nicht mit den Administratorrechten des Installers.
Filename: "{app}\{#ExeName}"; Description: "{cm:StartNow}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[Code]
const
  OldUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{8E2B6C4A-5D1F-4B7A-9C3E-2F6A1D0B7E54}_is1';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

// Beendet ein laufendes Hedgie ueber sein verstecktes Hauptfenster und wartet
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

// Raeumt die Benutzer-Installation bis 0.6.3 (%LOCALAPPDATA%\Programs\Hedgie, vorleser.exe) weg.
// Deren Deinstallation wird bewusst nicht gestartet: Sie wuerde auch die heruntergeladenen
// KI-Stimmen loeschen. Einstellungen und Stimmen bleiben so erhalten.
procedure RemoveOldUserInstall;
var
  OldDir, RunValue: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, OldUninstallKey, 'InstallLocation', OldDir) then
    exit;
  OldDir := RemoveBackslashUnlessRoot(OldDir);
  if (OldDir <> '') and (CompareText(OldDir, ExpandConstant('{app}')) <> 0) then
  begin
    DeleteFile(OldDir + '\vorleser.exe');
    DeleteFile(OldDir + '\LICENSE.txt');
    DeleteFile(OldDir + '\unins000.exe');
    DeleteFile(OldDir + '\unins000.dat');
    RemoveDir(OldDir);
    // Ein Autostart, der noch auf die alte EXE zeigt, wird nur bei gewaehltem Autostart neu gesetzt.
    if RegQueryStringValue(HKEY_CURRENT_USER, RunKey, 'Vorleser', RunValue) and
       (Pos(Lowercase(OldDir), Lowercase(RunValue)) > 0) then
      RegDeleteValue(HKEY_CURRENT_USER, RunKey, 'Vorleser');
  end;
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Classes\Applications\vorleser.exe');
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, OldUninstallKey);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunning;
  RemoveOldUserInstall;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    CloseRunning;
    // Der Autostart kann auch im Programm eingeschaltet worden sein, deshalb immer entfernen.
    RegDeleteValue(HKEY_CURRENT_USER, RunKey, 'Vorleser');
  end;
  // Den Herstellerordner C:\Program Files\Beubl-IT mitnehmen, falls er danach leer ist.
  if CurUninstallStep = usPostUninstall then
    RemoveDir(ExtractFileDir(RemoveBackslashUnlessRoot(ExpandConstant('{app}'))));
end;
