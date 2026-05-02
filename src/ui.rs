use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Wrap},
};

use crate::app::*;
use crate::gpu::*;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
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
