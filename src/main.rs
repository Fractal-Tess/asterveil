use std::{
    collections::BTreeSet,
    io::{self, Stdout},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
};

const APP_NAME: &str = "Asterveil";
const DEFAULT_FAN_SPEED: u32 = 100;
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Debug)]
struct GpuState {
    index: u32,
    name: String,
    temperature_c: String,
    gpu_utilization_pct: String,
    graphics_clock_mhz: String,
    fan_speed_pct: String,
    memory_utilization_pct: String,
    memory_used_mib: String,
    memory_total_mib: String,
    draw_w: String,
    limit_w: String,
    default_w: String,
    min_w: String,
    max_w: String,
    fan_control_state: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionKind {
    Power,
    Fan,
}

impl ActionKind {
    fn title(self) -> &'static str {
        match self {
            ActionKind::Power => "Power Limit",
            ActionKind::Fan => "Fan Speed",
        }
    }

    fn helper(self) -> &'static str {
        match self {
            ActionKind::Power => "Choose a power profile for the active target scope.",
            ActionKind::Fan => "Choose a fixed fan preset or return control to auto.",
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            ActionKind::Power => "p",
            ActionKind::Fan => "f",
        }
    }

    fn choices(self) -> &'static [ChoiceItem] {
        match self {
            ActionKind::Power => POWER_CHOICES,
            ActionKind::Fan => FAN_CHOICES,
        }
    }
}

#[derive(Clone, Debug)]
enum Overlay {
    Choice { action: ActionKind, cursor: usize },
    Prompt { action: ActionKind, input: String },
}

#[derive(Clone, Copy, Debug)]
struct ChoiceItem {
    label: &'static str,
    value: &'static str,
    detail: &'static str,
}

const POWER_CHOICES: &[ChoiceItem] = &[
    ChoiceItem {
        label: "Eco",
        value: "eco",
        detail: "Clamp to each card's minimum power limit.",
    },
    ChoiceItem {
        label: "Balanced",
        value: "balanced",
        detail: "Set each card to the midpoint between min and max.",
    },
    ChoiceItem {
        label: "Default",
        value: "default",
        detail: "Restore the vendor default power limit.",
    },
    ChoiceItem {
        label: "Performance",
        value: "performance",
        detail: "Clamp to each card's maximum power limit.",
    },
    ChoiceItem {
        label: "Custom...",
        value: "custom",
        detail: "Enter an exact watt limit.",
    },
];

const FAN_CHOICES: &[ChoiceItem] = &[
    ChoiceItem {
        label: "Auto",
        value: "auto",
        detail: "Return fan control to the driver.",
    },
    ChoiceItem {
        label: "40%",
        value: "40",
        detail: "Low fixed fan preset.",
    },
    ChoiceItem {
        label: "60%",
        value: "60",
        detail: "Moderate fixed fan preset.",
    },
    ChoiceItem {
        label: "80%",
        value: "80",
        detail: "High fixed fan preset.",
    },
    ChoiceItem {
        label: "100%",
        value: "100",
        detail: "Maximum fixed fan preset.",
    },
    ChoiceItem {
        label: "Custom...",
        value: "custom",
        detail: "Enter an exact fan percentage.",
    },
];

#[derive(Debug)]
struct App {
    display: String,
    gpus: Vec<GpuState>,
    cursor: usize,
    selected: BTreeSet<usize>,
    overlay: Option<Overlay>,
    message: String,
    history: Vec<String>,
    last_refresh: Instant,
    quit: bool,
}

impl App {
    fn new(display: String) -> Self {
        Self {
            display,
            gpus: Vec::new(),
            cursor: 0,
            selected: BTreeSet::new(),
            overlay: None,
            message: "Press `?` for help".to_string(),
            history: Vec::new(),
            last_refresh: Instant::now() - REFRESH_INTERVAL,
            quit: false,
        }
    }

    fn push_message(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.message = message.clone();
        self.history.insert(0, message);
        self.history.truncate(4);
    }

    fn refresh(&mut self) -> Result<()> {
        self.gpus = query_gpus(&self.display)?;

        if self.gpus.is_empty() {
            self.push_message("No NVIDIA GPUs found");
        } else {
            self.cursor = self.cursor.min(self.gpus.len().saturating_sub(1));
            self.selected = self
                .selected
                .iter()
                .copied()
                .filter(|index| *index < self.gpus.len())
                .collect();
            self.push_message(format!("Refreshed {} GPU(s)", self.gpus.len()));
        }

        self.last_refresh = Instant::now();
        Ok(())
    }

    fn move_cursor_down(&mut self) {
        if self.gpus.is_empty() {
            return;
        }

        self.cursor = (self.cursor + 1) % self.gpus.len();
        self.push_message(format!("Focus {}", self.focus_title()));
    }

    fn move_cursor_up(&mut self) {
        if self.gpus.is_empty() {
            return;
        }

        self.cursor = if self.cursor == 0 {
            self.gpus.len() - 1
        } else {
            self.cursor - 1
        };
        self.push_message(format!("Focus {}", self.focus_title()));
    }

    fn toggle_selected(&mut self) {
        if self.gpus.is_empty() {
            return;
        }

        if !self.selected.insert(self.cursor) {
            self.selected.remove(&self.cursor);
        }

        let current_title = self.focus_title();
        let was_selected = self.selected.contains(&self.cursor);
        self.cursor = (self.cursor + 1) % self.gpus.len();

        self.push_message(format!(
            "{} {} | focus {}",
            current_title,
            if was_selected { "selected" } else { "cleared" },
            self.focus_title()
        ));
    }

    fn select_all(&mut self) {
        self.selected = (0..self.gpus.len()).collect();
        self.push_message(format!("Selected {} GPU(s)", self.selected.len()));
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.push_message("Selection cleared");
    }

    fn open_prompt(&mut self, action: ActionKind) {
        let default_fan_speed = DEFAULT_FAN_SPEED.to_string();
        let default = match action {
            ActionKind::Power => 1,
            ActionKind::Fan => FAN_CHOICES
                .iter()
                .position(|choice| choice.value == default_fan_speed)
                .unwrap_or(0),
        };

        self.overlay = Some(Overlay::Choice {
            action,
            cursor: default,
        });
    }

    fn close_overlay(&mut self) {
        self.overlay = None;
    }

    fn open_custom_prompt(&mut self, action: ActionKind) {
        let input = match action {
            ActionKind::Power => self
                .focused_gpu()
                .map(|gpu| gpu.limit_w.clone())
                .unwrap_or_else(|| "0".to_string()),
            ActionKind::Fan => self
                .focused_gpu()
                .map(|gpu| gpu.fan_speed_pct.clone())
                .unwrap_or_else(|| DEFAULT_FAN_SPEED.to_string()),
        };

        self.overlay = Some(Overlay::Prompt { action, input });
    }

    fn action_indices(&self) -> Vec<usize> {
        if self.selected.is_empty() {
            vec![self.cursor]
        } else {
            self.selected.iter().copied().collect()
        }
    }

    fn focused_gpu(&self) -> Option<&GpuState> {
        self.gpus.get(self.cursor)
    }

    fn focus_title(&self) -> String {
        self.focused_gpu()
            .map(|gpu| format!("GPU {} {}", gpu.index, gpu.name))
            .unwrap_or_else(|| "GPU".to_string())
    }

    fn target_summary(&self) -> String {
        if self.gpus.is_empty() {
            return "No GPU target".to_string();
        }

        if self.selected.is_empty() {
            self.focus_title()
        } else {
            format!("{} selected GPU(s)", self.selected.len())
        }
    }

    fn selection_scope_hint(&self) -> &'static str {
        if self.selected.is_empty() {
            "Actions apply to the focused GPU."
        } else {
            "Actions apply to the selected GPUs."
        }
    }

    fn prompt_target_summary(&self) -> String {
        if self.selected.is_empty() {
            self.focus_title()
        } else {
            format!("{} selected GPU(s)", self.selected.len())
        }
    }

    fn fan_scope_hint(&self) -> &'static str {
        "Fan speed currently writes every discovered fan target."
    }

    fn focus_snapshot(&self) -> String {
        self.focused_gpu()
            .map(|gpu| {
                format!(
                    "{} | {} C | load {} | mem {} | clock {} MHz | fan {} | mode {} | VRAM {}",
                    self.focus_title(),
                    gpu.temperature_c,
                    format_percent(&gpu.gpu_utilization_pct),
                    format_percent(&gpu.memory_utilization_pct),
                    gpu.graphics_clock_mhz,
                    format_percent(&gpu.fan_speed_pct),
                    gpu.fan_control_state.as_deref().unwrap_or("unknown"),
                    format_vram_summary(&gpu.memory_used_mib, &gpu.memory_total_mib),
                )
            })
            .unwrap_or_else(|| "No focused GPU".to_string())
    }

    fn choice_detail(&self, action: ActionKind, choice: ChoiceItem) -> String {
        match action {
            ActionKind::Power => {
                if let Some(gpu) = self.focused_gpu() {
                    match resolve_power_value(choice.value, &gpu.default_w, &gpu.min_w, &gpu.max_w)
                    {
                        Ok(watts) => format!("{} Focused GPU: {} W", choice.detail, watts),
                        Err(_) => choice.detail.to_string(),
                    }
                } else {
                    choice.detail.to_string()
                }
            }
            ActionKind::Fan => choice.detail.to_string(),
        }
    }

    fn apply_power_value(&mut self, value: &str) -> Result<()> {
        let targets = self.action_indices();
        for gpu_index in targets {
            let gpu = self.gpus.get(gpu_index).context("GPU index out of range")?;
            let resolved = resolve_power_value(value, &gpu.default_w, &gpu.min_w, &gpu.max_w)?;
            set_power_limit(gpu.index, &resolved)?;
        }

        self.refresh()?;
        self.push_message(format!("Power set to {}", value.trim()));
        Ok(())
    }

    fn apply_fan_value(&mut self, value: &str) -> Result<()> {
        let targets = self.action_indices();
        if is_auto_fan_value(value) {
            set_manual_fan_control(&targets, false)?;
            self.refresh()?;
            self.push_message("Fan control returned to auto");
            return Ok(());
        }

        let speed = value
            .trim()
            .parse::<u32>()
            .context("fan speed must be a number or default/auto")?;
        set_manual_fan_control(&targets, true)?;
        set_all_fans(speed)?;
        self.refresh()?;
        self.push_message(format!(
            "Fan speed set to {}% across discovered fan target(s)",
            speed
        ));
        Ok(())
    }

    fn apply_default(&mut self) -> Result<()> {
        let targets = self.action_indices();
        for gpu_index in &targets {
            let gpu = self
                .gpus
                .get(*gpu_index)
                .context("GPU index out of range")?;
            set_power_limit(gpu.index, &gpu.default_w)?;
        }

        set_manual_fan_control(&targets, false)?;
        self.refresh()?;
        self.push_message("Applied default settings");
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match self.overlay.clone() {
            Some(Overlay::Choice { action, cursor }) => self.handle_choice_key(key, action, cursor),
            Some(Overlay::Prompt { action, input }) => self.handle_prompt_key(key, action, input),
            None => self.handle_main_key(key),
        }
    }

    fn handle_main_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('r') => self.refresh()?,
            KeyCode::Up => self.move_cursor_up(),
            KeyCode::Down => self.move_cursor_down(),
            KeyCode::Char(' ') => self.toggle_selected(),
            KeyCode::Enter | KeyCode::Char('p') => self.open_prompt(ActionKind::Power),
            KeyCode::Char('f') => self.open_prompt(ActionKind::Fan),
            KeyCode::Char('d') => self.apply_default()?,
            KeyCode::Char('a') => self.select_all(),
            KeyCode::Char('c') => self.clear_selection(),
            KeyCode::Char('?') => {
                self.push_message("Arrows move | p power | f fan | d default | Space select | a all | c clear | r refresh | q quit");
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_choice_key(
        &mut self,
        key: KeyEvent,
        action: ActionKind,
        cursor: usize,
    ) -> Result<()> {
        let choices = action.choices();
        if choices.is_empty() {
            self.close_overlay();
            bail!("no choices available for action");
        }

        let mut next_cursor = cursor.min(choices.len() - 1);

        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                let value = choices
                    .get(next_cursor)
                    .map(|choice| choice.value)
                    .context("invalid choice cursor")?;
                if value == "custom" {
                    self.open_custom_prompt(action);
                } else {
                    self.close_overlay();
                    match action {
                        ActionKind::Power => self.apply_power_value(value)?,
                        ActionKind::Fan => self.apply_fan_value(value)?,
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                next_cursor = (next_cursor + 1) % choices.len();
                if let Some(choice) = choices.get(next_cursor) {
                    self.push_message(format!("{} preset: {}", action.title(), choice.label));
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                next_cursor = if next_cursor == 0 {
                    choices.len() - 1
                } else {
                    next_cursor - 1
                };
                if let Some(choice) = choices.get(next_cursor) {
                    self.push_message(format!("{} preset: {}", action.title(), choice.label));
                }
            }
            _ => {
                self.overlay = Some(Overlay::Choice {
                    action,
                    cursor: next_cursor,
                });
            }
        }

        if !matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            self.overlay = Some(Overlay::Choice {
                action,
                cursor: next_cursor,
            });
        }

        Ok(())
    }

    fn handle_prompt_key(
        &mut self,
        key: KeyEvent,
        action: ActionKind,
        mut input: String,
    ) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.close_overlay();
            }
            KeyCode::Enter => {
                self.close_overlay();
                match action {
                    ActionKind::Power => self.apply_power_value(input.trim())?,
                    ActionKind::Fan => self.apply_fan_value(input.trim())?,
                }
            }
            KeyCode::Backspace => {
                input.pop();
                self.overlay = Some(Overlay::Prompt { action, input });
            }
            KeyCode::Char(c) if c.is_ascii_digit() || (action == ActionKind::Power && c == '.') => {
                input.push(c);
                self.overlay = Some(Overlay::Prompt { action, input });
            }
            _ => {
                self.overlay = Some(Overlay::Prompt { action, input });
            }
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
    let mut app = App::new(display);
    app.refresh().context("initial refresh failed")?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;

        if app.quit {
            return Ok(());
        }

        let timeout = REFRESH_INTERVAL
            .checked_sub(app.last_refresh.elapsed())
            .unwrap_or(Duration::ZERO);

        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
        {
            app.handle_key(key)?;
        }

        if app.last_refresh.elapsed() >= REFRESH_INTERVAL
            && app.overlay.is_none()
            && let Err(err) = app.refresh()
        {
            app.push_message(format!("Refresh failed: {err:#}"));
        }
    }
}

fn draw(frame: &mut Frame<'_>, app: &App) {
    let size = frame.area();
    let outer = Block::default()
        .title(Line::from(vec![
            Span::styled(
                APP_NAME,
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "operator console for NVIDIA power and cooling",
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(outer, size);

    let inner = size.inner(Margin::new(1, 1));
    let sections = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(14),
        Constraint::Length(3),
    ])
    .split(inner);

    draw_header(frame, sections[0], app);
    draw_body(frame, sections[1], app);
    draw_footer(frame, sections[2], app);

    if let Some(overlay) = &app.overlay {
        draw_overlay(frame, size, app, overlay);
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let text = vec![
        Line::from(vec![
            Span::styled("Display ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.display, Style::default().fg(Color::Green)),
            Span::raw("  "),
            Span::styled("GPUs ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.gpus.len().to_string(),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("Target ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                app.target_summary(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Last sync ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}s ago", app.last_refresh.elapsed().as_secs()),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                app.selection_scope_hint(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            app.focus_snapshot(),
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            app.message.clone(),
            Style::default().fg(Color::White),
        )),
    ];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("Overview"))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_body(frame: &mut Frame<'_>, area: Rect, app: &App) {
    draw_gpu_cards(frame, area, app);
}

fn draw_gpu_cards(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let outer = Block::default().title("GPU Fleet").borders(Borders::ALL);
    frame.render_widget(outer, area);

    if app.gpus.is_empty() {
        let empty = Paragraph::new("No NVIDIA GPUs detected.")
            .block(Block::default())
            .wrap(Wrap { trim: true });
        frame.render_widget(empty, area.inner(Margin::new(1, 1)));
        return;
    }

    let inner = area.inner(Margin::new(1, 1));
    let card_height = 7u16;
    let visible_cards = usize::max(1, (inner.height / card_height) as usize);
    let max_start = app.gpus.len().saturating_sub(visible_cards);
    let start = app
        .cursor
        .saturating_sub(visible_cards.saturating_sub(1))
        .min(max_start);
    let end = usize::min(start + visible_cards, app.gpus.len());

    let constraints: Vec<Constraint> = (start..end)
        .map(|_| Constraint::Length(card_height))
        .collect();
    let card_areas = Layout::vertical(constraints).split(inner);

    for (card_area, (i, gpu)) in card_areas
        .iter()
        .zip(app.gpus.iter().enumerate().skip(start).take(end - start))
    {
        let is_cursor = i == app.cursor;
        let is_selected = app.selected.contains(&i);
        let border_style = if is_cursor {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = format!(
            "{}{} GPU {} {}",
            if is_cursor { ">" } else { " " },
            if is_selected { "[x]" } else { "[ ]" },
            gpu.index,
            gpu.name
        );

        let card_block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        frame.render_widget(card_block, *card_area);

        let inner = card_area.inner(Margin::new(1, 1));
        let columns = Layout::horizontal([Constraint::Percentage(72), Constraint::Percentage(28)])
            .split(inner);
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(columns[0]);

        let thermals = Paragraph::new(Line::from(vec![
            Span::styled("Thermals ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "{} C  fan {}  clock {} MHz",
                gpu.temperature_c,
                format_percent(&gpu.fan_speed_pct),
                gpu.graphics_clock_mhz
            )),
        ]));
        frame.render_widget(thermals, rows[0]);

        let load = Paragraph::new(Line::from(vec![
            Span::styled("Load ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!(
                "gpu {}  mem {}",
                format_percent(&gpu.gpu_utilization_pct),
                format_percent(&gpu.memory_utilization_pct)
            )),
        ]));
        frame.render_widget(load, rows[1]);

        let vram = Paragraph::new(Line::from(vec![
            Span::styled("VRAM ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_vram_summary(
                &gpu.memory_used_mib,
                &gpu.memory_total_mib,
            )),
        ]));
        frame.render_widget(vram, rows[2]);

        let fans = Paragraph::new(Line::from(vec![
            Span::styled("Fans ", Style::default().fg(Color::DarkGray)),
            Span::raw(format_percent(&gpu.fan_speed_pct)),
            Span::raw("  "),
            Span::styled("Mode ", Style::default().fg(Color::DarkGray)),
            Span::raw(gpu.fan_control_state.as_deref().unwrap_or("unknown")),
        ]));
        frame.render_widget(fans, rows[3]);

        let power = Paragraph::new(Line::from(vec![
            Span::styled("Power ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("draw {} W  TDP {} W", gpu.draw_w, gpu.limit_w)),
        ]))
        .wrap(Wrap { trim: true });
        frame.render_widget(power, rows[4]);

        let gauges = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(1),
        ])
        .split(columns[1]);

        let gpu_gauge = Gauge::default()
            .label(format!("GPU {}", format_percent(&gpu.gpu_utilization_pct)))
            .ratio(percent_ratio(&gpu.gpu_utilization_pct))
            .gauge_style(Style::default().fg(Color::LightCyan));
        let vram_gauge = Gauge::default()
            .label(format!(
                "VRAM {}",
                format_percent(&gpu.memory_utilization_pct)
            ))
            .ratio(percent_ratio(&gpu.memory_utilization_pct))
            .gauge_style(Style::default().fg(Color::Green));

        frame.render_widget(gpu_gauge, gauges[0]);
        frame.render_widget(vram_gauge, gauges[1]);
    }
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let line = match &app.overlay {
        Some(Overlay::Choice { .. }) => "Up/Down move | Enter confirm | Esc cancel".to_string(),
        Some(Overlay::Prompt { action, .. }) => match action {
            ActionKind::Power => "Type watts | Enter confirm | Esc cancel".to_string(),
            ActionKind::Fan => "Type percent | Enter confirm | Esc cancel".to_string(),
        },
        None => {
            "Up/Down move | Space select | p power | f fan | d default | a all | c clear | r refresh | q quit".to_string()
        }
    };

    let paragraph = Paragraph::new(line)
        .block(Block::default().title("Controls").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_overlay(frame: &mut Frame<'_>, area: Rect, app: &App, overlay: &Overlay) {
    match overlay {
        Overlay::Choice { action, cursor } => {
            let choice_count = action.choices().len() as u16;
            let popup_height =
                (9 + choice_count.saturating_mul(2)).min(area.height.saturating_sub(2));
            let popup_width = area.width.saturating_mul(70).saturating_div(100).max(52);
            let popup = centered_rect_fixed(
                popup_width.min(area.width.saturating_sub(2)),
                popup_height.max(12),
                area,
            );
            frame.render_widget(Clear, popup);
            let content = popup.inner(Margin::new(1, 1));
            let sections =
                Layout::vertical([Constraint::Length(7), Constraint::Min(8)]).split(content);
            let choices = action.choices();
            let selected = choices.get(*cursor).copied().unwrap_or(ChoiceItem {
                label: "-",
                value: "-",
                detail: "",
            });
            let text = vec![
                Line::from(Span::styled(
                    format!("{} for {}", action.title(), app.prompt_target_summary()),
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Shortcut ", Style::default().fg(Color::DarkGray)),
                    Span::raw(action.shortcut()),
                ]),
                Line::from(vec![
                    Span::styled("Selected ", Style::default().fg(Color::DarkGray)),
                    Span::styled(selected.label, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(Span::styled(
                    if *action == ActionKind::Fan {
                        app.fan_scope_hint()
                    } else {
                        app.selection_scope_hint()
                    },
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Enter confirm | Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let header = Paragraph::new(text)
                .block(Block::default().title(action.title()).borders(Borders::ALL))
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: true });

            let items: Vec<ListItem> = choices
                .iter()
                .enumerate()
                .map(|(index, choice)| {
                    let style = if index == *cursor {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };

                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(if index == *cursor { "> " } else { "  " }, style),
                            Span::styled(choice.label, style),
                        ]),
                        Line::from(Span::styled(
                            app.choice_detail(*action, *choice),
                            Style::default().fg(Color::DarkGray),
                        )),
                    ])
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .title(action.helper())
                    .borders(Borders::ALL),
            );

            frame.render_widget(header, sections[0]);
            frame.render_widget(list, sections[1]);
        }
        Overlay::Prompt { action, input } => {
            let popup = centered_rect_fixed(
                area.width
                    .saturating_mul(60)
                    .saturating_div(100)
                    .max(48)
                    .min(area.width.saturating_sub(2)),
                12.min(area.height.saturating_sub(2)).max(8),
                area,
            );
            frame.render_widget(Clear, popup);
            let content = popup.inner(Margin::new(1, 1));
            let title = match action {
                ActionKind::Power => "Custom Power Limit",
                ActionKind::Fan => "Custom Fan Speed",
            };
            let helper = match action {
                ActionKind::Power => "Enter exact watts for the current target scope.",
                ActionKind::Fan => "Enter exact fan percentage for the current target scope.",
            };
            let value_hint = match action {
                ActionKind::Power => "Watts",
                ActionKind::Fan => "Percent",
            };

            let text = vec![
                Line::from(Span::styled(
                    format!("{} for {}", title, app.prompt_target_summary()),
                    Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(helper, Style::default().fg(Color::DarkGray))),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        format!("{value_hint} "),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(input.clone(), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    if *action == ActionKind::Fan {
                        app.fan_scope_hint()
                    } else {
                        app.selection_scope_hint()
                    },
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Enter confirm | Esc cancel",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let prompt = Paragraph::new(text)
                .block(Block::default().title(title).borders(Borders::ALL))
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: true });

            frame.render_widget(prompt, content);
        }
    }
}

fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;

    Rect::new(x, y, width, height)
}

fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {cmd}"))?;

    if !output.status.success() {
        bail!("{cmd} exited with status {}", output.status);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_sudo_status(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new("sudo")
        .arg("-n")
        .arg(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run sudo {cmd}"))?;

    if !status.success() {
        bail!("sudo {cmd} exited with status {status}");
    }

    Ok(())
}

fn query_gpus(display: &str) -> Result<Vec<GpuState>> {
    let output = run_capture(
        "nvidia-smi",
        &[
            "--query-gpu=index,name,temperature.gpu,utilization.gpu,clocks.current.graphics,fan.speed,utilization.memory,memory.used,memory.total,power.draw,power.limit,power.default_limit,power.min_limit,power.max_limit",
            "--format=csv,noheader,nounits",
        ],
    )?;

    parse_gpu_rows(&output, display)
}

fn query_gpu_fan_state(display: &str, index: u32) -> Result<String> {
    run_capture(
        "nvidia-settings",
        &[
            "-c",
            display,
            "-q",
            &format!("[gpu:{index}]/GPUFanControlState"),
            "-t",
        ],
    )
}

fn set_power_limit(index: u32, watts: &str) -> Result<()> {
    run_sudo_status("nvidia-smi", &["-i", &index.to_string(), "-pl", watts])
}

fn set_manual_fan_control(gpu_indices: &[usize], enabled: bool) -> Result<()> {
    let display = display_from_env();
    let gpus = run_capture(
        "nvidia-smi",
        &["--query-gpu=index", "--format=csv,noheader,nounits"],
    )?;

    let state = if enabled { "1" } else { "0" };
    let mut args: Vec<String> = vec!["-c".to_string(), display.clone()];
    for line in gpus.lines() {
        let index = line.trim();
        if index.is_empty() {
            continue;
        }

        let parsed_index = match index.parse::<usize>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !gpu_indices.contains(&parsed_index) {
            continue;
        }

        args.push("-a".to_string());
        args.push(format!("[gpu:{index}]/GPUFanControlState={state}"));
    }

    run_sudo_nvidia_settings(&display, args)
}

fn set_all_fans(speed: u32) -> Result<()> {
    let display = display_from_env();
    let fans = run_capture("nvidia-settings", &["-c", &display, "-q", "fans"])?;
    let mut args: Vec<String> = vec!["-c".to_string(), display.clone()];
    let mut found = false;

    for line in fans.lines() {
        if let Some(index) = extract_fan_index(line) {
            found = true;
            args.push("-a".to_string());
            args.push(format!("[fan:{index}]/GPUTargetFanSpeed={speed}"));
        }
    }

    if !found {
        bail!("no NVIDIA fans found");
    }

    run_sudo_nvidia_settings(&display, args)
}

fn run_sudo_nvidia_settings(display: &str, args: Vec<String>) -> Result<()> {
    let status = Command::new("sudo")
        .arg("-n")
        .arg("nvidia-settings")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("DISPLAY", display)
        .status()
        .context("failed to run sudo nvidia-settings")?;

    if !status.success() {
        bail!("sudo nvidia-settings exited with status {status}");
    }

    Ok(())
}

fn parse_gpu_rows(output: &str, display: &str) -> Result<Vec<GpuState>> {
    let mut gpus = Vec::new();
    for line in output.lines() {
        let parts: Vec<_> = line.split(',').map(|part| part.trim()).collect();
        if parts.len() < 14 {
            continue;
        }

        let index = parts[0].parse::<u32>().context("invalid GPU index")?;
        gpus.push(GpuState {
            index,
            name: parts[1].to_string(),
            temperature_c: parts[2].to_string(),
            gpu_utilization_pct: parts[3].to_string(),
            graphics_clock_mhz: parts[4].to_string(),
            fan_speed_pct: parts[5].to_string(),
            memory_utilization_pct: parts[6].to_string(),
            memory_used_mib: parts[7].to_string(),
            memory_total_mib: parts[8].to_string(),
            draw_w: parts[9].to_string(),
            limit_w: parts[10].to_string(),
            default_w: parts[11].to_string(),
            min_w: parts[12].to_string(),
            max_w: parts[13].to_string(),
            fan_control_state: query_gpu_fan_state(display, index).ok(),
        });
    }

    Ok(gpus)
}

fn extract_fan_index(line: &str) -> Option<u32> {
    let start = line.find("[fan:")?;
    let rest = &line[start + 5..];
    let end = rest.find(']')?;
    rest[..end].parse::<u32>().ok()
}

fn resolve_power_value(value: &str, default_w: &str, min_w: &str, max_w: &str) -> Result<String> {
    let lower = value.trim().to_ascii_lowercase();
    let resolved = match lower.as_str() {
        "min" | "eco" => min_w.to_string(),
        "balanced" => {
            let min = min_w.parse::<f64>()?;
            let max = max_w.parse::<f64>()?;
            format!("{:.0}", (min + max) / 2.0)
        }
        "default" => default_w.to_string(),
        "max" | "performance" => max_w.to_string(),
        _ => {
            let watts = value.parse::<f64>().with_context(
                || "power value must be watts or one of min/eco/balanced/default/max/performance",
            )?;
            let min = min_w.parse::<f64>()?;
            let max = max_w.parse::<f64>()?;
            format!("{:.0}", clamp_watts(watts, min, max))
        }
    };

    Ok(resolved)
}

fn clamp_watts(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

fn format_vram_summary(used_mib: &str, total_mib: &str) -> String {
    let used = used_mib.parse::<f64>().ok();
    let total = total_mib.parse::<f64>().ok();

    match (used, total) {
        (Some(used), Some(total)) if total > 0.0 => {
            format!(
                "{:.1}/{:.1} GiB ({:.0}%)",
                used / 1024.0,
                total / 1024.0,
                (used / total) * 100.0
            )
        }
        _ => format!("{used_mib}/{total_mib} MiB"),
    }
}

fn format_percent(value: &str) -> String {
    if value.eq_ignore_ascii_case("n/a") {
        "N/A".to_string()
    } else {
        format!("{value}%")
    }
}

fn percent_ratio(value: &str) -> f64 {
    value
        .parse::<f64>()
        .map(|percent| (percent / 100.0).clamp(0.0, 1.0))
        .unwrap_or(0.0)
}

fn display_from_env() -> String {
    std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string())
}

fn is_auto_fan_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "default" | "auto"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_power_presets() {
        assert_eq!(
            resolve_power_value("eco", "350", "100", "500").unwrap(),
            "100"
        );
        assert_eq!(
            resolve_power_value("balanced", "350", "100", "500").unwrap(),
            "300"
        );
        assert_eq!(
            resolve_power_value("default", "350", "100", "500").unwrap(),
            "350"
        );
        assert_eq!(
            resolve_power_value("max", "350", "100", "500").unwrap(),
            "500"
        );
    }

    #[test]
    fn clamps_numeric_power_values() {
        assert_eq!(
            resolve_power_value("50", "350", "100", "500").unwrap(),
            "100"
        );
        assert_eq!(
            resolve_power_value("600", "350", "100", "500").unwrap(),
            "500"
        );
    }

    #[test]
    fn extracts_fan_indices() {
        assert_eq!(extract_fan_index("    [0] [fan:0] (Fan 0)"), Some(0));
        assert_eq!(extract_fan_index("    [1] [fan:12] (Fan 12)"), Some(12));
        assert_eq!(extract_fan_index("no fan here"), None);
    }

    #[test]
    fn parses_gpu_rows() {
        let output = "\
0, NVIDIA GeForce RTX 3090, 68, 91, 1890, 62, 54, 18432, 24576, 275.00 W, 350.00 W, 350.00 W, 100.00 W, 500.00 W\n\
1, NVIDIA GeForce RTX 3090, 59, 72, 1710, 48, 43, 12288, 24576, 210.00 W, 350.00 W, 350.00 W, 100.00 W, 500.00 W\n";
        let gpus = parse_gpu_rows(output, ":0").unwrap();
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[1].index, 1);
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 3090");
        assert_eq!(gpus[0].temperature_c, "68");
        assert_eq!(gpus[0].gpu_utilization_pct, "91");
        assert_eq!(gpus[0].graphics_clock_mhz, "1890");
        assert_eq!(gpus[0].fan_speed_pct, "62");
        assert_eq!(gpus[0].memory_used_mib, "18432");
    }

    #[test]
    fn handles_fan_default_values() {
        assert!(is_auto_fan_value("default"));
        assert!(is_auto_fan_value("auto"));
        assert!(!is_auto_fan_value("100"));
    }

    #[test]
    fn formats_vram_summary_in_gib() {
        assert_eq!(format_vram_summary("18432", "24576"), "18.0/24.0 GiB (75%)");
    }
}

