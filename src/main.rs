use crossterm::event::{self, KeyCode};
use freedesktop_desktop_entry::{Iter, default_paths, get_languages_from_env};
use ratatui::style::Color;
use ratatui::{
    DefaultTerminal,
    layout::{Constraint, Layout},
    widgets::{Block, List, ListState, Paragraph},
};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
struct App {
    name: String,
    name_lower: String,
    name_lower_chars: Vec<char>,
    exec: String,
    is_terminal: bool,
}

const FUZZY_SIMILARITY_THRESHOLD: f64 = 0.3;

fn main() -> std::io::Result<()> {
    let mut to_launch: Option<App> = None;
    ratatui::run(|t| run(t, &mut to_launch))?;

    if let Some(app) = to_launch {
        launch(&app);
    }
    Ok(())
}

fn load_apps() -> Vec<App> {
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
            let is_terminal = entry.terminal();
            let name_lower = name.to_lowercase();
            let name_lower_chars = name_lower.chars().collect();
            Some(App {
                name,
                name_lower,
                name_lower_chars,
                exec,
                is_terminal,
            })
        })
        .collect()
}

fn launch(app: &App) {
    let parts: Vec<&str> = app
        .exec
        .split_whitespace()
        .filter(|p| !p.starts_with('%'))
        .collect();

    let Some((&cmd, args)) = parts.split_first() else {
        return;
    };

    if app.is_terminal {
        // Replace the current process with the terminal application.
        // This closes the launcher and runs the app in the current terminal.
        let err = Command::new(cmd).args(args).exec();
        // exec only returns if it failed
        let _ = std::fs::write("/tmp/launch.err", format!("{cmd}: {err}\n"));
    } else {
        let mut command = Command::new(cmd);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        if let Err(e) = command.spawn() {
            let _ = std::fs::write("/tmp/launch.err", format!("{cmd}: {e}\n"));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchTier {
    Fuzzy,
    Subsequence,
    Substring,
    Prefix,
    Exact,
}

fn match_score(name_lower: &str, name_chars: &[char], needle: &str) -> Option<(MatchTier, i64)> {
    if needle.is_empty() || name_lower == needle {
        return Some((MatchTier::Exact, 0));
    }

    if name_lower.starts_with(needle) {
        return Some((MatchTier::Prefix, -(name_lower.len() as i64)));
    }

    if let Some(pos) = name_lower.find(needle) {
        return Some((
            MatchTier::Substring,
            -(pos as i64 * 5 + name_lower.len() as i64),
        ));
    }

    if let Some(score) = fuzzy_subsequence_score(needle, name_chars) {
        return Some((MatchTier::Subsequence, score));
    }

    let max_len = needle.len().max(name_lower.len());
    let min_len = needle.len().min(name_lower.len());
    if max_len > 0
        && (max_len - min_len)
            <= (max_len as f64 * (1.0 - FUZZY_SIMILARITY_THRESHOLD)) as usize
    {
        let dist = levenshtein_distance(needle, name_lower);
        let similarity = 1.0 - (dist as f64 / max_len as f64);
        if similarity > FUZZY_SIMILARITY_THRESHOLD {
            return Some((MatchTier::Fuzzy, (similarity * 1000.0) as i64));
        }
    }
    None
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a = a.chars().collect::<Vec<char>>();
    let b = b.chars().collect::<Vec<char>>();
    let na = a.len();
    let nb = b.len();
    if na == 0 {
        return nb;
    }
    if nb == 0 {
        return na;
    }

    let mut dp = vec![0; nb + 1];
    for j in 0..=nb {
        dp[j] = j;
    }

    for i in 1..=na {
        let mut prev_diag = dp[0];
        dp[0] = i;
        for j in 1..=nb {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            let next_diag = dp[j];
            dp[j] = (dp[j] + 1).min((dp[j - 1] + 1).min(prev_diag + cost));
            prev_diag = next_diag;
        }
    }
    dp[nb]
}

struct FuzzyState {
    h_idx: usize,
    prev_idx: Option<usize>,
    run: i64,
    score: i64,
}

fn fuzzy_subsequence_score(needle: &str, h: &[char]) -> Option<i64> {
    let n_len = needle.chars().count();

    let final_state = needle.chars().try_fold(
        FuzzyState {
            h_idx: 0,
            prev_idx: None,
            run: 0,
            score: 0,
        },
        |state, nc| {
            let idx = (state.h_idx..h.len()).find(|&i| h[i] == nc)?;

            let run = match state.prev_idx {
                Some(prev) if idx == prev + 1 => state.run + 1,
                _ => 0,
            };

            let mut char_score = 10 + if run > 0 { 15 + run * 5 } else { 0 };
            char_score += match idx {
                0 => 20,
                i if i > 0 && matches!(h[i - 1], ' ' | '-' | '_' | '.') => 15,
                _ => 0,
            };

            Some(FuzzyState {
                h_idx: idx + 1,
                prev_idx: Some(idx),
                run,
                score: state.score + char_score,
            })
        },
    )?;

    Some(final_state.score - (h.len() as i64 - n_len as i64).min(50))
}

fn filter_and_sort_apps<'a>(apps: &'a [App], input: &str) -> Vec<&'a App> {
    let needle = input.to_lowercase();

    if needle.is_empty() {
        let mut all: Vec<&App> = apps.iter().collect();
        all.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
        return all;
    }

    let mut scored: Vec<(&App, (MatchTier, i64))> = apps
        .iter()
        .filter_map(|entry| {
            match_score(&entry.name_lower, &entry.name_lower_chars, &needle)
                .map(|s| (entry, s))
        })
        .collect();

    scored.sort_by(|(name_a, score_a), (name_b, score_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| name_a.name.len().cmp(&name_b.name.len()))
            .then_with(|| name_a.name.cmp(&name_b.name))
    });

    scored.into_iter().map(|(entry, _)| entry).collect()
}

fn draw_ui(
    terminal: &mut DefaultTerminal,
    input: &str,
    filtered: &[&App],
    list_state: &mut ListState,
) -> std::io::Result<()> {
    terminal.draw(|f| {
        let area = f.area();
        let [top, bottom] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);

        f.render_widget(
            Paragraph::new(input)
                .block(Block::bordered().title("Search"))
                .style(Color::Cyan),
            top,
        );

        f.render_stateful_widget(
            List::new(filtered.iter().map(|app| app.name.as_str()))
                .style(Color::Cyan)
                .block(Block::bordered().title("Applications"))
                .highlight_symbol("-> "),
            bottom,
            list_state,
        );

        let version_area = ratatui::layout::Rect::new(
            area.width.saturating_sub(12),
            area.height.saturating_sub(1),
            11,
            1,
        );
        f.render_widget(
            Paragraph::new("Version 1.3")
                .alignment(ratatui::layout::Alignment::Right),
            version_area,
        );
    })?;
    Ok(())
}

enum Action {
    Continue,
    Launch(App),
    Quit,
}

fn handle_key(code: KeyCode, input: &mut String, selected: &mut usize, filtered: &[&App]) -> Action {
    match code {
        KeyCode::Char(c) => {
            input.push(c);
            *selected = 0;
        }
        KeyCode::Backspace => {
            input.pop();
            *selected = 0;
        }
        KeyCode::Down if !filtered.is_empty() => *selected = (*selected + 1) % filtered.len(),
        KeyCode::Up if !filtered.is_empty() => {
            *selected = if *selected == 0 {
                filtered.len() - 1
            } else {
                *selected - 1
            };
        }
        KeyCode::Enter if !filtered.is_empty() => {
            return Action::Launch(filtered[*selected].clone());
        }
        KeyCode::Esc => return Action::Quit,
        _ => {}
    }
    Action::Continue
}

fn run(terminal: &mut DefaultTerminal, to_launch: &mut Option<App>) -> std::io::Result<()> {
    let apps = load_apps();
    let mut input = String::new();
    let mut selected = 0usize;
    let mut list_state = ListState::default();
    let mut filtered = filter_and_sort_apps(&apps, &input);

    loop {
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        if !filtered.is_empty() {
            list_state.select(Some(selected));
        } else {
            list_state.select(None);
        }

        draw_ui(terminal, &input, &filtered, &mut list_state)?;

        if let event::Event::Key(k) = event::read()? {
            match handle_key(k.code, &mut input, &mut selected, &filtered) {
                Action::Continue => {
                    filtered = filter_and_sort_apps(&apps, &input);
                }
                Action::Launch(app) => {
                    *to_launch = Some(app);
                    break;
                }
                Action::Quit => break,
            }
        }
    }

    Ok(())
}
