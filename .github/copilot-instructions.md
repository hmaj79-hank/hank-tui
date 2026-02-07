# Copilot instructions for hank-tui

## Build / run / test / lint
- Build: `cargo build --release`.
- Dev run: `cargo run -- --host <host> --port <port>` (requires `hank-rest` backend; falls back to `HANK_HOST`/`HANK_PORT` or config defaults).
- Tests: `cargo test` (none present today); single test/filter: `cargo test <name_or_pattern>`.
- Lint: `cargo clippy -- -D warnings`.
- Format: `cargo fmt`.

## Architecture
- Single binary in `src/main.rs` using `tokio` async runtime, `ratatui` + `crossterm` for the TUI, `reqwest`/`serde` for HTTP + data, and `arboard` for clipboard.
- State lives in `App` (input buffer/cursor, scroll + auto-scroll, focus, messages, command history, connection/error status, history toggle, timestamps). `Focus` tracks `Input`/`Chat`/`Help`.
- Config path: `~/.config/hank-tui/config.toml`; priority is CLI args → env (`HANK_HOST`, `HANK_PORT`) → config file → defaults (`localhost:8080`). Config is saved on startup with the resolved values.
- History path: `~/.config/hank-tui/history.json`; loads on start unless `--no-history`, saves on exit, and only persists the last 100 messages. `Ctrl+Shift+D` deletes the local history file.
- On startup the app fetches all messages via `GET {server}/messages?since=0`; then polls every ~2s with `GET /messages?since=<last_timestamp>` (skips echoing user messages). `Ctrl+L` posts to `/messages/clear` and clears local chat.
- Sending: `Ctrl+S` or `Ctrl+Enter` posts to `{server}/chat` with `ChatRequest { message }`, handled in a background task with a 120s timeout; assistant/error replies are appended and update connection status.

## UI and interaction conventions
- Layout: chat pane, 5-line input pane, status bar; manual Unicode-aware character wrapping keeps cursor positions and scroll in sync.
- Focus: `Tab` toggles Input/Chat; `F1` or `?` (outside Input) toggles Help. Cursor is shown only when Input is focused.
- Input: `Enter` inserts newline; command history via `Ctrl+↑/↓`; paste with `Ctrl+V` inserts at the cursor; `--no-history` disables loading/saving chat history.
- Chat navigation: with Chat focused, `↑/↓` scroll line-wise, `PageUp/PageDown` for pages, `Home/End` jump to top/bottom; `Alt+↑/↓` scrolls chat without changing focus; auto-scroll re-enables when the scroll offset returns to zero.
- Clearing/history: `Ctrl+L` clears chat locally and on the server; `Ctrl+Shift+D` removes the local history file (if history is enabled).
- Status: bottom bar shows server URL, message count, wrapped line totals, scroll offset, and connection status; errors are also injected as chat messages. Help overlay lists all keybindings and closes on any keypress.
