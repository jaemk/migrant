/*!
Interactive terminal UI for viewing and applying migrations
*/
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use migrant_lib::{Config, Direction, MigrationStatus, Migrator};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::{DefaultTerminal, Frame};

pub fn run(config: &Config) -> super::Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Err("`migrant tui` requires an interactive terminal".into());
    }
    let mut app = App::new(config)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        ratatui::restore();
        previous_hook(panic_info);
    }));
    let mut terminal = ratatui::init();
    let res = app.run(&mut terminal);
    ratatui::restore();
    res
}

struct App {
    config: Config,
    statuses: Vec<MigrationStatus>,
    list_state: ListState,
    message: String,
    quit: bool,
}

impl App {
    fn new(config: &Config) -> super::Result<Self> {
        let config = config.reload()?;
        let statuses = migrant_lib::migration_statuses(&config)?;
        let mut list_state = ListState::default();
        if !statuses.is_empty() {
            list_state.select(Some(0));
        }
        Ok(Self {
            config,
            statuses,
            list_state,
            message: String::from("Ready"),
            quit: false,
        })
    }

    fn run(&mut self, terminal: &mut DefaultTerminal) -> super::Result<()> {
        while !self.quit {
            terminal.draw(|frame| self.render(frame))?;
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    self.handle_key(key);
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Char('u') => self.apply(Direction::Up, false),
            KeyCode::Char('d') => self.apply(Direction::Down, false),
            KeyCode::Char('a') => self.apply(Direction::Up, true),
            KeyCode::Char('D') => self.apply(Direction::Down, true),
            KeyCode::Char('r') => self.refresh("Refreshed"),
            _ => {}
        }
    }

    fn select_next(&mut self) {
        if self.statuses.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.statuses.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    fn select_previous(&mut self) {
        if self.statuses.is_empty() {
            return;
        }
        let previous = self
            .list_state
            .selected()
            .map_or(0, |i| i.saturating_sub(1));
        self.list_state.select(Some(previous));
    }

    fn apply(&mut self, direction: Direction, all: bool) {
        let res = Migrator::with_config(&self.config)
            .direction(direction)
            .all(all)
            .show_output(false)
            .apply();
        let message = match res {
            Ok(()) => format!(
                "Applied {}[{}] migration{}",
                if all { "all " } else { "" },
                direction.to_string().to_lowercase(),
                if all { "s" } else { "" }
            ),
            Err(e) if e.is_migration_complete() => match direction {
                Direction::Up => "No un-applied migrations".to_string(),
                Direction::Down => "Nothing to un-apply".to_string(),
            },
            Err(e) => format!("{}", e),
        };
        self.refresh(&message);
    }

    fn refresh(&mut self, message: &str) {
        match self.config.reload() {
            Ok(config) => {
                self.config = config;
                match migrant_lib::migration_statuses(&self.config) {
                    Ok(statuses) => {
                        self.statuses = statuses;
                        self.message = message.to_string();
                    }
                    Err(e) => self.message = format!("{}", e),
                }
            }
            Err(e) => self.message = format!("{}", e),
        }
        // keep the selection in bounds
        match self.list_state.selected() {
            Some(i) if !self.statuses.is_empty() => {
                self.list_state.select(Some(i.min(self.statuses.len() - 1)));
            }
            Some(_) => self.list_state.select(None),
            None if !self.statuses.is_empty() => self.list_state.select(Some(0)),
            None => {}
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let [header_area, list_area, footer_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(4),
        ])
        .areas(frame.area());

        let applied = self.statuses.iter().filter(|m| m.applied).count();
        let header = Paragraph::new(Line::from(vec![
            Span::styled("migrant", Style::new().add_modifier(Modifier::BOLD)),
            Span::raw(format!(
                "  |  {}  |  {}/{} applied",
                self.config.database_type(),
                applied,
                self.statuses.len()
            )),
        ]))
        .block(Block::bordered());
        frame.render_widget(header, header_area);

        let items = self
            .statuses
            .iter()
            .map(|mig| {
                let (mark, style) = if mig.applied {
                    ("✓", Style::new().fg(Color::Green))
                } else {
                    (" ", Style::new().fg(Color::DarkGray))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{}] ", mark), style),
                    Span::raw(mig.tag.clone()),
                ]))
            })
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(Block::bordered().title("Migrations"))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, list_area, &mut self.list_state);

        let footer = Paragraph::new(vec![
            Line::from(Span::styled(
                "u: apply up  d: apply down  a: apply all up  D: apply all down  r: refresh  q: quit",
                Style::new().fg(Color::DarkGray),
            )),
            Line::from(self.message.as_str()),
        ])
        .wrap(Wrap { trim: false })
        .block(Block::bordered());
        frame.render_widget(footer, footer_area);
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use migrant_lib::{EmbeddedMigration, Settings};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_app() -> App {
        let settings = Settings::configure_sqlite().memory().build().unwrap();
        let mut config = Config::with_settings(&settings);
        config
            .use_migrations(&[
                EmbeddedMigration::with_tag("create-users")
                    .up("create table users (id integer primary key);")
                    .down("drop table users;")
                    .boxed(),
                EmbeddedMigration::with_tag("create-posts")
                    .up("create table posts (id integer primary key);")
                    .down("drop table posts;")
                    .boxed(),
            ])
            .unwrap();
        config.setup().unwrap();
        App::new(&config).unwrap()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn applied_count(app: &App) -> usize {
        app.statuses.iter().filter(|m| m.applied).count()
    }

    fn buffer_text(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            text.push('\n');
        }
        text
    }

    // TUI-1
    #[test]
    fn lists_migrations_with_applied_status() {
        let mut app = test_app();
        assert_eq!(2, app.statuses.len());

        let text = buffer_text(&mut app);
        assert!(text.contains("migrant"));
        assert!(text.contains("0/2 applied"));
        assert!(text.contains("[ ] create-users"));
        assert!(text.contains("[ ] create-posts"));

        app.handle_key(key(KeyCode::Char('u')));
        let text = buffer_text(&mut app);
        assert!(text.contains("1/2 applied"));
        assert!(text.contains("[✓] create-users"));
        assert!(text.contains("[ ] create-posts"));
    }

    // TUI-2
    #[test]
    fn navigation_moves_selection() {
        let mut app = test_app();
        assert_eq!(Some(0), app.list_state.selected());

        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(Some(1), app.list_state.selected());
        // clamped at the last entry
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(Some(1), app.list_state.selected());

        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(Some(0), app.list_state.selected());
        // clamped at the first entry
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(Some(0), app.list_state.selected());

        app.handle_key(key(KeyCode::Down));
        assert_eq!(Some(1), app.list_state.selected());
        app.handle_key(key(KeyCode::Up));
        assert_eq!(Some(0), app.list_state.selected());
    }

    // TUI-3
    #[test]
    fn apply_keys_run_migrations() {
        let mut app = test_app();
        assert_eq!(0, applied_count(&app));

        // `d` with nothing applied reports there's nothing to do
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!("Nothing to un-apply", app.message);

        // `u` applies one up migration at a time
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(1, applied_count(&app));
        assert_eq!("Applied [up] migration", app.message);
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(2, applied_count(&app));

        // `u` with everything applied reports completion
        app.handle_key(key(KeyCode::Char('u')));
        assert_eq!(2, applied_count(&app));
        assert_eq!("No un-applied migrations", app.message);

        // `D` un-applies everything
        app.handle_key(key(KeyCode::Char('D')));
        assert_eq!(0, applied_count(&app));
        assert_eq!("Applied all [down] migrations", app.message);

        // `a` applies everything
        app.handle_key(key(KeyCode::Char('a')));
        assert_eq!(2, applied_count(&app));
        assert_eq!("Applied all [up] migrations", app.message);

        // `d` un-applies one down migration
        app.handle_key(key(KeyCode::Char('d')));
        assert_eq!(1, applied_count(&app));
        assert_eq!("Applied [down] migration", app.message);
    }

    // TUI-4
    #[test]
    fn refresh_picks_up_external_changes() {
        let mut app = test_app();
        assert_eq!(0, applied_count(&app));

        // apply a migration outside the app; config clones share the
        // same in-memory database
        Migrator::with_config(&app.config)
            .show_output(false)
            .apply()
            .unwrap();
        assert_eq!(0, applied_count(&app));

        app.handle_key(key(KeyCode::Char('r')));
        assert_eq!(1, applied_count(&app));
        assert_eq!("Refreshed", app.message);
    }

    // TUI-4
    #[test]
    fn quit_keys() {
        for quit_key in [
            key(KeyCode::Char('q')),
            key(KeyCode::Esc),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            let mut app = test_app();
            assert!(!app.quit);
            app.handle_key(quit_key);
            assert!(app.quit);
        }
    }
}
