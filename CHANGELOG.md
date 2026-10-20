# Änderungsprotokoll

Alle nennenswerten Änderungen am Vorleser. Die Versionsnummern folgen [Semantic Versioning](https://semver.org/lang/de/).

## 0.3.0 – 2026-10-20

- Neu: KI-Stimmen (Piper) mit deutlich natürlicherem Klang: Thorsten, Thorsten HD und Kerstin auf Deutsch, Linda und Kristin auf Englisch. Download mit einem Klick über „KI-Stimmen …“, danach ohne Internet
- Mitlesen mit KI-Stimmen satzweise, WAV-Export auch mit KI-Stimmen
- Die Deinstallation entfernt heruntergeladene KI-Stimmen
- Neu: Menüleiste (Datei, Bearbeiten, Wiedergabe, Stimme, Extras, Hilfe) mit Tastenkürzeln: F5 Vorlesen, F6 Pause, F7 Stopp, Strg+O Öffnen, Strg+S als WAV speichern
- Neu: Textdateien öffnen, auch durch Hineinziehen ins Fenster
- Neu: Sprachsystem mit Sprachdateien; weitere Sprachen lassen sich ohne Programmierung ergänzen (siehe `lang/README.md`). Die Sprache wechselt sofort, ohne Neustart
- Nach Updates suchen jetzt auch von Hand (Hilfe → Nach Updates suchen)
- Neu: satzweise springen mit Alt+← / Alt+→ (auch während des Vorlesens) und den Schaltflächen „◀ Satz“ / „Satz ▶“
- Neu: F5 liest ab der Schreibmarke; nach Stopp steht sie am Anfang des zuletzt gesprochenen Satzes
- Neu: Text und Leseposition werden beim Beenden gemerkt, damit es nach einem Neustart weitergeht (abschaltbar)
- Neu: Speichern als MP3 (über den MP3-Encoder von Windows), WAV bleibt wählbar
- Neu: Proxy fürs Firmennetz: wpad.dat und Anmeldung mit dem Windows-Konto, wahlweise eigene Proxy-Adresse
- Behoben: Nach einem Neustart des Vorlesens konnten Meldungen des abgebrochenen Durchgangs das neue Vorlesen vorzeitig beenden

## 0.2.0 – 2026-10-20

- Online-Stimmen werden nicht mehr angeboten (unzuverlässig, Text ginge an fremde Dienste)
- Neu: englische Oberfläche; die Sprache folgt Windows und lässt sich in den Einstellungen umstellen
- Neu: Hinweis auf neue Versionen (einmal täglich über GitHub, in den Einstellungen abschaltbar)
- WAV-Export läuft im Hintergrund; das Fenster bleibt bedienbar
- Wird Text aus einem anderen Programm ins Fenster übernommen, holt Strg+Z den vorherigen Inhalt zurück
- Vorlagen für Fehlermeldungen und Wünsche auf GitHub
- Versionshinweise auf GitHub entstehen automatisch aus diesem Änderungsprotokoll

## 0.1.0 – 2026-10-13

Erste öffentliche Version.

- Markierten Text aus beliebigen Programmen per Tastenkombination vorlesen (Standard: Strg+Alt+V)
- Vorleser-Menü an der Mausposition per Strg+Rechtsklick oder Strg+Alt+M
- Pause, Fortsetzen, Stopp; Zwischenablage vorlesen
- Fenster mit Textfeld, Stimmenauswahl, Hörprobe und Geschwindigkeit
- Mitlesen: Das gesprochene Wort wird im Fenster markiert
- Fenster auf Wunsch immer im Vordergrund
- Alle installierten Windows-Stimmen (SAPI 5 und OneCore), nach Sprache gruppiert
- Textbereinigung für PDF-Text: Zeilenumbrüche, Silbentrennung, Links
- Text als WAV-Datei speichern
- Einstellungsfenster für Tastenkombinationen, Maus-Auslöser und Lautstärke
- Autostart mit Windows, Symbol im Infobereich
- Installer ohne Administratorrechte und portable ZIP-Variante
