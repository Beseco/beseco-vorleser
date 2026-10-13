# Vorleser

[Deutsche Fassung](README.md)

Vorleser ("reader" in German) reads selected text aloud in any Windows program: select text, press **Ctrl+Alt+V**, listen. It lives as a small icon in the notification area, uses very little memory and speaks with the voices Windows already ships.

The user interface is currently German only.

![Main window highlighting the word being spoken](docs/fenster.png)

## Download

Get the current version from [Releases](https://github.com/GITHUB_OWNER/beseco-vorleser/releases/latest):

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

All shortcuts can be changed under "Einstellungen" (settings).

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

Vorleser does not bundle any voices; it uses the ones installed in Windows. More voices can be added in Windows Settings under "Time & language → Speech → Add voices".

Microsoft does not expose the very natural-sounding Windows 11 Narrator voices to other programs directly. To use them, install the separate [NaturalVoiceSAPIAdapter](https://github.com/gexgd0419/NaturalVoiceSAPIAdapter); Vorleser then lists those voices automatically. The adapter is an independent project and not part of Vorleser.

## Privacy

Vorleser collects no data and does not connect to the internet on its own. Text is handed to the Windows speech engine on your own machine. The exception is online voices added through third-party tools such as the adapter above: those send the text to their provider for synthesis.

To read the selection, Vorleser uses the Windows accessibility interface. Where that is not available, it briefly copies the text through the clipboard and restores the previous clipboard content.

Settings are stored in `%APPDATA%\Vorleser\settings.ini`.

## Known limitations

- Vorleser cannot read the selection in windows running with administrator rights.
- Copy-protected PDFs and password fields do not hand out text.
- Text copied while reading the selection may show up in the clipboard history (Win+V).
- The interface is German only so far.

## Building

Vorleser is written in Rust and depends only on the `windows` crate.

```powershell
# Once: Rust, MinGW-w64 (provides dlltool and windres) and, for the installer, Inno Setup
winget install -e --id Rustlang.Rustup
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

Developed by Florian Beubl, [beseco – Beubl IT Systems](https://beseco.com), Freising, Germany. Please report bugs and wishes as an [issue](https://github.com/GITHUB_OWNER/beseco-vorleser/issues).
