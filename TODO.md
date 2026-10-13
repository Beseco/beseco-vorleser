# Vorleser – To-do-Liste

Ziel: Vorleser als kostenloses Tool zum Download anbieten. Reihenfolge innerhalb der Abschnitte = empfohlene Reihenfolge.

## Zuerst: Stolpersteine für fremde Nutzer

- [x] **Zwischenablage vollständig wiederherstellen.** Alle Formate (Text, formatierter Text, Bilder als DIB, Dateilisten) werden gesichert und zurückgeschrieben, bis 64 MB.
- [ ] **Zwischenablageverlauf (Win+V):** Der kopierte Text selbst landet weiterhin im Verlauf, weil ihn die fremde Anwendung einträgt; von außen nicht zu verhindern. Erledigt ist nur, dass der wiederhergestellte Inhalt nicht doppelt erscheint. Offen: Eintrag nachträglich aus dem Verlauf löschen (WinRT), falls zuverlässig machbar.
- [x] **Einstellungsfenster.** Tastenkombinationen, Maus-Auslöser, Lautstärke und Textbereinigung in der Oberfläche; belegte Kombinationen werden beim Speichern gemeldet.

## KI-Stimmen

- [ ] **Lokale KI-Stimmen als optionaler Download** im Programm („Stimme herunterladen“), z. B. Piper mit deutschen Stimmen. EXE bleibt klein, kein Konto, kein Internet beim Vorlesen.
  - [ ] Lizenzen der einzelnen Stimmmodelle prüfen (nicht einheitlich).
  - [ ] Laufzeitbibliothek und Stimmdatei getrennt vom Programm ablegen (zusammen etwa 50–100 MB).
- [ ] **Cloud-Stimmen mit eigenem Schlüssel** (Azure, OpenAI, ElevenLabs) als Option. Deutlicher Hinweis, dass der Text dabei den Rechner verlässt.
- [ ] Hinweis in der Anleitung: Die „Online (Natural)“-Stimmen stammen von einem separaten Adapter und werden nicht mitgeliefert.

## Funktionen

- [x] **Mitlesen.** Das gesprochene Wort wird im Fenster markiert; Text aus Tastenkombination oder Zwischenablage erscheint dazu im Fenster, wenn es offen ist.
- [x] **Immer im Vordergrund** als Schalter im Fenster.
- [ ] **Navigation.** Satzweise vor und zurück, an der letzten Stelle weiterlesen.
- [x] **Textbereinigung.** PDF-Zeilenumbrüche und Silbentrennung zusammenfügen, Links auf den Seitennamen kürzen (abschaltbar).
- [ ] Fußnotenziffern überspringen (braucht eine sichere Erkennung, sonst gehen echte Zahlen verloren).
- [ ] **Aussprache-Wörterbuch** für Namen und Abkürzungen.
- [ ] **Automatische Spracherkennung.** Englischer Text mit englischer Stimme, deutscher mit deutscher.
- [x] **Als Audiodatei speichern** (WAV).
- [ ] MP3-Export (braucht einen Encoder).
- [ ] WAV-Export im Hintergrund mit Fortschritt; bisher blockiert er das Fenster, bis die Datei fertig ist.
- [ ] **Dateien öffnen** per Drag-and-drop: TXT, DOCX, PDF, EPUB.
- [ ] **Texterkennung (OCR)** für Bildschirmbereiche, die sich nicht markieren lassen (gescannte PDFs, Bilder).
- [ ] **Stimmenliste im Fenster nach Sprache gruppieren** (im Menü schon vorhanden) und Online-Stimmen kennzeichnen.

## Oberfläche und Barrierefreiheit

- [ ] Englische Oberfläche (Sprachumschaltung).
- [ ] Dunkelmodus.
- [x] Lautstärke einstellbar.
- [ ] Ausgabegerät wählbar.
- [ ] Vollständige Tastaturbedienung und Test mit Bildschirmleser (Zielgruppe: Sehschwäche, Legasthenie).

## Veröffentlichung

- [x] **Eigenes EXE-Icon und Versionsinfo** in der Datei (`build.rs`, Symbol aus `res/make-icon.ps1`).
- [x] **Installer** und portable ZIP-Variante (`release.ps1`, Inno Setup, Installation ohne Adminrechte).
- [ ] Installer einmal vollständig durchspielen: Installation, Update über eine laufende Version, Deinstallation.
- [ ] **Code-Signatur.** Ohne sie warnt SmartScreen, und Virenscanner reagieren eher auf Maus-Hook und simulierte Tastendrücke. Zertifikat kostet Geld; Entscheidung offen.
- [ ] **winget-Paket.**
- [ ] **Git-Repository** anlegen, Lizenz festlegen (z. B. MIT).
- [ ] **Webseite/Downloadseite** auf beseco.com mit Kurzanleitung und Screenshots.
- [ ] **Änderungsprotokoll** und Versionsnummern pflegen (Version steht in `Cargo.toml`, erscheint im Info-Dialog).
- [ ] **Update-Hinweis** im Programm.
- [ ] **Datenschutzaussage:** keine Telemetrie, Text verlässt den Rechner nicht (Ausnahme: Online- und Cloud-Stimmen).
- [ ] Automatischer Build (CI), damit jede Version reproduzierbar entsteht.

## Offene technische Punkte

- [ ] Fehlerfall der Online-Stimmen (neuer Versuch, Hinweis) unter echten Ausfallbedingungen prüfen; bisher nicht gezielt auslösbar.
- [ ] Hotkeys, Strg+Rechtsklick-Menü und Markierungserkennung in der Testmatrix durchgehen: Editor, Edge/Chrome, Firefox, Word, Outlook, PDF-Reader, VS Code, Terminal.
- [ ] Verhalten in Fenstern mit Administratorrechten: verständlicher Hinweis statt stiller Fehlfunktion.

## Erledigt

- [x] Tray-Programm mit Vorlesen, Pause, Fortsetzen, Stopp
- [x] Stimmen- und Geschwindigkeitsauswahl, dauerhaft gespeichert
- [x] Markierten Text per Tastenkombination vorlesen (UI Automation, sonst Zwischenablage)
- [x] Vorleser-Menü per Strg+Rechtsklick und per Tastenkombination
- [x] Stimmen im Menü nach Sprache gruppiert
- [x] Fenster mit Textfeld, Hörprobe und Steuerung
- [x] Autostart, Einzelinstanz
- [x] Info-Dialog mit Version und Hersteller
- [x] Einstellungsfenster, Lautstärke, Textbereinigung, WAV-Export
- [x] Vollständige Wiederherstellung der Zwischenablage
- [x] EXE-Symbol und Versionsinfo
