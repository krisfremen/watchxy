use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;
use tui_input::{backend::crossterm::EventHandler, Input};

use super::{Component, Frame};
use crate::{
    action::Action,
    config::{Config, RuntimeConfig},
    tab_picker::{
        adjust_scroll_offset, command_title, filter_command_indices, match_ranges,
        parse_filter_query,
    },
};

pub struct TabPicker {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    runtime_config: RuntimeConfig,
    input: Input,
    is_active: bool,
    selection: usize,
    scroll_offset: usize,
    viewport_rows: usize,
}

impl TabPicker {
    pub fn new(runtime_config: RuntimeConfig) -> Self {
        Self {
            command_tx: None,
            config: Config::new().unwrap(),
            runtime_config,
            input: Input::default(),
            is_active: false,
            selection: 0,
            scroll_offset: 0,
            viewport_rows: 1,
        }
    }

    fn ensure_selection_visible(&mut self) {
        let total = self.filtered_indices().len();
        self.scroll_offset = adjust_scroll_offset(
            self.selection,
            self.scroll_offset,
            self.viewport_rows,
            total,
        );
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filter_command_indices(&self.runtime_config, self.input.value())
    }

    fn clamp_selection(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            self.selection = 0;
        } else if self.selection >= count {
            self.selection = count - 1;
        }
    }

    fn filtered_selection_index(&self) -> Option<usize> {
        self.filtered_indices().get(self.selection).copied()
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        let next = self.selection as isize + delta;
        let wrapped = (next.rem_euclid(count as isize)) as usize;
        self.selection = wrapped;
        self.ensure_selection_visible();
    }

    fn set_query(&mut self, query: String) -> Result<()> {
        self.input = Input::from(query);
        self.selection = 0;
        self.scroll_offset = 0;
        self.clamp_selection();
        self.ensure_selection_visible();
        if let Some(tx) = &self.command_tx {
            tx.send(Action::SetTabPickerQuery(self.input.value().to_string()))?;
        }
        Ok(())
    }

    fn enter_mode(&mut self) -> Result<()> {
        self.is_active = true;
        self.input = Input::default();
        self.scroll_offset = 0;
        if let Some(tx) = &self.command_tx {
            tx.send(Action::SetTabPickerQuery(String::new()))?;
        }
        let filtered = self.filtered_indices();
        self.selection = filtered
            .iter()
            .position(|&i| i == self.runtime_config.active_command_index)
            .unwrap_or(0);
        self.ensure_selection_visible();
        Ok(())
    }

    fn exit_mode(&mut self) -> Result<()> {
        self.is_active = false;
        self.input = Input::default();
        self.selection = 0;
        self.scroll_offset = 0;
        self.set_query(String::new())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Backspace => {
                self.input
                    .handle_event(&crossterm::event::Event::Key(key_event));
                self.clamp_selection();
                self.set_query(self.input.value().to_string())?;
            }
            KeyCode::Char('u')
                if key_event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.set_query(String::new())?;
            }
            _ => {
                self.input
                    .handle_event(&crossterm::event::Event::Key(key_event));
                self.clamp_selection();
                self.set_query(self.input.value().to_string())?;
            }
        }
        Ok(())
    }

    fn title_spans(&self, title: &str, query: &str, selected: bool) -> Vec<Span<'static>> {
        let parsed = parse_filter_query(query);
        let highlight = self.config.get_style("search_highlight");
        let normal = if selected {
            self.config.get_style("timemachine_selector")
        } else {
            self.config.get_style("secondary_text")
        };

        if parsed.needle.is_empty() {
            return vec![Span::styled(title.to_string(), normal)];
        }

        let ranges = match_ranges(title, &parsed);
        if ranges.is_empty() {
            return vec![Span::styled(title.to_string(), normal)];
        }

        let mut spans = Vec::new();
        let mut start = 0usize;
        for (match_start, match_end) in ranges {
            if match_start > start {
                spans.push(Span::styled(title[start..match_start].to_string(), normal));
            }
            spans.push(Span::styled(
                title[match_start..match_end].to_string(),
                highlight,
            ));
            start = match_end;
        }
        if start < title.len() {
            spans.push(Span::styled(title[start..].to_string(), normal));
        }
        spans
    }

    pub fn draw_prompt(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }
        f.set_cursor(area.x + self.input.visual_cursor() as u16 + 3, area.y);
        let paragraph = Paragraph::new(format!("T> {}", self.input.value()));
        f.render_widget(paragraph, area);
        Ok(())
    }
}

impl Component for TabPicker {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::EnterTabPickerMode => self.enter_mode()?,
            Action::ExitTabPickerMode => self.exit_mode()?,
            Action::KeyEventForTabPicker(key_event) => self.handle_key_event(key_event)?,
            Action::TabPickerMoveUp => self.move_selection(-1),
            Action::TabPickerMoveDown => self.move_selection(1),
            Action::ConfirmTabPicker => {
                if let Some(index) = self.filtered_selection_index() {
                    if let Some(tx) = &self.command_tx {
                        tx.send(Action::ActivateCommandIndex(index))?;
                        tx.send(Action::ExitTabPickerMode)?;
                    }
                }
            }
            Action::SetActiveCommandIndex(index) => {
                self.runtime_config.set_active_command_index(index);
            }
            Action::SetTabPickerQuery(_) => {}
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }

        let filtered = self.filtered_indices();
        let query = self.input.value().to_string();

        // Borders add 2 rows; show every match, capped only by the content pane height.
        const BORDER_ROWS: u16 = 2;
        let max_rows = area.height.saturating_sub(BORDER_ROWS) as usize;
        let visible_rows = filtered.len().max(1).min(max_rows);
        self.viewport_rows = visible_rows;
        self.ensure_selection_visible();

        let popup_height = visible_rows as u16 + BORDER_ROWS;
        let popup_width = area.width.saturating_sub(8).max(24);
        let popup_area = Rect {
            x: area.x + (area.width.saturating_sub(popup_width)) / 2,
            y: area.y + (area.height.saturating_sub(popup_height)) / 2,
            width: popup_width,
            height: popup_height,
        };

        let mut lines: Vec<Line> = Vec::new();
        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "No matching commands",
                self.config.get_style("secondary_text"),
            )));
        } else {
            let end = (self.scroll_offset + visible_rows).min(filtered.len());
            for (row, &command_index) in filtered[self.scroll_offset..end].iter().enumerate() {
                let list_index = self.scroll_offset + row;
                let title = command_title(&self.runtime_config.commands[command_index]);
                let marker = if command_index == self.runtime_config.active_command_index {
                    "●"
                } else {
                    " "
                };
                let prefix = format!("{marker} ");
                let mut spans = vec![Span::styled(
                    prefix,
                    self.config.get_style("secondary_text"),
                )];
                spans.extend(self.title_spans(&title, &query, list_index == self.selection));
                lines.push(Line::from(spans));
            }
        }

        let title = if filtered.len() > visible_rows {
            format!(
                " Switch command {}-{} of {} ",
                self.scroll_offset + 1,
                (self.scroll_offset + visible_rows).min(filtered.len()),
                filtered.len()
            )
        } else {
            " Switch command ".to_string()
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(self.config.get_style("border"));
        f.render_widget(Clear, popup_area);
        f.render_widget(
            Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
            popup_area,
        );

        Ok(())
    }
}
