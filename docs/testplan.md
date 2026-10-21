# Testplan

Vor jeder Veröffentlichung einmal von Hand durchgehen. Die automatischen Tests decken nur Einstellungen, Textbereinigung, Versionsvergleich und die stumme Sprachausgabe ab; alles, was mit anderen Programmen, der Maus oder dem Installer zu tun hat, lässt sich nur so prüfen.

Ergebnis je Zeile eintragen: ✓ geht, ✗ geht nicht (kurz notieren, was passiert), – nicht getestet.

## 1. Markierten Text vorlesen

Jeweils einen Satz markieren und zuerst **Strg+Alt+V** drücken, dann **Strg+Rechtsklick** → „Markierten Text vorlesen“ wählen.

| Programm | Strg+Alt+V | Strg+Rechtsklick | Zwischenablage danach unverändert? |
|---|---|---|---|
| Editor (Notepad) | | | |
| Edge, normale Webseite | | | |
| Chrome | | | |
| Firefox | | | |
| Word | | | |
| Outlook, E-Mail lesen | | | |
| Outlook, E-Mail schreiben | | | |
| Adobe Reader (PDF) | | | |
| PDF in Edge | | | |
| VS Code | | | |
| Windows-Terminal | | | |
| Explorer, Dateiname | | | |

„Zwischenablage unverändert“: Vor dem Test ein Bild oder einen anderen Text kopieren und nach dem Vorlesen mit Strg+V prüfen, dass es noch da ist.

## 2. Steuerung

- [ ] Während des Lesens Strg+Alt+V: hört auf
- [ ] Strg+Alt+P: pausiert, nochmals: liest weiter
- [ ] Pause und Stopp im Fenster und im Tray-Menü
- [ ] Stimme und Geschwindigkeit wechseln, Programm beenden und neu starten: Auswahl ist noch da
- [ ] Hörprobe mit einer deutschen und einer englischen Stimme

## 3. Fenster

- [ ] Text einfügen, Vorlesen: das gesprochene Wort ist markiert und das Feld rollt mit
- [ ] Nur einen Abschnitt markieren, Vorlesen: nur dieser wird gelesen, danach ist er wieder markiert
- [ ] Bei offenem Fenster per Strg+Alt+V aus einem anderen Programm vorlesen: Text erscheint im Fenster; Strg+Z im Textfeld holt den vorherigen Text zurück
- [ ] „Immer im Vordergrund“ an: Fenster bleibt über anderen Programmen; aus: verhält sich normal
- [ ] Als WAV speichern mit einem langen Text: Fenster bleibt bedienbar, Datei lässt sich abspielen
- [ ] Fenster schließen (X): Programm läuft im Infobereich weiter

## 4. Einstellungen

- [ ] Tastenkombination ändern, speichern: neue Kombination wirkt sofort, alte nicht mehr
- [ ] Eine belegte Kombination wählen: Hinweis erscheint
- [ ] Maus-Auslöser auf „Aus“: normaler Strg+Rechtsklick funktioniert wieder
- [ ] Lautstärke ändern: hörbar leiser

## 5. Installer

- [ ] Installieren ohne Adminrechte, Start aus dem Startmenü
- [ ] „Mit Windows starten“: nach Neuanmeldung ist das Symbol da, aber kein Fenster offen
- [ ] Neue Version installieren, während die alte läuft: alte wird beendet, neue startet
- [ ] Deinstallieren über Windows-Einstellungen → Apps: Programm, Startmenü-Eintrag und Autostart sind weg

## 6. Dateien und Texterkennung

- [ ] Je eine Datei öffnen (Datei → Öffnen, Hineinziehen, im Explorer „Öffnen mit“): DOCX, ODT, RTF, EPUB, HTML, TXT, PDF mit Text, gescanntes PDF, Foto/Screenshot
- [ ] Großes PDF (50+ Seiten): Fenstertitel zeigt den Fortschritt, Fenster bleibt bedienbar
- [ ] Strg+Alt+O: Rahmen um Text in einem Bild, einem Video-Standbild und einer Schaltfläche ziehen, wird vorgelesen; Esc bricht ab
- [ ] Mit zwei Bildschirmen: Ausschnitt auf dem zweiten Bildschirm
- [ ] Neue Installation: Anleitung steht beim ersten Start im Fenster; Hilfe → Anleitung vorlesen

## 7. Officeassistent

- [ ] Installer mit Häkchen „Officeassistent einschalten“: Fenster zeigt beim Start den Arbeitsbereich
- [ ] Extras → Beseco Officeassistent aus und an: Arbeitsbereich verschwindet und erscheint wieder
- [ ] KI-Einstellungen: KI-Check zeigt den Rechner richtig an; Ollama-Server eintragen, Modelle abrufen, Verbindung testen
- [ ] IONOS (oder anderer Cloud-Anbieter) mit Schlüssel: Verbindung testen; mit falschem Schlüssel verständliche Meldung
- [ ] F9 mit kurzem Text, mit markiertem Abschnitt und mit einem langen Dokument (Fenstertitel zeigt die Schritte); alle drei Arten der Zusammenfassung
- [ ] Zusammenfassung wird rechts mitgelesen; Umschalt+F9 liest sie erneut; Alt+←/→ springt rechts satzweise
- [ ] Im Büro über den Proxy: Cloud-Anbieter erreichbar, Ollama im Büronetz ohne Proxy erreichbar
- [ ] F8 ohne eingerichtetes Diktieren: Fenster „Diktieren einrichten“ öffnet sich; Modell „Genau“ herunterladen (auch über den Proxy)
- [ ] F8: „Ich höre zu“, einige Sätze mit Pausen sprechen; Text erscheint abschnittsweise an der Schreibmarke, nach Satzende groß; F8 beendet, der letzte Abschnitt kommt noch
- [ ] Ohne Mikrofon bzw. mit gesperrtem Mikrofon (Windows-Datenschutz): verständliche Meldung
- [ ] Nach dem Diktat mit eingerichteter KI: korrigierte Fassung erscheint rechts; ohne KI passiert nichts weiter
- [ ] Umschalt+F8 in allen vier Stufen; eigene Vorgabe in den KI-Einstellungen eintragen und verwenden
- [ ] Rechnergeschwindigkeit: Wartezeit nach jeder Pause mit „Genau“ und „Sehr genau“ (es wird immer das genaueste installierte Modell genutzt)

## 8. Sonderfälle

- [ ] Programm als Administrator starten (z. B. Editor), dort markieren: Vorleser meldet „Kein Text markiert“ statt sich aufzuhängen
- [ ] Explorer neu starten (Task-Manager): Symbol im Infobereich kommt zurück
- [ ] Ohne Internet eine Online-Stimme verwenden: verständlicher Hinweis
