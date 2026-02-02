use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};
use std::{env, fs, io, path::PathBuf};
use unicode_width::UnicodeWidthStr;
use chrono::Local;

#[derive(Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize, Deserialize)]
struct Session {
    name: String,
    created: String,
    messages: Vec<Message>,
}

#[derive(PartialEq)]
enum Mode {
    Normal,
    SessionPicker,
}

struct App {
    input: String,
    cursor_pos: usize,  // Cursor-Position in Zeichen (nicht Bytes)
    messages: Vec<Message>,
    server_url: String,
    loading: bool,
    scroll: u16,
    mode: Mode,
    sessions: Vec<String>,
    session_list_state: ListState,
}

#[derive(Serialize)]
struct ChatRequest {
    message: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    content: String,
    complete: bool,
}

fn sessions_dir() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hank-tui")
        .join("sessions");
    fs::create_dir_all(&config_dir).ok();
    config_dir
}

fn list_sessions() -> Vec<String> {
    let dir = sessions_dir();
    let mut sessions: Vec<String> = fs::read_dir(&dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".json") {
                        Some(name.trim_end_matches(".json").to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    sessions.sort();
    sessions.reverse(); // Neueste zuerst
    sessions
}

fn save_session(messages: &[Message]) -> Result<String, Box<dyn std::error::Error>> {
    let name = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    let session = Session {
        name: name.clone(),
        created: Local::now().to_rfc3339(),
        messages: messages.to_vec(),
    };
    let path = sessions_dir().join(format!("{}.json", name));
    fs::write(&path, serde_json::to_string_pretty(&session)?)?;
    Ok(name)
}

fn load_session(name: &str) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let path = sessions_dir().join(format!("{}.json", name));
    let data = fs::read_to_string(&path)?;
    let session: Session = serde_json::from_str(&data)?;
    Ok(session.messages)
}

impl App {
    fn new(server_url: String) -> Self {
        Self {
            input: String::new(),
            cursor_pos: 0,
            messages: Vec::new(),  // Start mit leerem Chat
            server_url,
            loading: false,
            scroll: 0,
            mode: Mode::Normal,
            sessions: Vec::new(),
            session_list_state: ListState::default(),
        }
    }

    fn insert_char(&mut self, c: char) {
        let byte_pos = self.input
            .char_indices()
            .nth(self.cursor_pos)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        self.input.insert(byte_pos, c);
        self.cursor_pos += 1;
    }

    fn delete_char_before_cursor(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            let byte_pos = self.input
                .char_indices()
                .nth(self.cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(self.input.len());
            let next_byte = self.input
                .char_indices()
                .nth(self.cursor_pos + 1)
                .map(|(i, _)| i)
                .unwrap_or(self.input.len());
            self.input.replace_range(byte_pos..next_byte, "");
        }
    }

    fn delete_char_at_cursor(&mut self) {
        let char_count = self.input.chars().count();
        if self.cursor_pos < char_count {
            let byte_pos = self.input
                .char_indices()
                .nth(self.cursor_pos)
                .map(|(i, _)| i)
                .unwrap_or(self.input.len());
            let next_byte = self.input
                .char_indices()
                .nth(self.cursor_pos + 1)
                .map(|(i, _)| i)
                .unwrap_or(self.input.len());
            self.input.replace_range(byte_pos..next_byte, "");
        }
    }

    fn move_cursor_left(&mut self) {
        self.cursor_pos = self.cursor_pos.saturating_sub(1);
    }

    fn move_cursor_right(&mut self) {
        let char_count = self.input.chars().count();
        if self.cursor_pos < char_count {
            self.cursor_pos += 1;
        }
    }

    fn cursor_position(&self, input_width: u16) -> (u16, u16) {
        // Berechne Cursor-Position unter Berücksichtigung von Wort-Wrap
        // ratatui wrapped bei Wortgrenzen, nicht mitten im Wort
        let text_before_cursor: String = self.input.chars().take(self.cursor_pos).collect();
        
        let mut x: u16 = 0;
        let mut y: u16 = 0;
        
        for c in text_before_cursor.chars() {
            let char_width = UnicodeWidthStr::width(c.to_string().as_str()) as u16;
            
            // Prüfen ob das nächste Zeichen noch passt
            if x + char_width > input_width {
                // Neue Zeile
                y += 1;
                x = char_width;
            } else {
                x += char_width;
            }
        }
        
        (x, y)
    }

    fn open_session_picker(&mut self) {
        self.sessions = list_sessions();
        self.session_list_state = ListState::default();
        if !self.sessions.is_empty() {
            self.session_list_state.select(Some(0));
        }
        self.mode = Mode::SessionPicker;
    }

    fn close_session_picker(&mut self) {
        self.mode = Mode::Normal;
    }

    fn session_picker_up(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let i = match self.session_list_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.session_list_state.select(Some(i));
    }

    fn session_picker_down(&mut self) {
        if self.sessions.is_empty() {
            return;
        }
        let i = match self.session_list_state.selected() {
            Some(i) => (i + 1).min(self.sessions.len() - 1),
            None => 0,
        };
        self.session_list_state.select(Some(i));
    }

    fn load_selected_session(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(i) = self.session_list_state.selected() {
            if let Some(name) = self.sessions.get(i) {
                self.messages = load_session(name)?;
                self.scroll = 0;
            }
        }
        self.mode = Mode::Normal;
        Ok(())
    }

    fn save_current_session(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        // Nur speichern wenn es was zu speichern gibt
        let saveable: Vec<_> = self.messages
            .iter()
            .filter(|m| m.role == "user" || m.role == "assistant")
            .cloned()
            .collect();
        if saveable.is_empty() {
            return Ok("Nichts zu speichern".to_string());
        }
        save_session(&saveable)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server_url = env::var("HANK_SERVER")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(server_url);

    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(3),
                ])
                .split(f.area());

            // Chat-Verlauf
            let mut lines: Vec<Line> = Vec::new();
            
            if app.messages.is_empty() {
                lines.push(Line::from(Span::styled(
                    "Neuer Chat - Ctrl+O zum Laden einer Session",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                )));
            }
            
            for msg in &app.messages {
                let (prefix, style) = match msg.role.as_str() {
                    "user" => ("Du: ", Style::default().fg(Color::Cyan)),
                    "assistant" => ("Hank: ", Style::default().fg(Color::Green)),
                    "system" => ("", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                    _ => ("", Style::default()),
                };
                
                for (i, line) in msg.content.lines().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                            Span::styled(line, style),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(
                            format!("{:width$}{}", "", line, width = prefix.width()),
                            style,
                        )));
                    }
                }
                lines.push(Line::from(""));
            }

            if app.loading {
                lines.push(Line::from(Span::styled(
                    "Hank denkt nach...",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC),
                )));
            }

            let messages_widget = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Chat "))
                .wrap(Wrap { trim: false })
                .scroll((app.scroll, 0));
            f.render_widget(messages_widget, chunks[0]);

            // Input
            let input_title = " Nachricht (Enter=senden, Ctrl+S=speichern, Ctrl+O=laden, Esc=beenden) ";
            let input_widget = Paragraph::new(app.input.as_str())
                .block(Block::default().borders(Borders::ALL).title(input_title))
                .style(if app.loading {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                });
            f.render_widget(input_widget, chunks[1]);

            // Cursor - korrekte Position mit Unicode-Breite und Wrap
            if !app.loading && app.mode == Mode::Normal {
                let input_width = chunks[1].width.saturating_sub(2); // Abzüglich Borders
                let (cursor_x, cursor_y) = app.cursor_position(input_width);
                
                f.set_cursor_position((
                    chunks[1].x + 1 + cursor_x,
                    chunks[1].y + 1 + cursor_y
                ));
            }

            // Session-Picker Popup
            if app.mode == Mode::SessionPicker {
                let area = centered_rect(60, 60, f.area());
                f.render_widget(Clear, area);
                
                let items: Vec<ListItem> = app.sessions
                    .iter()
                    .map(|s| ListItem::new(s.as_str()))
                    .collect();
                
                let list = List::new(items)
                    .block(Block::default()
                        .borders(Borders::ALL)
                        .title(" Sessions laden (Enter=laden, Esc=abbrechen) "))
                    .highlight_style(Style::default()
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                
                f.render_stateful_widget(list, area, &mut app.session_list_state);
            }
        })?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Session Picker Modus
                if app.mode == Mode::SessionPicker {
                    match key.code {
                        KeyCode::Esc => app.close_session_picker(),
                        KeyCode::Up => app.session_picker_up(),
                        KeyCode::Down => app.session_picker_down(),
                        KeyCode::Enter => {
                            if let Err(e) = app.load_selected_session() {
                                app.messages.push(Message {
                                    role: "system".to_string(),
                                    content: format!("Fehler beim Laden: {}", e),
                                });
                                app.mode = Mode::Normal;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Normal Modus
                if app.loading {
                    continue;
                }
                
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        match app.save_current_session() {
                            Ok(name) => {
                                app.messages.push(Message {
                                    role: "system".to_string(),
                                    content: format!("Session gespeichert: {}", name),
                                });
                            }
                            Err(e) => {
                                app.messages.push(Message {
                                    role: "system".to_string(),
                                    content: format!("Fehler beim Speichern: {}", e),
                                });
                            }
                        }
                    }
                    KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.open_session_picker();
                    }
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Neue Session
                        app.messages.clear();
                        app.input.clear();
                        app.cursor_pos = 0;
                        app.scroll = 0;
                    }
                    KeyCode::Enter => {
                        if !app.input.trim().is_empty() {
                            app.messages.push(Message {
                                role: "user".to_string(),
                                content: app.input.clone(),
                            });
                            let user_msg = app.input.clone();
                            app.input.clear();
                            app.cursor_pos = 0;
                            app.loading = true;
                            
                            let server_url = app.server_url.clone();
                            let handle = tokio::spawn(async move {
                                let client = reqwest::Client::new();
                                client
                                    .post(format!("{}/chat", server_url))
                                    .json(&ChatRequest { message: user_msg })
                                    .timeout(std::time::Duration::from_secs(120))
                                    .send()
                                    .await
                                    .ok()
                                    .and_then(|r| futures::executor::block_on(r.json::<ChatResponse>()).ok())
                            });
                            
                            loop {
                                terminal.draw(|f| {
                                    let chunks = Layout::default()
                                        .direction(Direction::Vertical)
                                        .constraints([Constraint::Min(3), Constraint::Length(3)])
                                        .split(f.area());

                                    let mut lines: Vec<Line> = Vec::new();
                                    for msg in &app.messages {
                                        let (prefix, style) = match msg.role.as_str() {
                                            "user" => ("Du: ", Style::default().fg(Color::Cyan)),
                                            "assistant" => ("Hank: ", Style::default().fg(Color::Green)),
                                            "system" => ("", Style::default().fg(Color::DarkGray)),
                                            _ => ("", Style::default()),
                                        };
                                        for (i, line) in msg.content.lines().enumerate() {
                                            if i == 0 {
                                                lines.push(Line::from(vec![
                                                    Span::styled(prefix, style.add_modifier(Modifier::BOLD)),
                                                    Span::styled(line, style),
                                                ]));
                                            } else {
                                                lines.push(Line::from(Span::styled(line, style)));
                                            }
                                        }
                                        lines.push(Line::from(""));
                                    }
                                    lines.push(Line::from(Span::styled(
                                        "Hank denkt nach...",
                                        Style::default().fg(Color::Yellow),
                                    )));

                                    let messages = Paragraph::new(lines)
                                        .block(Block::default().borders(Borders::ALL).title(" Chat "))
                                        .wrap(Wrap { trim: false });
                                    f.render_widget(messages, chunks[0]);

                                    let input = Paragraph::new("")
                                        .block(Block::default().borders(Borders::ALL).title(" Warte... "))
                                        .style(Style::default().fg(Color::DarkGray));
                                    f.render_widget(input, chunks[1]);
                                })?;

                                if handle.is_finished() {
                                    match handle.await {
                                        Ok(Some(resp)) => {
                                            app.messages.push(Message {
                                                role: "assistant".to_string(),
                                                content: resp.content,
                                            });
                                        }
                                        _ => {
                                            app.messages.push(Message {
                                                role: "system".to_string(),
                                                content: "Fehler bei der Anfrage".to_string(),
                                            });
                                        }
                                    }
                                    app.loading = false;
                                    break;
                                }

                                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            }
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char(c) => app.insert_char(c),
                    KeyCode::Backspace => app.delete_char_before_cursor(),
                    KeyCode::Delete => app.delete_char_at_cursor(),
                    KeyCode::Left => app.move_cursor_left(),
                    KeyCode::Right => app.move_cursor_right(),
                    KeyCode::Home => app.cursor_pos = 0,
                    KeyCode::End => app.cursor_pos = app.input.chars().count(),
                    KeyCode::Up => app.scroll = app.scroll.saturating_add(1),
                    KeyCode::Down => app.scroll = app.scroll.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    
    Ok(())
}
