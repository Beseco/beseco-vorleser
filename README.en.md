# Vorleser

[Deutsche Fassung](README.md)

Vorleser ("reader" in German) reads selected text aloud in any Windows program: select text, press **Ctrl+Alt+V**, listen. It lives as a small icon in the notification area, uses very little memory and speaks with the voices Windows already ships.

The interface is available in English and German. It follows the Windows display language and can be switched under Settings.

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
| Open the window | Left-click the notification area icon |
| Menu with voice, speed and settings | Right-click the icon |

All shortcuts can be changed under "Settings".

Windows does not let a program add an entry to the regular right-click menu of other applications. Ctrl+right-click is the substitute: the Vorleser menu appears instead of the application's own menu.

## Features

- **System-wide:** works in browsers, Office, PDF readers, editors and most other programs
- **Paste window:** paste any text and compare voices with a sample sentence
- **Read along:** the word being spoken is highlighted in the window
- **Always on top:** the window can stay above other windows
- **Voices:** all installed Windows voices, grouped by language
- **Text clean-up:** joins wrapped PDF lines, removes hyphenation, shortens links
- **Audio export:** save text as a WAV file
- **Autostart:** optionally start with Windows

## Voices

Vorleser uses the voices installed in Windows. More can be added in Windows Settings under "Time & language → Speech → Add voices".

### AI voices

Much more natural are the AI voices you can add with one click via **"AI voices …"** in the Vorleser window. They are based on [Piper](https://github.com/rhasspy/piper) and, after a one-time download, run offline on your own machine only.

| Voice | Language | Download | Training data license |
|---|---|---|---|
| Thorsten | German, male | 63 MB | CC0 ([Thorsten-Voice](https://github.com/thorstenMueller/Thorsten-Voice)) |
| Thorsten HD | German, male, highest quality | 114 MB | CC0 |
| Kerstin | German, female | 63 MB | CC0 |
| Linda | English, female, highest quality | 114 MB | public domain (LJ Speech) |
| Kristin | English, female | 64 MB | public domain (LibriVox) |

The first download also fetches the Piper speech engine once (22 MB). All files are checked against a built-in SHA-256 checksum and stored in `%LOCALAPPDATA%\Vorleser\piper`; uninstalling removes them. While reading along, AI voices highlight the whole sentence instead of single words.

Online voices that some add-ons register in Windows (marked "Online" in their name) are not listed: they depend on a third-party service, fail depending on the network and send the text to their provider.

## Privacy

Vorleser collects no data. It goes online in only two cases: the update check (at most one request per day to GitHub, without any text, settings or machine identifier; can be turned off in the settings) and when you download an AI voice yourself (from github.com and huggingface.co). Text is processed only on your own machine.

To read the selection, Vorleser uses the Windows accessibility interface. Where that is not available, it briefly copies the text through the clipboard and restores the previous clipboard content.

Settings are stored in `%APPDATA%\Vorleser\settings.ini`.

## Known limitations

- Vorleser cannot read the selection in windows running with administrator rights.
- Copy-protected PDFs and password fields do not hand out text.
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
