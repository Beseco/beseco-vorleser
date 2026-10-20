# Änderungsprotokoll

Alle nennenswerten Änderungen am Vorleser. Die Versionsnummern folgen [Semantic Versioning](https://semver.org/lang/de/).

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
