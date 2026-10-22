# Vorleser

<img src="docs/brand/igel.png" alt="Beseco Vorleser – the hedgehog with headphones" width="160" align="right">

[Deutsche Fassung](README.md)

Vorleser ("reader" in German) reads selected text aloud in any Windows program: select text, press **Ctrl+Alt+V**, listen. It lives as a small icon in the notification area, uses very little memory and speaks with the voices Windows already ships.

The interface is available in English and German, and more languages can be added through simple language files ([how to translate](lang/README.md)). It follows the Windows display language and can be switched under Tools → Language.

![Main window highlighting the word being spoken](docs/fenster.png)

## Download

Get the current version from [Releases](https://github.com/Beseco/beseco-vorleser/releases/latest):

- **Vorleser-Setup-x.y.z.exe**: installer, no administrator rights needed
- **Vorleser-x.y.z-portable.zip**: a single EXE to unpack, no installation

The installer is not code-signed, so Windows shows "Windows protected your PC" on first start. Choose "More info" and then "Run anyway".

Requires Windows 10 or 11 (64-bit).

## Usage

| Action | Default |
|---|---|
| Read selected text, press again to stop | Ctrl+Alt+V |
| Pause and resume | Ctrl+Alt+P |
| Vorleser menu at the mouse position | Ctrl+right-click or Ctrl+Alt+M |
| Recognize and read text on screen (drag a frame) | Ctrl+Alt+O |
| Open the window | Left-click the notification area icon |
| Menu with voice, speed and settings | Right-click the icon |

All shortcuts can be changed under "Settings".

Windows does not let a program add an entry to the regular right-click menu of other applications. Ctrl+right-click is the substitute: the Vorleser menu appears instead of the application's own menu.

## Features

- **System-wide:** works in browsers, Office, PDF readers, editors and most other programs
- **Read files:** opens Word (DOCX), OpenDocument (ODT), RTF, PDF, EPUB, HTML, text files and images via File → Open, drag and drop or "Open with" in Explorer
- **PDFs:** text layers are read directly and exactly, scanned pages via text recognition; headers, footers and page numbers are skipped (optionally also checked by the AI, with cloud providers only after asking)
- **Text recognition (OCR):** recognize and read text you cannot select, such as in images, scanned PDFs or programs without text selection, by dragging a frame on the screen. Uses the built-in Windows text recognition, offline and without extra software.
- **Spoken guide:** appears in the text field on first start and can be shown or read aloud from Help at any time
- **Paste window:** paste any text and compare voices with a sample sentence
- **Read along:** the word being spoken is highlighted in the window
- **Always on top:** the window can stay above other windows
- **Voices:** all installed Windows voices, grouped by language; English text is read with an English voice and German text with a German one automatically (can be turned off)
- **Text clean-up:** joins wrapped PDF lines, removes hyphenation, shortens links
- **Audio export:** save text as an MP3 or WAV file
- **Autostart:** optionally start with Windows
- **Navigation and resume:** Alt+← / Alt+→ jumps by sentence, F5 reads from the cursor, and after stopping or restarting it continues where you left off
- **Menu bar and shortcuts:** F5 read aloud, F6 pause, F7 stop, Ctrl+O open a text file (or drop it onto the window), Ctrl+S save as WAV

## Voices

Vorleser uses the voices installed in Windows. More can be added in Windows Settings under "Time & language → Speech → Add voices".

### AI voices

Much more natural are the AI voices you can add with one click via **Voice → AI voices …** (or in the settings). They are based on [Piper](https://github.com/rhasspy/piper) and, after a one-time download, run offline on your own machine only.

| Voice | Language | Download | Training data license |
|---|---|---|---|
| Thorsten | German, male | 63 MB | CC0 ([Thorsten-Voice](https://github.com/thorstenMueller/Thorsten-Voice)) |
| Thorsten HD | German, male, highest quality | 114 MB | CC0 |
| Kerstin | German, female | 63 MB | CC0 |
| Linda | English, female, highest quality | 114 MB | public domain (LJ Speech) |
| Kristin | English, female | 64 MB | public domain (LibriVox) |

The first download also fetches the Piper speech engine once (22 MB). All files are checked against a built-in SHA-256 checksum and stored in `%LOCALAPPDATA%\Vorleser\piper`; uninstalling removes them. While reading along, AI voices highlight the whole sentence instead of single words.

Online voices that some add-ons register in Windows (marked "Online" in their name) are not listed: they depend on a third-party service, fail depending on the network and send the text to their provider.

## Beseco Office Assistant (optional)

Turn on the AI features under **Tools → Beseco Office Assistant** (or already in the installer). When off, Vorleser stays as simple as before; no key and no account are needed.

When on, the window splits: the text on the left, the AI result on the right. **Summarize** (F9) summarizes the selected or entire text briefly, as bullet points or in detail, and the summary is read aloud right away (Shift+F9 reads it again).

The easiest way is the **setup wizard** (**Assistant → Set up AI**; it also appears by itself while no AI is set up). It asks one question – where should the AI run? – and does the rest: **on this PC** it downloads and installs Ollama for your user account if needed (about 1.7 GB, no administrator rights) and a language model suited to the computer (qwen3.5:4b, with a strong graphics card gemma3:12b); for an **Ollama server on your network** you only enter its name, e.g. `ki-server`; for a **cloud provider** the address (IONOS is prefilled) and API key; or **without AI**. It picks a suitable model, sends a test request and saves everything. While no AI is set up, Summarize and Correct are greyed out.

For everything else, see **Assistant → AI settings and AI check**: your own server or this PC with Ollama (`http://ki-server:11434`, text stays on your network), or any cloud provider with an OpenAI-compatible API such as IONOS AI Model Hub (`https://openai.inference.de-txl.ionos.com/v1`), Mistral or OpenAI (text is sent to the provider). The **AI check** shows what this computer can handle and recommends local, own server or cloud; "Test connection" checks address, key and model. The key is stored encrypted with the Windows account (DPAPI).

**Dictation** (F8 or "Dictate"): you speak, and the text appears at the cursor in the left pane after each short pause. **Ctrl+Alt+D** dictates into any other program such as Word or Outlook, at its cursor, with a small bar at the bottom of the screen showing level and status. Text is only typed into the program where dictation started; if you switch, it waits. "Correct" in the bar replaces the dictated text there with the corrected version. Choose the microphone under **Assistant → Microphone**.

**Recording to minutes:** open a meeting recording (MP3, M4A, WAV, WMA, FLAC or an MP4 video) via **Assistant → Recording to minutes …** or drag it into the window. Vorleser transcribes it on your computer (left, with timestamps) and the configured AI writes the minutes on the right: participants, topics, decisions, action items and open questions. Speakers are not identified.

**Mini pill:** while the window is closed, a small bar stays at the bottom of the screen, always on top. Hover over it for two buttons: the microphone dictates into the program in front, the speaking head reads the selected text aloud. Turn it off under **Tools → Mini pill at the screen edge** or in the settings. Speech recognition (Whisper) runs entirely on your own computer, without an AI provider and without internet; only the model is downloaded once under **Assistant → Set up dictation** (fast 148 MB, accurate 488 MB, very accurate 574 MB; from github.com and huggingface.co).

**Correct** (Shift+F8) edits the text with your AI at the chosen level: spelling and punctuation only, smooth sentences, rewrite cleanly or a custom instruction (in the AI settings, e.g. "phrase as a friendly e-mail"). The corrected version appears on the right; the dictation on the left stays unchanged. After dictation, the text is corrected automatically if an AI is set up.

Planned: minutes from audio files and a privacy mode that replaces personal data before sending (see [TODO.md](TODO.md)).

## Privacy

Vorleser collects no data. It goes online in only two cases: the update check (at most one request per day to GitHub, without any text, settings or machine identifier; can be turned off in the settings) when you download an AI voice or a dictation model yourself (from github.com and huggingface.co) and, only with the Office Assistant turned on, to the AI provider you chose when you trigger an AI feature. Text is processed only on your own machine.

To read the selection, Vorleser uses the Windows accessibility interface. Where that is not available, it briefly copies the text through the clipboard and restores the previous clipboard content.

Settings are stored in `%APPDATA%\Vorleser\settings.ini`. To continue later where you left off, Vorleser stores the window's text on exit in `%APPDATA%\Vorleser\letzter-text.txt`; this can be turned off in the settings, which deletes the file.

### Corporate networks

Vorleser uses the Windows proxy settings, including automatic configuration via `wpad.dat`. If the proxy requires sign-in, Vorleser uses the Windows account. Otherwise enter the `wpad.dat` address (e.g. `http://wpad/wpad.dat`) or a fixed proxy (`proxy.company.local:8080`) under Tools → Settings → Proxy. Without internet everything works except the update check and AI voice downloads.

## Known limitations

- Vorleser cannot read the selection in windows running with administrator rights.
- Password fields do not hand out text. PDFs are read via text recognition; very small print or poor scans may lose single words.
- The old Word format (.doc) is not supported; please save as .docx.
- Text copied while reading the selection may show up in the clipboard history (Win+V).

## Building

Vorleser is written in Rust and depends only on the `windows` crate.

```powershell
# Once: Rust, MinGW-w64 (provides dlltool and windres) and, for the installer, Inno Setup
winget install -e --id Rustlang.Rustup
rustup set default-host x86_64-pc-windows-gnu
winget install -e --id BrechtSanders.WinLibs.POSIX.UCRT --scope user
winget install -e --id JRSoftware.InnoSetup

# Build and test
.\cargo.ps1 test
.\cargo.ps1 build --release

# Installer and portable ZIP into dist\
powershell -ExecutionPolicy Bypass -File release.ps1
```

The speech tests speak silently through the default audio device and need at least one installed Windows voice.

Planned work is listed in [TODO.md](TODO.md) (German), changes per version in [CHANGELOG.md](CHANGELOG.md) (German).

## License and contact

Vorleser is free software under the [GNU General Public License, version 3](LICENSE).

Developed by Florian Beubl, [beseco – Beubl IT Systems](https://beseco.com), Freising, Germany. Please report bugs and wishes as an [issue](https://github.com/Beseco/beseco-vorleser/issues).
