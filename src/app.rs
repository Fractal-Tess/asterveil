use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent};

use crate::gpu::*;
use crate::prelude::*;

pub const APP_NAME: &str = "Asterveil";
pub const DEFAULT_FAN_SPEED: u32 = 100;
pub const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Power,
    Fan,
}

impl ActionKind {
    pub fn title(self) -> &'static str {
        match self {
            ActionKind::Power => "Power Limit",
            ActionKind::Fan => "Fan Speed",
        }
    }

    pub fn helper(self) -> &'static str {
        match self {
            ActionKind::Power => "Choose a power profile for the active target scope.",
            ActionKind::Fan => "Choose a fixed fan preset or return control to auto.",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            ActionKind::Power => "p",
            ActionKind::Fan => "f",
        }
    }

    pub fn choices(self) -> &'static [ChoiceItem] {
        match self {
            ActionKind::Power => POWER_CHOICES,
            ActionKind::Fan => FAN_CHOICES,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Overlay {
    Choice { action: ActionKind, cursor: usize },
    Prompt { action: ActionKind, input: String },
}

#[derive(Clone, Copy, Debug)]
pub struct ChoiceItem {
    pub label: &'static str,
    pub value: &'static str,
    pub detail: &'static str,
}

pub const POWER_CHOICES: &[ChoiceItem] = &[
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

pub const FAN_CHOICES: &[ChoiceItem] = &[
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
pub struct App {
    pub display: String,
    pub gpus: Vec<GpuState>,
    pub cursor: usize,
    pub selected: BTreeSet<usize>,
    pub overlay: Option<Overlay>,
    pub message: String,
    pub history: Vec<String>,
    pub last_refresh: Instant,
    pub quit: bool,
}

impl App {
    pub fn new(display: String) -> Self {
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

    pub fn push_message(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.message = message.clone();
        self.history.insert(0, message);
        self.history.truncate(4);
    }

    pub fn refresh(&mut self) -> Result<()> {
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

    pub fn move_cursor_down(&mut self) {
        if self.gpus.is_empty() {
            return;
        }

        self.cursor = (self.cursor + 1) % self.gpus.len();
        self.push_message(format!("Focus {}", self.focus_title()));
    }

    pub fn move_cursor_up(&mut self) {
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

    pub fn toggle_selected(&mut self) {
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

    pub fn select_all(&mut self) {
        self.selected = (0..self.gpus.len()).collect();
        self.push_message(format!("Selected {} GPU(s)", self.selected.len()));
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.push_message("Selection cleared");
    }

    pub fn open_prompt(&mut self, action: ActionKind) {
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

    pub fn close_overlay(&mut self) {
        self.overlay = None;
    }

    pub fn open_custom_prompt(&mut self, action: ActionKind) {
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

    pub fn action_indices(&self) -> Vec<usize> {
        if self.selected.is_empty() {
            vec![self.cursor]
        } else {
            self.selected.iter().copied().collect()
        }
    }

    pub fn focused_gpu(&self) -> Option<&GpuState> {
        self.gpus.get(self.cursor)
    }

    pub fn focus_title(&self) -> String {
        self.focused_gpu()
            .map(|gpu| format!("GPU {} {}", gpu.index, gpu.name))
            .unwrap_or_else(|| "GPU".to_string())
    }

    pub fn target_summary(&self) -> String {
        if self.gpus.is_empty() {
            return "No GPU target".to_string();
        }

        if self.selected.is_empty() {
            self.focus_title()
        } else {
            format!("{} selected GPU(s)", self.selected.len())
        }
    }

    pub fn selection_scope_hint(&self) -> &'static str {
        if self.selected.is_empty() {
            "Actions apply to the focused GPU."
        } else {
            "Actions apply to the selected GPUs."
        }
    }

    pub fn prompt_target_summary(&self) -> String {
        if self.selected.is_empty() {
            self.focus_title()
        } else {
            format!("{} selected GPU(s)", self.selected.len())
        }
    }

    pub fn fan_scope_hint(&self) -> &'static str {
        "Fan speed currently writes every discovered fan target."
    }

    pub fn focus_snapshot(&self) -> String {
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

    pub fn choice_detail(&self, action: ActionKind, choice: ChoiceItem) -> String {
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

    pub fn apply_power_value(&mut self, value: &str) -> Result<()> {
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

    pub fn apply_fan_value(&mut self, value: &str) -> Result<()> {
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

    pub fn apply_default(&mut self) -> Result<()> {
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

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
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
