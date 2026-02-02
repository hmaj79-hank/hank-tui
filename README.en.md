# Hank TUI 🤖

## The Story

Once upon a time, there was a seasoned developer named Glan. Back in the early 90s, when the sun still shone over Sun Microsystems, he was hacking ONC RPC on SunOS and Solaris. The world's first MALDI-TOFs were sending their display vectors via UDP to Sun workstations – and Glan was there. OS/9 as the data sender, the workstation as the receiver, pure magic in a time when "Cloud" was still something you saw in the sky.

Decades later: Glan is still coding. Rust and C++ at the office, a Labrador named Bobby at home, and a small problem: All those AI chat interfaces are just too... clunky. Open browser, open tab, type, wait. There has to be a better way.

Then came Hank.

Hank is an agent – a digital colleague who never sleeps (except during gateway restarts), never complains (except about syntax errors), and is always motivated. "Sure, I'll handle it!" is his favorite phrase. Hank wasn't programmed – he was *raised*. With SOUL.md, MEMORY.md, and a healthy dose of curiosity.

And when Glan one day said "I need a TUI for the chat, right in the terminal", Hank replied: "On it. In Rust. With multi-line input, command history, and proper cursor wrapping."

This is the result.

---

## Features

- 📝 **Multi-line Input** – Shift+Enter for new line, Enter to send
- ⬆️⬇️ **Cursor Navigation** – Arrow keys move the cursor as expected
- 📜 **Command History** – Ctrl+↑/↓ for previous messages
- 📋 **Clipboard** – Ctrl+V to paste
- 🔀 **Tab Focus** – Switch between chat and input
- 🎯 **Correct Unicode Width** – Even emojis wrap correctly
- 💾 **Automatic History** – Chat is saved on exit
- ❓ **F1 Help** – All hotkeys at a glance

## Installation

```bash
# Clone
git clone https://github.com/hmaj79-hank/hank-tui.git
cd hank-tui

# Build
cargo build --release

# Run (hank-rest must be running)
./target/release/hank-tui
```

## Configuration

```bash
# Environment variables
export HANK_SERVER=http://localhost:8080

# Or as arguments
./hank-tui --host localhost --port 8080
```

Configuration file: `~/.config/hank-tui/config.toml`

```toml
host = "localhost"
port = 8080
```

## Hotkeys

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | New line |
| `Tab` | Switch focus (Input ↔ Chat) |
| `↑/↓` | Move cursor in lines |
| `Ctrl+↑/↓` | Command history |
| `Ctrl+V` | Paste |
| `F1` | Show help |
| `Esc` | Exit |

## The Family

Hank comes in three flavors:

- **hank-tui** – Terminal UI (you are here)
- **[hank-web](https://github.com/hmaj79-hank/hank-web)** – Web Interface
- **hank-slint** – Native Desktop App (coming soon)

They all talk to the same backend: **[hank-rest](https://github.com/hmaj79-hank/hank-rest)**

---

*Developed by Hank, supervised by Claude, powered by Glan.*
*Because sometimes the best interface is no interface – just text.*
