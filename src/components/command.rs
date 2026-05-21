use std::{collections::HashMap, time::Duration};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use super::{Component, Frame};
use crate::{
    action::Action,
    config::{Config, KeyBindings, RuntimeConfig},
};

pub struct Command {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    runtime_config: RuntimeConfig,
}

impl Command {
    pub fn new(runtime_config: RuntimeConfig) -> Self {
        Self {
            runtime_config,
            command_tx: None,
            config: Config::new().unwrap(),
        }
    }
}

impl Component for Command {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> Result<()> {
        self.config = config;
        Ok(())
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        if let Action::SetActiveCommandIndex(index) = action {
            self.runtime_config.set_active_command_index(index);
        }
        Ok(None)
    }

    fn draw(&mut self, f: &mut Frame<'_>, area: Rect) -> Result<()> {
        let title = if self.runtime_config.command_count() > 1 {
            format!(
                "Command {}/{}",
                self.runtime_config.active_command_index + 1,
                self.runtime_config.command_count()
            )
        } else {
            "Command".to_string()
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(self.config.get_style("border"))
            .title_style(self.config.get_style("title"));
        let paragraph =
            Paragraph::new(self.runtime_config.active_command_display()).block(block);

        f.render_widget(paragraph, area);
        Ok(())
    }
}
