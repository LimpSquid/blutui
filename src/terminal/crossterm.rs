use clap::{Parser, Subcommand};
use crossterm::event::{
    Event as CrosstermEvent, EventStream as CrosstermEventStream, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures::{FutureExt, StreamExt};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use super::app::*;
use super::ui::{self, *};
use crate::event::EventBus;

impl TryFrom<KeyEvent> for ui::KeyCode {
    type Error = ();

    fn try_from(event: KeyEvent) -> Result<Self, Self::Error> {
        match event.code {
            KeyCode::Char(q) => Ok(ui::KeyCode::Char(q)),
            KeyCode::Esc => Ok(ui::KeyCode::Esc),
            KeyCode::Up => Ok(ui::KeyCode::Up),
            KeyCode::Down => Ok(ui::KeyCode::Down),
            KeyCode::Left => Ok(ui::KeyCode::Left),
            KeyCode::Right => Ok(ui::KeyCode::Right),
            KeyCode::Tab => Ok(ui::KeyCode::Tab),
            KeyCode::Home => Ok(ui::KeyCode::Home),
            KeyCode::End => Ok(ui::KeyCode::End),
            KeyCode::Enter => Ok(ui::KeyCode::Enter),
            KeyCode::Backspace => Ok(ui::KeyCode::Backspace),
            _ => Err(()),
        }
    }
}

impl From<KeyEvent> for ui::KeyModifiers {
    fn from(event: KeyEvent) -> Self {
        let modifiers = event.modifiers;

        Self {
            ctrl: modifiers.contains(KeyModifiers::CONTROL),
            shift: modifiers.contains(KeyModifiers::SHIFT),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    Discover {
        #[arg(long)]
        watch: bool,
    },
}

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

pub async fn run() -> anyhow::Result<()> {
    let _args = Args::parse();

    // Setup event stream as early as possible to queue up emitted events
    let event_bus = EventBus::new();
    let mut event_stream = event_bus.subscribe();

    // Setup application
    app_init_dir_structure()?;
    let _log_guard = app_init_logging(event_bus.clone());
    let mut app = App::new(event_bus).await?;

    // Setup terminal
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen,)?;

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut input_event_stream = CrosstermEventStream::new();

    let result = async {
        while !app.ui.should_quit {
            before_render(&app.state, &mut app.ui);
            terminal.draw(|frame| render(frame, &app.state, &mut app.ui))?;
            after_render(&app.state, &mut app.ui);

            // Wait for input or app event
            tokio::select! {
                biased;
                event = input_event_stream.next().fuse() => {
                    if let Some(e) = event.transpose()? {
                        match e {
                            CrosstermEvent::Key(e) => {
                                if let Ok((e, m)) = e.try_into().map(|k| (k, e.into())) {
                                    tracing::debug!(event = ?e, "handling input event");
                                    ui::user_event(UserEvent::Key(e, m), &app.state, &mut app.ui);
                                }
                            }
                            CrosstermEvent::FocusGained => {
                                tracing::debug!(event = ?e, "handling focus gained event");
                                ui::user_event(UserEvent::FocusGained, &app.state, &mut app.ui);
                            }
                            _ => {}
                        }
                    }
                }
                events = event_stream.recv_all() => {
                    for event in events? {
                        app.handle_app_event(event).await?;
                    }
                },
            }

            for action in std::mem::take(&mut app.ui.pending_actions) {
                app.handle_user_action(action).await?;
            }
        }

        Ok(())
    }
    .await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen,)?;

    result
}
