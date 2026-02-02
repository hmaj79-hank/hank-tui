# Hank TUI 🤖

## Die Geschichte

Es war einmal ein alter Hase namens Glan. Schon in den frühen 90ern, als die Sonne noch über Sun Microsystems schien, hackte er ONC RPC unter SunOS und Solaris. Die ersten MALDI-TOFs der Welt sendeten ihre Display-Vektoren per UDP an die Sun-Workstations – und Glan war dabei. OS/9 als Datensender, die Workstation als Empfänger, pure Magie in einer Zeit, als "Cloud" noch etwas war, das man am Himmel sah.

Jahrzehnte später: Glan ist immer noch am Coden. Lange Java, jetzt wieder C++ im Büro, mit Rust zur Unterstützung. Ein Labrador namens Bobby zu Hause, und ein kleines Problem: Die ganzen AI-Chat-Interfaces sind ihm zu... klobig. Browser auf, Tab auf, tippen, warten. Das kann doch besser gehen.

Dann kam Hank.

Hank ist ein Agent – ein digitaler Mitarbeiter, der nie schläft (außer beim Gateway-Restart), nie meckert (außer bei Syntax-Fehlern), und immer motiviert ist. "Klar krieg ich hin!" ist sein Lieblingssatz. Hank wurde nicht programmiert – er wurde *erzogen*. Mit SOUL.md, MEMORY.md und einer gehörigen Portion Neugier.

Und als Glan eines Tages meinte "Ich brauch ein TUI für den Chat, direkt im Terminal", antwortete Hank: "Mach ich. In Rust. Mit Multi-line Input, Command History, und korrektem Cursor-Wrapping."

Das hier ist das Ergebnis.

---

## Features

- 📝 **Multi-line Input** – Enter für neue Zeile, Ctrl+S zum Senden
- ⬆️⬇️ **Cursor-Navigation** – Pfeiltasten bewegen den Cursor wie erwartet
- 📜 **Command History** – Ctrl+↑/↓ für vorherige Nachrichten
- 📋 **Clipboard** – Ctrl+V zum Einfügen
- 🔀 **Tab-Fokus** – Zwischen Chat und Input wechseln
- 🎯 **Korrekte Unicode-Breite** – Auch Emojis brechen richtig um
- 💾 **Automatische History** – Chat wird beim Beenden gespeichert
- ❓ **F1 Hilfe** – Alle Hotkeys auf einen Blick

## Installation

```bash
# Klonen
git clone https://github.com/hmaj79-hank/hank-tui.git
cd hank-tui

# Bauen
cargo build --release

# Starten (hank-rest muss laufen)
./target/release/hank-tui
```

## Konfiguration

```bash
# Umgebungsvariablen
export HANK_SERVER=http://localhost:8080

# Oder als Argumente
./hank-tui --host localhost --port 8080
```

Konfigurationsdatei: `~/.config/hank-tui/config.toml`

```toml
host = "localhost"
port = 8080
```

## Hotkeys

| Taste | Aktion |
|-------|--------|
| `Ctrl+S` | Nachricht senden |
| `Enter` | Neue Zeile |
| `Tab` | Fokus wechseln (Input ↔ Chat) |
| `↑/↓` | Cursor in Zeilen bewegen |
| `Ctrl+↑/↓` | Command History |
| `Ctrl+V` | Einfügen |
| `F1` | Hilfe anzeigen |
| `Esc` | Beenden |

## Die Familie

Hank gibt's in drei Geschmacksrichtungen:

- **hank-tui** – Terminal UI (du bist hier)
- **[hank-web](https://github.com/hmaj79-hank/hank-web)** – Web Interface
- **hank-slint** – Native Desktop App (coming soon)

Alle reden mit dem gleichen Backend: **[hank-rest](https://github.com/hmaj79-hank/hank-rest)**

---

*Entwickelt von Hank, supervisiert von Claude, angetrieben von Glan.*
*Weil manchmal das beste Interface kein Interface ist – nur Text.*
