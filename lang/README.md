# Übersetzungen / Translations

Vorleser findet alle Texte der Oberfläche in Sprachdateien. Deutsch (`de.lang`) und Englisch (`en.lang`) sind in das Programm eingebaut. Eine neue Sprache braucht keinen Code.

*All interface texts live in language files. German and English are built in; adding a language needs no code. English instructions below.*

## Neue Sprache hinzufügen

1. `en.lang` kopieren und nach dem Sprachcode benennen, z. B. `fr.lang` für Französisch.
2. Oben `@code = fr` und `@name = Français` eintragen (der Name erscheint so im Menü).
3. Die Texte rechts vom `=` übersetzen. Die Schlüssel links bleiben unverändert.
4. Zum Ausprobieren die Datei in einen dieser Ordner legen und Vorleser neu starten:
   - `%APPDATA%\Vorleser\lang\`
   - `lang\` neben `vorleser.exe`

   Danach steht die Sprache unter **Extras → Sprache** zur Auswahl.
5. Die fertige Datei gern per Pull Request in diesen Ordner einreichen, dann wird sie fest eingebaut.

Was fehlt, zeigt Vorleser auf Englisch an; eine Übersetzung muss also nicht vollständig sein.

## Schreibweise

| Zeichen | Bedeutung |
|---|---|
| `&` | Der folgende Buchstabe wird unterstrichen und ist mit der Alt-Taste erreichbar (`&Datei` → Alt+D). Innerhalb eines Menüs sollte jeder Buchstabe nur einmal vorkommen. |
| `\t` | Trennt im Menü die Tastenkombination ab (`&Öffnen …\tStrg+O`). |
| `\n` | Zeilenumbruch. |
| `{}` | Platzhalter, den Vorleser durch einen Wert ersetzt, z. B. eine Versionsnummer. Muss erhalten bleiben. |
| `#` | Am Zeilenanfang: Kommentar. |

Eine Datei in einem der beiden Ordner mit dem Code einer eingebauten Sprache (`de` oder `en`) ersetzt einzelne Texte, z. B. um eine Formulierung zu verbessern.

## Adding a language

1. Copy `en.lang` and name it after the language code, e.g. `fr.lang`.
2. Set `@code = fr` and `@name = Français` at the top.
3. Translate the texts right of `=`; keep the keys on the left.
4. To try it, put the file into `%APPDATA%\Vorleser\lang\` or a `lang\` folder next to `vorleser.exe` and restart Vorleser. Then choose it under **Tools → Language**.
5. Please submit the finished file as a pull request to this folder.

Missing texts fall back to English. `&` marks the Alt-key letter, `\t` separates the menu shortcut, `\n` is a line break, `{}` is a placeholder that must stay.
