# Änderungsprotokoll

Alle nennenswerten Änderungen am Vorleser. Die Versionsnummern folgen [Semantic Versioning](https://semver.org/lang/de/).

## 0.5.6 – unveröffentlicht

- Mini-Pille: wandert mit auf den Bildschirm, auf dem gerade gearbeitet wird; lässt sich mit der Maus an eine andere Stelle ziehen und bleibt dort (Rechtsklick → Position zurücksetzen); eingeklappt besser zu sehen (heller Rand, blauer Punkt)

## 0.5.5 – unveröffentlicht

- Neu: Stimme passend zur Sprache: englischer Text wird mit einer englischen Stimme gelesen, deutscher mit einer deutschen (erkannt werden Deutsch, Englisch, Französisch, Spanisch, Italienisch und Niederländisch). Genommen wird die zuletzt für diese Sprache gewählte Stimme, sonst eine vorhandene, KI-Stimmen zuerst; danach gilt wieder die eingestellte Stimme. Abschaltbar in den Einstellungen

## 0.5.4 – unveröffentlicht

- Die KI schreibt live: Zusammenfassung, Protokoll und Korrektur erscheinen rechts Wort für Wort, während sie entstehen
- Während die KI arbeitet, wird „Zusammenfassen“ bzw. „Korrigieren“ zu „Abbrechen“; das bis dahin Geschriebene bleibt stehen

## 0.5.3 – unveröffentlicht

- PDFs mit Textebene werden direkt gelesen: viel schneller und zeichengenau (Umlaute, Zahlen, Fachbegriffe); gescannte Seiten gehen weiter über die Texterkennung
- Kopf- und Fußzeilen in PDFs werden übersprungen: Zeilen, die oben oder unten auf vielen Seiten an derselben Stelle wiederkehren, und Seitenzahlen („Seite 3 von 12“, „- 3 -“); abschaltbar in den Einstellungen
- Auf Wunsch prüft zusätzlich die KI die Ränder der Seiten (Einstellungen, Officeassistent); dabei gehen nur die ersten und letzten drei Zeilen jeder Seite an die KI, bei einem Cloud-Anbieter erst nach Rückfrage

## 0.5.2 – unveröffentlicht

- Neu: Aufnahme zu Protokoll (Assistent → Aufnahme zu Protokoll …, oder die Datei einfach ins Fenster ziehen): MP3, M4A, WAV, WMA, FLAC und Ton aus MP4-Videos werden lokal mit Whisper abgeschrieben, mit Zeitmarken; mit KI entsteht daraus gleich ein Protokoll mit Teilnehmern, Themen, Beschlüssen, Aufgaben und offenen Punkten
- „Protokoll“ als vierte Art beim Zusammenfassen, auch für eingefügte Abschriften
- Fortschritt im Fenstertitel, Abbrechen über Assistent → Abschrift abbrechen

## 0.5.1 – 2026-10-22

- Neues Design: Textblatt in Lesegröße, unten eine Wiedergabe-Leiste mit großem Vorlesen/Pause-Knopf, Satz zurück/vor, „Satz 3 von 9“ mit Fortschrittslinie, Stimme und Geschwindigkeit („1,2×“) zum Anklicken; hell oder dunkel wie in Windows eingestellt (auch Titel- und Menüleiste)
- Officeassistent als Seitenleiste mit den Registern Zusammenfassen, Diktieren und Korrigieren; Art und Stufe als Knöpfe, Ergebnis mit „Übernehmen“, „Kopieren“ und „Vorlesen“
- Neu: Mini-Pille am unteren Bildschirmrand, immer im Vordergrund, solange das Fenster zu ist (abschaltbar unter Extras bzw. in den Einstellungen): unter der Maus zeigt sie Mikrofon (diktieren) und den sprechenden Kopf (markierten Text vorlesen)
- Neu: kompakte Diktier-Pille: ✕ beenden, Pegel, ✓ fertig (mit KI: gleich korrigieren); Zustand und Zielprogramm stehen im Hinweis darüber
- Neu: Diktieren in jedes Programm (Strg+Alt+D): der Text erscheint dort an der Schreibmarke; getippt wird nur in das Programm, in dem das Diktat begonnen hat. „Korrigieren“ ersetzt dort den diktierten Text durch die korrigierte Fassung
- Neu: Mini-Player beim Vorlesen aus anderen Programmen: Satz zurück/vor, Pause, Geschwindigkeit, Fenster öffnen, Stopp
- Neu: Mikrofon wählbar (Assistent → Mikrofon)

### Außerdem neu seit 0.4.0 (Version 0.5.0 wurde nicht einzeln veröffentlicht)

- Neu: Beseco Officeassistent, optional unter Extras oder im Installer einschaltbar; ausgeschaltet ändert sich nichts
- Arbeitsbereich mit geteilter Ansicht: links der Text, rechts die Zusammenfassung, die gleich vorgelesen wird
- Zusammenfassen (F9) kurz, in Stichpunkten oder ausführlich; lange Texte werden in Teilen verarbeitet
- KI-Anbieter: eigener Server oder dieser PC (Ollama) oder Cloud-Anbieter mit OpenAI-kompatibler Schnittstelle (z. B. IONOS AI Model Hub); Modelle abrufen und Verbindung testen
- KI-Check: zeigt Prozessor, Arbeitsspeicher und Grafikkarte und empfiehlt lokal, Server oder Cloud
- Der Schlüssel wird mit dem Windows-Konto verschlüsselt gespeichert
- Neu: Diktieren (F8) mit lokaler Spracherkennung (Whisper): der Text erscheint während des Sprechens an der Schreibmarke, Aufnahmen verlassen den Rechner nicht. Modell (schnell, genau, sehr genau) einmalig unter Assistent → Diktieren einrichten herunterladen
- Neu: Korrigieren (Umschalt+F8) in Stufen: nur Rechtschreibung, Satzbau glätten, umformulieren oder eigene Vorgabe (z. B. „als freundliche E-Mail“); nach einem Diktat automatisch, wenn eine KI eingerichtet ist
- Die Deinstallation entfernt heruntergeladene Modelle der Spracherkennung
- Neu: Einrichtungsassistent für die KI (Assistent → KI einrichten): auf diesem PC, Ollama-Server im Firmennetz, Cloud-Anbieter mit API-Schlüssel oder ohne KI. Auf diesem PC installiert er bei Bedarf Ollama und lädt ein passendes Sprachmodell; er wählt das Modell aus und prüft die Verbindung. Öffnet sich auch beim ersten Start, wenn der Officeassistent im Installer gewählt wurde
- Ohne eingerichtete KI sind deren Schaltflächen und Menüpunkte gesperrt, statt des Status steht „KI einrichten …“ da; Diktieren geht immer
- Lokale Modelle mit Denkfunktion (z. B. qwen3.5) antworten ohne langes Nachdenken: deutlich schneller auf PCs ohne Grafikkarte
- Aufgeräumtes Hauptfenster: Stimme, Geschwindigkeit, Hörprobe und KI-Stimmen stehen jetzt in den Einstellungen (und wie bisher im Menü „Stimme“); unten zeigt das Fenster die eingestellte Stimme. „Immer im Vordergrund“ unter Extras

## 0.4.0 – 2026-10-21

- Neu: Dateien vorlesen: Word (DOCX), OpenDocument (ODT), RTF, PDF, EPUB, HTML, Textdateien und Bilder; über Datei → Öffnen, durch Hineinziehen oder „Öffnen mit“ im Explorer
- Neu: Texterkennung (OCR) mit der Windows-Texterkennung: Bildschirmausschnitt per Rahmen erkennen und vorlesen (Strg+Alt+O), PDFs und gescannte Dokumente
- Neu: Anleitung beim ersten Start im Textfeld; Hilfe → Anleitung anzeigen / vorlesen
- Große Dateien werden im Hintergrund gelesen, der Fenstertitel zeigt den Fortschritt

## 0.3.0 – 2026-10-20

- Neu: KI-Stimmen (Piper) mit deutlich natürlicherem Klang: Thorsten, Thorsten HD und Kerstin auf Deutsch, Linda und Kristin auf Englisch. Download mit einem Klick über „KI-Stimmen …“, danach ohne Internet
- Mitlesen mit KI-Stimmen satzweise, WAV-Export auch mit KI-Stimmen
- Die Deinstallation entfernt heruntergeladene KI-Stimmen
- Neu: Menüleiste (Datei, Bearbeiten, Wiedergabe, Stimme, Extras, Hilfe) mit Tastenkürzeln: F5 Vorlesen, F6 Pause, F7 Stopp, Strg+O Öffnen, Strg+S als Audiodatei speichern
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
