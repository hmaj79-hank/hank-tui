use arboard::Clipboard;
use chrono::{Local, TimeZone};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::{env, fs, io, panic, path::PathBuf, time::Instant};
use unicode_width::UnicodeWidthChar;

#[derive(Parser, Debug)]
#[command(name = "hank-tui")]
#[command(about = "Terminal UI for Hank chat server", long_about = None)]
struct Args {
    /// Host to connect to (can also be set via HANK_HOST environment variable)
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// Port to connect to (can also be set via HANK_PORT environment variable)
    #[arg(short, long)]
    port: Option<u16>,
    
    /// Disable chat history (do not load or save)
    #[arg(long)]
    no_history: bool,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Config {
    host: String,
    port: u16,
}

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut path| {
            path.push("hank-tui");
            path.push("config.toml");
            path
        })
    }

    fn load() -> Self {
        Self::config_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_else(|| Config {
                host: "localhost".to_string(),
                port: 8080,
            })
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(self)?;
            fs::write(path, content)?;
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
    timestamp: String,
    #[serde(default)]
    timestamp_ms: Option<u64>,
}

#[derive(Serialize, Deserialize)]
struct ChatHistory {
    server_url: String,
    messages: Vec<Message>,
    saved_at: String,
}

impl ChatHistory {
    fn history_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut path| {
            path.push("hank-tui");
            path.push("history.json");
            path
        })
    }

    fn load() -> Option<Self> {
        Self::history_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|content| serde_json::from_str(&content).ok())
    }

    fn save(server_url: &str, messages: &[Message]) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::history_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            
            // Only save last 100 messages
            let messages_to_save: Vec<Message> = messages
                .iter()
                .rev()
                .take(100)
                .rev()
                .cloned()
                .collect();
            
            let history = ChatHistory {
                server_url: server_url.to_string(),
                messages: messages_to_save,
                saved_at: Local::now().to_rfc3339(),
            };
            
            let content = serde_json::to_string_pretty(&history)?;
            fs::write(path, content)?;
        }
        Ok(())
    }
    
    fn delete() -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = Self::history_path() {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
}

#[derive(PartialEq)]
enum Focus {
    Input,
    Chat,
    Help,
}

struct App {
    input: String,
    cursor_pos: usize,
    messages: Vec<Message>,
    server_url: String,
    loading: bool,
    scroll: u16,
    input_scroll: u16,  // Scroll offset for input field
    command_history: Vec<String>,
    history_index: Option<usize>,
    connection_status: String,
    last_error: Option<String>,
    auto_scroll: bool,
    focus: Focus,
    history_enabled: bool,
    last_timestamp: u64,
    last_poll: Instant,
    debug_overlay: bool,
}

#[derive(Serialize)]
struct ChatRequest {
    message: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    content: String,
    #[allow(dead_code)]
    complete: bool,
}

#[derive(Deserialize, Serialize)]
struct ServerMessage {
    role: String,
    content: String,
    timestamp: u64,
}

enum PollEvent {
    Messages(Vec<Message>),
    Error(String),
}

impl App {
    fn new(server_url: String, history_enabled: bool) -> Self {
        let mut messages = Vec::new();
        
        // Load history if enabled
        if history_enabled {
            if let Some(history) = ChatHistory::load() {
                if history.server_url == server_url {
                    messages = history.messages;
                    messages.push(Message {
                        role: "system".to_string(),
                        content: format!("Historie geladen ({} Nachrichten) - {}", 
                            messages.len(), history.saved_at),
                        timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                    });
                } else {
                    messages.push(Message {
                        role: "system".to_string(),
                        content: format!("Neue Session für {}", server_url),
                        timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                    });
                }
            } else {
                messages.push(Message {
                    role: "system".to_string(),
                    content: format!("Verbunden mit {} (History aktiviert)", server_url),
                    timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                });
            }
        } else {
            messages.push(Message {
                role: "system".to_string(),
                content: format!("Verbunden mit {} (History deaktiviert)", server_url),
                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
            });
        }
        
        let last_timestamp = messages
            .iter()
            .filter_map(|m| m.timestamp_ms)
            .max()
            .unwrap_or(0);

        Self {
            input: String::new(),
            cursor_pos: 0,
            messages,
            server_url,
            loading: false,
            scroll: 0,
            input_scroll: 0,
            command_history: Vec::new(),
            history_index: None,
            connection_status: "Connected".to_string(),
            last_error: None,
            auto_scroll: true,
            focus: Focus::Input,
            history_enabled,
            last_timestamp,
            last_poll: Instant::now(),
            debug_overlay: false,
        }
    }

    fn navigate_history_up(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        
        let new_index = match self.history_index {
            None => Some(self.command_history.len() - 1),
            Some(0) => Some(0),
            Some(i) => Some(i - 1),
        };
        
        if let Some(idx) = new_index {
            self.history_index = Some(idx);
            self.input = self.command_history[idx].clone();
            self.cursor_pos = self.input.len();
        }
    }

    fn navigate_history_down(&mut self) {
        if self.command_history.is_empty() {
            return;
        }
        
        match self.history_index {
            None => {}
            Some(i) if i >= self.command_history.len() - 1 => {
                self.history_index = None;
                self.input.clear();
                self.cursor_pos = 0;
            }
            Some(i) => {
                self.history_index = Some(i + 1);
                self.input = self.command_history[i + 1].clone();
                self.cursor_pos = self.input.len();
            }
        }
    }
    
    fn scroll_to_bottom(&mut self) {
        self.scroll = 0;
        self.auto_scroll = true;
    }
    
    fn scroll_up(&mut self) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_add(1);
    }
    
    fn scroll_down(&mut self) {
        if self.scroll > 0 {
            self.scroll = self.scroll.saturating_sub(1);
        }
        if self.scroll == 0 {
            self.auto_scroll = true;
        }
    }

    fn scroll_page_up(&mut self, amount: u16) {
        self.auto_scroll = false;
        self.scroll = self.scroll.saturating_add(amount.max(1));
    }

    fn scroll_page_down(&mut self, amount: u16) {
        if self.scroll > amount {
            self.scroll = self.scroll.saturating_sub(amount);
        } else {
            self.scroll = 0;
            self.auto_scroll = true;
        }
    }

    fn jump_to_top(&mut self) {
        self.auto_scroll = false;
        self.scroll = u16::MAX;
    }

    fn jump_to_bottom(&mut self) {
        self.scroll = 0;
        self.auto_scroll = true;
    }
    
    fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Input => Focus::Chat,
            Focus::Chat => Focus::Input,
            Focus::Help => Focus::Input,
        };
    }
    
    fn toggle_help(&mut self) {
        self.focus = match self.focus {
            Focus::Help => Focus::Input,
            _ => Focus::Help,
        };
    }
    
    /// Calculate cursor line and column for given width (accounting for wrapping and newlines)
    fn cursor_line_col(&self, width: usize) -> (usize, usize) {
        if width == 0 {
            return (0, 0);
        }
        
        let mut line = 0;
        let mut col = 0;
        
        for (i, ch) in self.input.chars().enumerate() {
            // Return position BEFORE processing this character
            if i == self.cursor_pos {
                return (line, col);
            }
            
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                let char_width = ch.width().unwrap_or(1);
                // Wrap BEFORE adding character if it would exceed width
                if col + char_width > width {
                    line += 1;
                    col = 0;
                }
                col += char_width;
            }
        }
        
        // Cursor is at the end of input
        (line, col)
    }
    
    /// Calculate total lines for input (accounting for wrapping and newlines)
    fn input_total_lines(&self, width: usize) -> usize {
        if width == 0 || self.input.is_empty() {
            return 1;
        }
        
        let mut lines = 1;
        let mut col = 0;
        
        for ch in self.input.chars() {
            if ch == '\n' {
                lines += 1;
                col = 0;
            } else {
                let char_width = ch.width().unwrap_or(1);
                // Wrap BEFORE adding character if it would exceed width
                if col + char_width > width {
                    lines += 1;
                    col = 0;
                }
                col += char_width;
            }
        }
        
        lines
    }
    
    /// Move cursor up one line in input
    fn cursor_up(&mut self, width: usize) {
        if width == 0 {
            return;
        }
        
        let (line, target_col) = self.cursor_line_col(width);
        
        if line == 0 {
            return; // Already at first line
        }
        
        // Find position at same column in previous line
        let target_line = line - 1;
        let mut current_line = 0;
        let mut current_col = 0;
        let mut last_pos_on_target_line = 0;
        
        for (i, ch) in self.input.chars().enumerate() {
            if current_line == target_line {
                last_pos_on_target_line = i;
                if current_col >= target_col {
                    self.cursor_pos = i;
                    return;
                }
            }
            if current_line > target_line {
                // Went past target line
                self.cursor_pos = last_pos_on_target_line;
                return;
            }
            
            if ch == '\n' {
                if current_line == target_line {
                    // End of target line before reaching column
                    self.cursor_pos = i;
                    return;
                }
                current_line += 1;
                current_col = 0;
            } else {
                let char_width = ch.width().unwrap_or(1);
                // Wrap BEFORE if would exceed
                if current_col + char_width > width {
                    if current_line == target_line {
                        // End of target line (wrapped)
                        self.cursor_pos = i;
                        return;
                    }
                    current_line += 1;
                    current_col = 0;
                }
                current_col += char_width;
            }
        }
        
        self.cursor_pos = last_pos_on_target_line.min(self.input.len());
    }
    
    /// Move cursor down one line in input
    fn cursor_down(&mut self, width: usize) {
        if width == 0 {
            return;
        }
        
        let (line, target_col) = self.cursor_line_col(width);
        let total_lines = self.input_total_lines(width);
        
        if line >= total_lines - 1 {
            return; // Already at last line
        }
        
        // Find position at same column in next line
        let target_line = line + 1;
        let mut current_line = 0;
        let mut current_col = 0;
        let mut last_pos_on_target_line = self.input.len();
        
        for (i, ch) in self.input.chars().enumerate() {
            if current_line == target_line {
                last_pos_on_target_line = i;
                if current_col >= target_col {
                    self.cursor_pos = i;
                    return;
                }
            }
            
            if ch == '\n' {
                if current_line == target_line {
                    // End of target line before reaching column
                    self.cursor_pos = i;
                    return;
                }
                current_line += 1;
                current_col = 0;
            } else {
                let char_width = ch.width().unwrap_or(1);
                // Wrap BEFORE if would exceed
                if current_col + char_width > width {
                    if current_line == target_line {
                        // End of target line (wrapped)
                        self.cursor_pos = i;
                        return;
                    }
                    current_line += 1;
                    current_col = 0;
                }
                current_col += char_width;
            }
        }
        
        // Cursor ends up at end of input if target line is last
        self.cursor_pos = self.input.len();
    }
    
    /// Update input scroll to keep cursor visible
    fn update_input_scroll(&mut self, width: usize, visible_lines: u16) {
        if width == 0 || visible_lines == 0 {
            return;
        }
        
        let (cursor_line, _) = self.cursor_line_col(width);
        let cursor_line = cursor_line as u16;
        
        // Scroll up if cursor is above visible area
        if cursor_line < self.input_scroll {
            self.input_scroll = cursor_line;
        }
        // Scroll down if cursor is below visible area
        if cursor_line >= self.input_scroll + visible_lines {
            self.input_scroll = cursor_line - visible_lines + 1;
        }
    }
    
    /// Wrap text manually using character-wrapping (not word-wrapping)
    /// This ensures cursor calculation matches display exactly
    fn wrap_text_for_display(&self, width: usize) -> String {
        if width == 0 {
            return self.input.clone();
        }
        
        let mut result = String::with_capacity(self.input.len() + self.input.len() / width);
        let mut col = 0;
        
        for ch in self.input.chars() {
            if ch == '\n' {
                result.push(ch);
                col = 0;
            } else {
                let char_width = ch.width().unwrap_or(1);
                // Wrap BEFORE adding character if it would exceed width
                if col + char_width > width {
                    result.push('\n');
                    col = 0;
                }
                result.push(ch);
                col += char_width;
            }
        }
        
        result
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn format_timestamp(ms: u64) -> String {
    let ts = chrono::Local.timestamp_millis_opt(ms as i64).single();
    match ts {
        Some(t) => t.format("%H:%M:%S").to_string(),
        None => Local::now().format("%H:%M:%S").to_string(),
    }
}

fn wrapped_line_count(lines: &[Line], width: usize) -> u32 {
    if width == 0 {
        return lines.len() as u32;
    }

    let mut total: u32 = 0;
    for line in lines {
        if line.spans.is_empty() {
            total = total.saturating_add(1);
            continue;
        }

        let mut col = 0usize;
        let mut line_count: u32 = 1;
        for span in &line.spans {
            for ch in span.content.chars() {
                let char_width = ch.width().unwrap_or(1);
                if char_width == 0 {
                    continue;
                }
                if col + char_width > width {
                    line_count = line_count.saturating_add(1);
                    col = char_width;
                } else {
                    col += char_width;
                }
            }
        }

        total = total.saturating_add(line_count);
    }

    total
}

const CHAT_PADDING_LINES: u32 = 20;

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_values(lines: &[Line], width: usize, visible_lines: u16, auto_scroll: bool, scroll: u16) -> (u16, u16, u32) {
        let total_lines: u32 = wrapped_line_count(lines, width).saturating_add(CHAT_PADDING_LINES);
        let visible_lines_u32 = visible_lines as u32;
        let max_scroll_u32 = total_lines.saturating_sub(visible_lines_u32);
        let max_scroll: u16 = max_scroll_u32.min(u32::from(u16::MAX)) as u16;

        let scroll_offset = if total_lines <= visible_lines_u32 {
            0
        } else if auto_scroll {
            max_scroll
        } else {
            max_scroll.saturating_sub(scroll)
        };

        (max_scroll, scroll_offset, total_lines)
    }

    #[test]
    fn counts_wrapped_lines_basic() {
        let lines = vec![Line::from("12345"), Line::from("1234567890")]; // second wraps once at width 8
        let total = wrapped_line_count(&lines, 8);
        assert_eq!(total, 3); // two logical + one wrapped
    }

    #[test]
    fn counts_wrapped_lines_unicode_width() {
        let lines = vec![Line::from("😀abc")]; // emoji width 2
        let total = wrapped_line_count(&lines, 3); // 2+1 exceeds 3, so wrap after emoji
        assert_eq!(total, 2);
    }

    #[test]
    fn scroll_auto_goes_to_max_with_padding() {
        let lines = vec![Line::from("one"), Line::from("two"), Line::from("three")];
        let (max_scroll, scroll_offset, total) = scroll_values(&lines, 10, 2, true, 0);
        assert!(total > wrapped_line_count(&lines, 10)); // padding applied
        assert_eq!(scroll_offset, max_scroll);
    }

    #[test]
    fn manual_scroll_clamps() {
        let lines = vec![Line::from("short"), Line::from("another short line"), Line::from("last")];
        let (max_scroll, scroll_offset, _) = scroll_values(&lines, 10, 2, false, 5);
        assert!(max_scroll >= scroll_offset);
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let mut config = Config::load();
    
    // Priority: CLI args > environment variables > config file > defaults
    let host = args.host
        .or_else(|| std::env::var("HANK_HOST").ok())
        .unwrap_or(config.host.clone());
    
    let port = args.port
        .or_else(|| std::env::var("HANK_PORT").ok().and_then(|p| p.parse().ok()))
        .unwrap_or(config.port);
    
    // Update config with the values being used
    config.host = host.clone();
    config.port = port;
    
    // Save config for next time (ignore errors)
    let _ = config.save();
    
    let server_url = format!("http://{}:{}", host, port);

    // Setup panic handler to restore terminal
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Clear the terminal to prevent any echo issues
    terminal.clear()?;

    let mut app = App::new(server_url.clone(), !args.no_history);

    let result = run_app(&mut terminal, &mut app).await;

    // Save history on exit if enabled
    if app.history_enabled {
        let _ = ChatHistory::save(&server_url, &app.messages);
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initial load: fetch ALL messages from server (since=0)
    {
        let server_url = app.server_url.clone();
        if let Ok(response) = reqwest::Client::new()
            .get(format!("{}/messages?since=0", server_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            if let Ok(messages) = response.json::<Vec<ServerMessage>>().await {
                // Dump initial payload next to the executable for debugging
                if let Ok(exe_path) = env::current_exe() {
                    if let Some(dir) = exe_path.parent() {
                        if let Ok(serialized) = serde_json::to_string_pretty(&messages) {
                            let _ = fs::write(dir.join("initial_messages.json"), serialized);
                        }
                    }
                }

                // Clear local history and load from server
                let had_local = !app.messages.is_empty();
                app.messages.clear();
                
                for msg in messages {
                    let timestamp_str = chrono::Local
                        .timestamp_millis_opt(msg.timestamp as i64)
                        .single()
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "??:??:??".to_string());
                    
                    app.messages.push(Message {
                        role: msg.role,
                        content: msg.content,
                        timestamp: timestamp_str,
                        timestamp_ms: Some(msg.timestamp),
                    });
                    
                    if msg.timestamp > app.last_timestamp {
                        app.last_timestamp = msg.timestamp;
                    }
                }
                
                let msg_count = app.messages.len();
                let source = "Server";
                app.messages.push(Message {
                    role: "system".to_string(),
                    content: format!("{} Nachrichten vom {} geladen", msg_count, source),
                    timestamp: Local::now().format("%H:%M:%S").to_string(),
                    timestamp_ms: Some(now_ms()),
                });
                
                app.scroll_to_bottom();
            }
        }
    }
    
    loop {
        // Poll server für neue Nachrichten (alle 2 Sekunden, wenn nicht loading)
        if !app.loading && app.last_poll.elapsed().as_secs() >= 2 {
            app.last_poll = Instant::now();
            let server_url = app.server_url.clone();
            let since = app.last_timestamp;
            
            // Non-blocking poll
            if let Ok(response) = reqwest::Client::new()
                .get(format!("{}/messages?since={}", server_url, since))
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                if let Ok(messages) = response.json::<Vec<ServerMessage>>().await {
                    for msg in messages {
                        // Skip only if we already have this exact message (avoid echo duplicates)
                        if msg.role == "user" {
                            if msg.timestamp > app.last_timestamp {
                                app.last_timestamp = msg.timestamp;
                            }
                            let already_exists = app
                                .messages
                                .iter()
                                .any(|m| m.role == msg.role && m.timestamp_ms == Some(msg.timestamp));
                            if already_exists {
                                continue;
                            }
                        }

                        // Nur hinzufügen wenn noch nicht vorhanden (exact role+timestamp)
                        let already_exists = app
                            .messages
                            .iter()
                            .any(|m| m.role == msg.role && m.timestamp_ms == Some(msg.timestamp));
                        
                        if !already_exists {
                            let timestamp_str = chrono::Local
                                .timestamp_millis_opt(msg.timestamp as i64)
                                .single()
                                .map(|dt| dt.format("%H:%M:%S").to_string())
                                .unwrap_or_else(|| "??:??:??".to_string());
                            
                            app.messages.push(Message {
                                role: msg.role,
                                content: msg.content,
                                timestamp: timestamp_str,
                                timestamp_ms: Some(msg.timestamp),
                            });
                            
                            if msg.timestamp > app.last_timestamp {
                                app.last_timestamp = msg.timestamp;
                            }
                            
                            // Auto-scroll bei neuen Nachrichten
                            if app.auto_scroll {
                                app.scroll_to_bottom();
                            }
                        }
                    }
                }
            }
        }

        terminal.draw(|f| {
            // Fixed input height of 5 lines
            let input_height = 5u16;

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(input_height),
                    Constraint::Length(1),
                ])
                .split(f.area());

            // Chat-Verlauf mit Timestamps
            let mut lines: Vec<Line> = Vec::new();
            for msg in &app.messages {
                let (prefix, style) = match msg.role.as_str() {
                    "user" => ("Du: ", Style::default().fg(Color::Cyan)),
                    "assistant" => ("Hank: ", Style::default().fg(Color::Green)),
                    "system" => ("", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                    "error" => ("Error: ", Style::default().fg(Color::Red)),
                    _ => ("", Style::default()),
                };
                
                // Timestamp für non-system messages
                if !msg.role.is_empty() && msg.role != "system" {
                    lines.push(Line::from(vec![
                        Span::styled(&msg.timestamp, Style::default().fg(Color::DarkGray)),
                        Span::raw(" "),
                        Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                        Span::styled(msg.content.lines().next().unwrap_or(""), style),
                    ]));
                    
                    // Weitere Zeilen
                    for line in msg.content.lines().skip(1) {
                        lines.push(Line::from(Span::styled(
                            format!("{:width$}{}", "", line, width = msg.timestamp.len() + 1 + prefix.len()),
                            style,
                        )));
                    }
                } else {
                    lines.push(Line::from(Span::styled(&msg.content, style)));
                }
                lines.push(Line::from(""));
            }

            if app.loading {
                lines.push(Line::from(Span::styled(
                    "Hank denkt nach...",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
                )));
            }

            // Show last error if any
            if let Some(ref err) = app.last_error {
                lines.push(Line::from(Span::styled(
                    format!("⚠ {}", err),
                    Style::default().fg(Color::Red),
                )));
            }

            // Calculate scroll offset for chat using the same wrapping logic as rendering
            let chat_width = chunks[0].width.saturating_sub(2) as usize;
            let visible_lines = chunks[0].height.saturating_sub(2);
            let total_lines: u32 = wrapped_line_count(&lines, chat_width)
                .saturating_add(CHAT_PADDING_LINES);
            let visible_lines_u32 = visible_lines as u32;
            let max_scroll_u32 = total_lines.saturating_sub(visible_lines_u32);
            let max_scroll: u16 = max_scroll_u32.min(u32::from(u16::MAX)) as u16;

            // Clamp stored scroll to max
            if app.scroll > max_scroll {
                app.scroll = max_scroll;
            }

            let scroll_offset = if total_lines <= visible_lines_u32 {
                0
            } else if app.auto_scroll {
                max_scroll
            } else {
                max_scroll.saturating_sub(app.scroll)
            };

            // Chat widget with focus indicator
            let chat_title = if app.focus == Focus::Chat {
                " Chat [FOKUSSIERT - ↑↓=Scroll, Tab=Wechsel] "
            } else {
                " Chat [Tab=Fokussieren] "
            };
            
            let chat_block = Block::default()
                .borders(Borders::ALL)
                .title(chat_title)
                .border_style(if app.focus == Focus::Chat {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                });

            let messages_widget = Paragraph::new(lines)
                .block(chat_block)
                .wrap(Wrap { trim: false })
                .scroll((scroll_offset, 0));
            f.render_widget(messages_widget, chunks[0]);

            // Input with wrapping and focus indicator
            let input_title = if app.loading {
                " Warte... "
            } else if app.focus == Focus::Input {
                " Nachricht [Ctrl+S=Senden, F1=Hilfe] "
            } else {
                " Nachricht [Tab=Fokussieren] "
            };
            
            let input_block = Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(if app.focus == Focus::Input && !app.loading {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                });
            
            // Calculate input dimensions
            let input_area_width = chunks[1].width.saturating_sub(2) as usize;
            let visible_input_lines = input_height.saturating_sub(2);
            
            // Update scroll to keep cursor visible
            app.update_input_scroll(input_area_width, visible_input_lines);
            
            // Use manually wrapped text to ensure cursor matches display
            let wrapped_input = app.wrap_text_for_display(input_area_width);
            let input_widget = Paragraph::new(wrapped_input)
                .block(input_block)
                .scroll((app.input_scroll, 0))
                .style(if app.loading {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                });
            f.render_widget(input_widget, chunks[1]);

            // Status bar
            let status_text = format!(
                " {} | Msgs: {} | Lines: {}/{} | Scroll: {} | {}",
                app.server_url,
                app.messages.len(),
                total_lines,
                visible_lines,
                if app.auto_scroll { "bottom".to_string() } else { app.scroll.to_string() },
                app.connection_status
            );
            let status_widget = Paragraph::new(status_text)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White));
            f.render_widget(status_widget, chunks[2]);

            // Cursor positioning (only when input is focused)
            if !app.loading && app.focus == Focus::Input {
                let input_width = chunks[1].width.saturating_sub(2) as usize;
                if input_width > 0 {
                    let (cursor_line, cursor_col) = app.cursor_line_col(input_width);
                    let visible_line = (cursor_line as u16).saturating_sub(app.input_scroll);
                    
                    if visible_line < visible_input_lines {
                        f.set_cursor_position((
                            chunks[1].x + cursor_col as u16 + 1,
                            chunks[1].y + visible_line + 1,
                        ));
                    }
                }
            }
            
            // Help overlay
            if app.focus == Focus::Help {
                let help_text = vec![
                    Line::from(Span::styled("═══ Hank TUI Hilfe ═══", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                    Line::from(""),
                    Line::from(Span::styled("── Allgemein ──", Style::default().fg(Color::Cyan))),
                    Line::from("  F1, ?         Hilfe anzeigen/schließen"),
                    Line::from("  Tab           Fokus wechseln (Input ↔ Chat)"),
                    Line::from("  Esc, Ctrl+C   Beenden"),
                    Line::from(""),
                    Line::from(Span::styled("── Eingabe (Input fokussiert) ──", Style::default().fg(Color::Cyan))),
                    Line::from("  Ctrl+S        Nachricht senden"),
                    Line::from("  Enter         Neue Zeile"),
                    Line::from(""),
                    Line::from(Span::styled("── Chat Scroll ──", Style::default().fg(Color::Cyan))),
                    Line::from("  Tab           Chat fokussieren"),
                    Line::from("  ↑/↓           Zeilenweise scrollen"),
                    Line::from("  PageUp/Down   Seitenweise scrollen"),
                    Line::from("  Home/End      Anfang/Ende"),
                    Line::from("  Ctrl+V        Einfügen aus Zwischenablage"),
                    Line::from("  ↑/↓           Cursor zwischen Zeilen bewegen"),
                    Line::from("  ←/→           Cursor links/rechts"),
                    Line::from("  Home/End      Zeilenanfang/-ende"),
                    Line::from("  Ctrl+↑/↓      Command History (vorherige Nachrichten)"),
                    Line::from(""),
                    Line::from(Span::styled("── Chat (Chat fokussiert) ──", Style::default().fg(Color::Cyan))),
                    Line::from("  ↑/↓           Scrollen (1 Zeile)"),
                    Line::from("  PgUp/PgDown   Scrollen (10 Zeilen)"),
                    Line::from("  Home          Zum Anfang"),
                    Line::from("  End           Zum Ende (Auto-Scroll)"),
                    Line::from(""),
                    Line::from(Span::styled("── Sonstiges ──", Style::default().fg(Color::Cyan))),
                    Line::from("  Alt+↑/↓       Chat scrollen (immer)"),
                    Line::from("  Ctrl+L        Chat löschen (Server + lokal)"),
                    Line::from("  Ctrl+Shift+D  History-Datei löschen"),
                    Line::from(""),
                    Line::from(Span::styled("Drücke eine beliebige Taste zum Schließen", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))),
                ];
                
                // Clamp help dimensions to terminal size
                let term_width = f.area().width;
                let term_height = f.area().height;
                let help_height = (help_text.len() as u16 + 2).min(term_height.saturating_sub(2));
                let help_width = 55u16.min(term_width.saturating_sub(2));
                let help_x = term_width.saturating_sub(help_width) / 2;
                let help_y = term_height.saturating_sub(help_height) / 2;
                
                // Ensure we don't overflow
                let help_width = help_width.min(term_width.saturating_sub(help_x));
                let help_height = help_height.min(term_height.saturating_sub(help_y));
                
                if help_width > 2 && help_height > 2 {
                    let help_area = ratatui::layout::Rect::new(help_x, help_y, help_width, help_height);
                    
                    // Clear area behind help
                    f.render_widget(ratatui::widgets::Clear, help_area);
                    
                    let help_block = Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .style(Style::default().bg(Color::Black));
                    
                    let help_widget = Paragraph::new(help_text)
                        .block(help_block)
                        .wrap(Wrap { trim: false });
                    f.render_widget(help_widget, help_area);
                }
            }

            // Debug overlay (toggle with F2)
            if app.debug_overlay {
                let dbg_lines = vec![
                    Line::from(format!(
                        "tl={} vis={} max={} off={}",
                        total_lines, visible_lines, max_scroll, scroll_offset
                    )),
                    Line::from(format!(
                        "auto={} scroll={} pad={}",
                        app.auto_scroll, app.scroll, CHAT_PADDING_LINES
                    )),
                    Line::from(format!("msgs={} loading={}", app.messages.len(), app.loading)),
                ];

                let term_width = f.area().width;
                let term_height = f.area().height;
                let dbg_width = 48u16.min(term_width.saturating_sub(2));
                let dbg_height = (dbg_lines.len() as u16 + 2).min(term_height.saturating_sub(2));
                let dbg_x = term_width.saturating_sub(dbg_width + 1);
                let dbg_y = term_height.saturating_sub(dbg_height + 1);

                if dbg_width > 2 && dbg_height > 2 {
                    let dbg_area = ratatui::layout::Rect::new(dbg_x, dbg_y, dbg_width, dbg_height);
                    f.render_widget(ratatui::widgets::Clear, dbg_area);

                    let dbg_block = Block::default()
                        .borders(Borders::ALL)
                        .title(" debug ")
                        .border_style(Style::default().fg(Color::Magenta))
                        .style(Style::default().bg(Color::Black));

                    let dbg_widget = Paragraph::new(dbg_lines)
                        .block(dbg_block)
                        .wrap(Wrap { trim: false });
                    f.render_widget(dbg_widget, dbg_area);
                }
            }
        })?;

        // Kürzeres Poll-Timeout für schnelleres UI-Update (100ms statt 500ms)
        // Das stellt sicher dass neue Nachrichten vom Server schnell angezeigt werden
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Only process key press events, not release events
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                
                // Help screen: any key closes it
                if app.focus == Focus::Help {
                    app.toggle_help();
                    continue;
                }
                
                if app.loading {
                    continue;
                }
                
                // Get terminal width for cursor calculations
                let term_width = terminal.size()?.width.saturating_sub(4) as usize;
                
                match key.code {
                    KeyCode::F(1) => {
                        app.toggle_help();
                    }
                    KeyCode::F(2) => {
                        app.debug_overlay = !app.debug_overlay;
                    }
                    KeyCode::Char('?') if key.modifiers.is_empty() && app.focus != Focus::Input => {
                        app.toggle_help();
                    }
                    KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Clear chat (server + local)
                        let url = format!("{}/messages/clear", app.server_url);
                        match reqwest::Client::new().post(url).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                app.messages.clear();
                                app.messages.push(Message {
                                    role: "system".to_string(),
                                    content: format!("Chat gelöscht (Server + lokal). Verbunden mit {}", app.server_url),
                                    timestamp: Local::now().format("%H:%M:%S").to_string(),
                                    timestamp_ms: Some(now_ms()),
                                });
                                app.last_error = None;
                            }
                            Ok(resp) => {
                                app.last_error = Some(format!("Clear fehlgeschlagen: {}", resp.status()));
                            }
                            Err(e) => {
                                app.last_error = Some(format!("Clear fehlgeschlagen: {}", e));
                            }
                        }
                    }
                    KeyCode::Char('d') | KeyCode::Char('D') 
                        if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                        // Clear history file (Ctrl+Shift+D)
                        if app.history_enabled {
                            match ChatHistory::delete() {
                                Ok(_) => {
                                    app.messages.clear();
                                    app.messages.push(Message {
                                        role: "system".to_string(),
                                        content: "Chat Historie gelöscht.".to_string(),
                                        timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                    });
                                    app.last_error = None;
                                }
                                Err(e) => {
                                    app.last_error = Some(format!("Fehler beim Löschen: {}", e));
                                }
                            }
                        } else {
                            app.last_error = Some("History ist deaktiviert (--no-history)".to_string());
                        }
                    }
                    KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Paste from clipboard (Ctrl+V) - only when input is focused
                        if app.focus == Focus::Input {
                            match Clipboard::new() {
                                Ok(mut clipboard) => {
                                    match clipboard.get_text() {
                                        Ok(text) => {
                                            // Insert at cursor position (convert char pos to byte pos)
                                            let byte_pos: usize = app.input.chars().take(app.cursor_pos).map(|c| c.len_utf8()).sum();
                                            app.input.insert_str(byte_pos, &text);
                                            app.cursor_pos += text.chars().count();
                                        }
                                        Err(_) => {
                                            app.last_error = Some("Clipboard ist leer oder nicht verfügbar".to_string());
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.last_error = Some(format!("Clipboard-Fehler: {}", e));
                                }
                            }
                        }
                    }
                    KeyCode::Tab => {
                        // Toggle focus between input and chat
                        app.toggle_focus();
                    }
                    KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Command history navigation with Ctrl+Up
                        if app.focus == Focus::Input {
                            app.navigate_history_up();
                        }
                    }
                    KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Command history navigation with Ctrl+Down
                        if app.focus == Focus::Input {
                            app.navigate_history_down();
                        }
                    }
                    KeyCode::Up if key.modifiers.is_empty() => {
                        match app.focus {
                            Focus::Input => app.cursor_up(term_width),
                            Focus::Chat => app.scroll_up(),
                            Focus::Help => {}
                        }
                    }
                    KeyCode::Down if key.modifiers.is_empty() => {
                        match app.focus {
                            Focus::Input => app.cursor_down(term_width),
                            Focus::Chat => app.scroll_down(),
                            Focus::Help => {}
                        }
                    }
                    KeyCode::Left if app.focus == Focus::Input => {
                        if app.cursor_pos > 0 {
                            app.cursor_pos -= 1;
                        }
                    }
                    KeyCode::Right if app.focus == Focus::Input => {
                        if app.cursor_pos < app.input.len() {
                            app.cursor_pos += 1;
                        }
                    }
                    KeyCode::Home if app.focus == Focus::Input => {
                        // Move to start of current line
                        let (line, _) = app.cursor_line_col(term_width);
                        if line == 0 {
                            app.cursor_pos = 0;
                        } else {
                            // Find start of current line
                            let mut current_line = 0;
                            let mut line_start = 0;
                            let mut col = 0;
                            
                            for (i, ch) in app.input.chars().enumerate() {
                                if current_line == line {
                                    line_start = i;
                                    break;
                                }
                                if ch == '\n' {
                                    current_line += 1;
                                    col = 0;
                                } else {
                                    col += 1;
                                    if col >= term_width {
                                        current_line += 1;
                                        col = 0;
                                    }
                                }
                            }
                            app.cursor_pos = line_start;
                        }
                    }
                    KeyCode::End if app.focus == Focus::Input => {
                        // Move to end of current line
                        let (line, _) = app.cursor_line_col(term_width);
                        let total_lines = app.input_total_lines(term_width);
                        
                        if line >= total_lines - 1 {
                            app.cursor_pos = app.input.len();
                        } else {
                            // Find end of current line
                            let mut current_line = 0;
                            let mut col = 0;
                            
                            for (i, ch) in app.input.chars().enumerate() {
                                if current_line > line {
                                    app.cursor_pos = i.saturating_sub(1);
                                    break;
                                }
                                if ch == '\n' {
                                    if current_line == line {
                                        app.cursor_pos = i;
                                        break;
                                    }
                                    current_line += 1;
                                    col = 0;
                                } else {
                                    col += 1;
                                    if col >= term_width {
                                        if current_line == line {
                                            app.cursor_pos = i + 1;
                                            break;
                                        }
                                        current_line += 1;
                                        col = 0;
                                    }
                                }
                            }
                        }
                    }
                    KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => {
                        app.scroll_up();
                    }
                    KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
                        app.scroll_down();
                    }
                    KeyCode::Home if app.focus == Focus::Chat => {
                        app.jump_to_top();
                    }
                    KeyCode::End if app.focus == Focus::Chat => {
                        app.jump_to_bottom();
                    }
                    KeyCode::PageUp if app.focus == Focus::Chat => {
                        app.scroll_page_up(10);
                    }
                    KeyCode::PageDown if app.focus == Focus::Chat => {
                        app.scroll_page_down(10);
                    }
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Send message with Ctrl+S (alternative to Ctrl+Enter)
                        if !app.input.trim().is_empty() {
                            let user_msg = app.input.trim().to_string();
                            
                            // Add to command history
                            app.command_history.push(user_msg.clone());
                            app.history_index = None;
                            
                            // Add user message
                            app.messages.push(Message {
                                role: "user".to_string(),
                                content: user_msg.clone(),
                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                            });
                            app.input.clear();
                            app.cursor_pos = 0;
                            app.input_scroll = 0;
                            app.loading = true;
                            app.connection_status = "Sending...".to_string();
                            app.last_error = None;
                            app.scroll_to_bottom();
                            
                            // Send request in background
                            let server_url = app.server_url.clone();
                            let handle = tokio::spawn(async move {
                                let client = reqwest::Client::new();
                                let result = client
                                    .post(format!("{}/chat", server_url))
                                    .json(&ChatRequest { message: user_msg })
                                    .timeout(std::time::Duration::from_secs(120))
                                    .send()
                                    .await;
                                
                                match result {
                                    Ok(response) => {
                                        match response.json::<ChatResponse>().await {
                                            Ok(data) => Ok(data.content),
                                            Err(e) => Err(format!("Failed to parse response: {}", e)),
                                        }
                                    }
                                    Err(e) => Err(format!("Connection error: {}", e)),
                                }
                            });
                            
                            // Wait for response with UI updates
                            loop {
                                terminal.draw(|f| {
                                    let chunks = Layout::default()
                                        .direction(Direction::Vertical)
                                        .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(1)])
                                        .split(f.area());

                                    let mut lines: Vec<Line> = Vec::new();
                                    for msg in &app.messages {
                                        let (prefix, style) = match msg.role.as_str() {
                                            "user" => ("Du: ", Style::default().fg(Color::Cyan)),
                                            "assistant" => ("Hank: ", Style::default().fg(Color::Green)),
                                            "system" => ("", Style::default().fg(Color::DarkGray)),
                                            _ => ("", Style::default()),
                                        };
                                        
                                        if !msg.role.is_empty() && msg.role != "system" {
                                            lines.push(Line::from(vec![
                                                Span::styled(&msg.timestamp, Style::default().fg(Color::DarkGray)),
                                                Span::raw(" "),
                                                Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                                                Span::styled(msg.content.lines().next().unwrap_or(""), style),
                                            ]));
                                            for line in msg.content.lines().skip(1) {
                                                lines.push(Line::from(Span::styled(line, style)));
                                            }
                                        } else {
                                            lines.push(Line::from(Span::styled(&msg.content, style)));
                                        }
                                        lines.push(Line::from(""));
                                    }
                                    lines.push(Line::from(Span::styled(
                                        "Hank denkt nach...",
                                        Style::default().fg(Color::Yellow),
                                    )));

                                    // Auto-scroll to bottom
                                    let total_lines = lines.len() as u16;
                                    let visible_lines = chunks[0].height.saturating_sub(2);
                                    let scroll_offset = total_lines.saturating_sub(visible_lines);

                                    let messages = Paragraph::new(lines)
                                        .block(Block::default().borders(Borders::ALL).title(" Chat "))
                                        .wrap(Wrap { trim: false })
                                        .scroll((scroll_offset, 0));
                                    f.render_widget(messages, chunks[0]);

                                    let input = Paragraph::new("")
                                        .block(Block::default().borders(Borders::ALL).title(" Warte... "))
                                        .style(Style::default().fg(Color::DarkGray));
                                    f.render_widget(input, chunks[1]);
                                    
                                    let status_text = format!(" {} | Sending request...", app.server_url);
                                    let status = Paragraph::new(status_text)
                                        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
                                    f.render_widget(status, chunks[2]);
                                })?;

                                if handle.is_finished() {
                                    match handle.await {
                                        Ok(Ok(content)) => {
                                            app.messages.push(Message {
                                                role: "assistant".to_string(),
                                                content,
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.connection_status = "Connected".to_string();
                                            app.scroll_to_bottom();
                                        }
                                        Ok(Err(err)) => {
                                            app.messages.push(Message {
                                                role: "error".to_string(),
                                                content: err.clone(),
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.last_error = Some(err);
                                            app.connection_status = "Error".to_string();
                                            app.scroll_to_bottom();
                                        }
                                        Err(e) => {
                                            let err_msg = format!("Task failed: {}", e);
                                            app.messages.push(Message {
                                                role: "error".to_string(),
                                                content: err_msg.clone(),
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.last_error = Some(err_msg);
                                            app.connection_status = "Error".to_string();
                                            app.scroll_to_bottom();
                                        }
                                    }
                                    app.loading = false;
                                    break;
                                }

                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Send message with Ctrl+Enter (may not work in all terminals)
                        if !app.input.trim().is_empty() {
                            let user_msg = app.input.trim().to_string();
                            
                            // Add to command history
                            app.command_history.push(user_msg.clone());
                            app.history_index = None;
                            
                            // Add user message
                            app.messages.push(Message {
                                role: "user".to_string(),
                                content: user_msg.clone(),
                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                            });
                            app.input.clear();
                            app.cursor_pos = 0;
                            app.input_scroll = 0;
                            app.loading = true;
                            app.connection_status = "Sending...".to_string();
                            app.last_error = None;
                            app.scroll_to_bottom();
                            
                            // Send request in background
                            let server_url = app.server_url.clone();
                            let handle = tokio::spawn(async move {
                                let client = reqwest::Client::new();
                                let result = client
                                    .post(format!("{}/chat", server_url))
                                    .json(&ChatRequest { message: user_msg })
                                    .timeout(std::time::Duration::from_secs(120))
                                    .send()
                                    .await;
                                
                                match result {
                                    Ok(response) => {
                                        match response.json::<ChatResponse>().await {
                                            Ok(data) => Ok(data.content),
                                            Err(e) => Err(format!("Failed to parse response: {}", e)),
                                        }
                                    }
                                    Err(e) => Err(format!("Connection error: {}", e)),
                                }
                            });
                            
                            // Wait for response with UI updates
                            loop {
                                terminal.draw(|f| {
                                    let chunks = Layout::default()
                                        .direction(Direction::Vertical)
                                        .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(1)])
                                        .split(f.area());

                                    let mut lines: Vec<Line> = Vec::new();
                                    for msg in &app.messages {
                                        let (prefix, style) = match msg.role.as_str() {
                                            "user" => ("Du: ", Style::default().fg(Color::Cyan)),
                                            "assistant" => ("Hank: ", Style::default().fg(Color::Green)),
                                            "system" => ("", Style::default().fg(Color::DarkGray)),
                                            _ => ("", Style::default()),
                                        };
                                        
                                        if !msg.role.is_empty() && msg.role != "system" {
                                            lines.push(Line::from(vec![
                                                Span::styled(&msg.timestamp, Style::default().fg(Color::DarkGray)),
                                                Span::raw(" "),
                                                Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                                                Span::styled(msg.content.lines().next().unwrap_or(""), style),
                                            ]));
                                            for line in msg.content.lines().skip(1) {
                                                lines.push(Line::from(Span::styled(line, style)));
                                            }
                                        } else {
                                            lines.push(Line::from(Span::styled(&msg.content, style)));
                                        }
                                        lines.push(Line::from(""));
                                    }
                                    lines.push(Line::from(Span::styled(
                                        "Hank denkt nach...",
                                        Style::default().fg(Color::Yellow),
                                    )));

                                    // Auto-scroll to bottom
                                    let total_lines = lines.len() as u16;
                                    let visible_lines = chunks[0].height.saturating_sub(2);
                                    let scroll_offset = total_lines.saturating_sub(visible_lines);

                                    let messages = Paragraph::new(lines)
                                        .block(Block::default().borders(Borders::ALL).title(" Chat "))
                                        .wrap(Wrap { trim: false })
                                        .scroll((scroll_offset, 0));
                                    f.render_widget(messages, chunks[0]);

                                    let input = Paragraph::new("")
                                        .block(Block::default().borders(Borders::ALL).title(" Warte... "))
                                        .style(Style::default().fg(Color::DarkGray));
                                    f.render_widget(input, chunks[1]);
                                    
                                    let status_text = format!(" {} | Sending request...", app.server_url);
                                    let status = Paragraph::new(status_text)
                                        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
                                    f.render_widget(status, chunks[2]);
                                })?;

                                if handle.is_finished() {
                                    match handle.await {
                                        Ok(Ok(content)) => {
                                            app.messages.push(Message {
                                                role: "assistant".to_string(),
                                                content,
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.connection_status = "Connected".to_string();
                                            app.scroll_to_bottom();
                                        }
                                        Ok(Err(err)) => {
                                            app.messages.push(Message {
                                                role: "error".to_string(),
                                                content: err.clone(),
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.last_error = Some(err);
                                            app.connection_status = "Error".to_string();
                                            app.scroll_to_bottom();
                                        }
                                        Err(e) => {
                                            let err_msg = format!("Task failed: {}", e);
                                            app.messages.push(Message {
                                                role: "error".to_string(),
                                                content: err_msg.clone(),
                                                timestamp: Local::now().format("%H:%M:%S").to_string(),
                        timestamp_ms: Some(now_ms()),
                                            });
                                            app.last_error = Some(err_msg);
                                            app.connection_status = "Error".to_string();
                                            app.scroll_to_bottom();
                                        }
                                    }
                                    app.loading = false;
                                    break;
                                }

                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    KeyCode::Enter if app.focus == Focus::Input => {
                        // Insert newline with Enter
                        let byte_pos: usize = app.input.chars().take(app.cursor_pos).map(|c| c.len_utf8()).sum();
                        app.input.insert(byte_pos, '\n');
                        app.cursor_pos += 1;
                        app.history_index = None;
                    }
                    KeyCode::Char(c) if app.focus == Focus::Input => {
                        let byte_pos: usize = app.input.chars().take(app.cursor_pos).map(|c| c.len_utf8()).sum();
                        app.input.insert(byte_pos, c);
                        app.cursor_pos += 1;
                        app.history_index = None;
                    }
                    KeyCode::Backspace if app.focus == Focus::Input => {
                        if app.cursor_pos > 0 {
                            app.cursor_pos -= 1;
                            let byte_pos: usize = app.input.chars().take(app.cursor_pos).map(|c| c.len_utf8()).sum();
                            let char_len = app.input.chars().nth(app.cursor_pos).map(|c| c.len_utf8()).unwrap_or(1);
                            app.input.drain(byte_pos..byte_pos + char_len);
                            app.history_index = None;
                        }
                    }
                    KeyCode::Delete if app.focus == Focus::Input => {
                        if app.cursor_pos < app.input.chars().count() {
                            let byte_pos: usize = app.input.chars().take(app.cursor_pos).map(|c| c.len_utf8()).sum();
                            let char_len = app.input.chars().nth(app.cursor_pos).map(|c| c.len_utf8()).unwrap_or(1);
                            app.input.drain(byte_pos..byte_pos + char_len);
                            app.history_index = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    
    Ok(())
}
