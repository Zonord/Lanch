use crossterm::event::{self, KeyCode};
use freedesktop_desktop_entry::{Iter, default_paths, get_languages_from_env};
use ratatui::{
    DefaultTerminal,
    layout::{Constraint, Layout},
    widgets::{Block, List, ListState, Paragraph},
};
use std::os::unix::process::CommandExt; // для .exec()
use std::process::{Command, Stdio};

fn main() -> std::io::Result<()> {
    let mut to_launch: Option<String> = None;
    ratatui::run(|t| run(t, &mut to_launch))?;

    if let Some(exec) = to_launch {
        launch(&exec); // -> !
    }
    Ok(())
}

fn load_apps() -> Vec<(String, String)> {
    let locales = get_languages_from_env();
    Iter::new(default_paths())
        .entries(Some(&locales))
        .filter_map(|entry| {
            if entry.no_display() || entry.hidden() {
                return None;
            }
            if entry.type_() != Some("Application") {
                return None;
            }
            let name = entry.name(&locales)?.to_string();
            let exec = entry.exec()?.to_string();
            Some((name, exec))
        })
        .collect()
}

fn launch(exec: &str) -> ! {
    let parts: Vec<&str> = exec
        .split_whitespace()
        .filter(|p| !p.starts_with('%'))
        .collect();

    let Some((&cmd, args)) = parts.split_first() else {
        std::process::exit(1);
    };

    let err = Command::new(cmd).args(args).exec();

    eprintln!("exec {cmd}: {err}");
    std::process::exit(127);
}

fn run(terminal: &mut DefaultTerminal, to_launch: &mut Option<String>) -> std::io::Result<()> {
    let apps = load_apps();
    let mut input = String::new();
    let mut selected = 0usize;

    loop {
        let needle = input.to_lowercase();
        let filtered: Vec<&(String, String)> = apps
            .iter()
            .filter(|(name, _)| name.to_lowercase().contains(&needle))
            .collect();

        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        terminal.draw(|f| {
            let [top, bottom] =
                Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(f.area());

            f.render_widget(Paragraph::new(input.as_str()).block(Block::bordered()), top);

            let mut state = ListState::default();
            if !filtered.is_empty() {
                state.select(Some(selected));
            }
            f.render_stateful_widget(
                List::new(filtered.iter().map(|(name, _)| name.as_str()))
                    .block(Block::bordered().title("Applications"))
                    .highlight_symbol("> "),
                bottom,
                &mut state,
            );
        })?;

        if let event::Event::Key(k) = event::read()? {
            match k.code {
                KeyCode::Char(c) => {
                    input.push(c);
                    selected = 0;
                }
                KeyCode::Backspace => {
                    input.pop();
                    selected = 0;
                }
                KeyCode::Down if !filtered.is_empty() => selected = (selected + 1) % filtered.len(),
                KeyCode::Up if !filtered.is_empty() => {
                    selected = (selected + filtered.len() - 1) % filtered.len()
                }
                KeyCode::Enter if !filtered.is_empty() => {
                    *to_launch = Some(filtered[selected].1.clone());
                    break;
                }
                KeyCode::Esc => break,
                _ => {}
            }
        }
    }

    Ok(())
}
