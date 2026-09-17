use crossterm::event::{self, KeyCode};
use freedesktop_desktop_entry::{Iter, default_paths, get_languages_from_env};
use ratatui::style::Color;
use ratatui::{
    DefaultTerminal,
    layout::{Constraint, Layout},
    widgets::{Block, List, ListState, Paragraph},
};
use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

type App = (String, String);

const FUZZY_SIMILARITY_THRESHOLD: f64 = 0.3;

fn main() -> std::io::Result<()> {
    let mut to_launch: Option<String> = None;
    ratatui::run(|t| run(t, &mut to_launch))?;

    if let Some(exec) = to_launch {
        launch(&exec);
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
            Some((name, exec))
        })
        .collect()
}

fn launch(exec: &str) {
    let parts: Vec<&str> = exec
        .split_whitespace()
        .filter(|p| !p.starts_with('%'))
        .collect();

    let Some((&cmd, args)) = parts.split_first() else {
        return;
    };

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchTier {
    Fuzzy,
    Subsequence,
    Substring,
    Prefix,
    Exact,
}

/// Скор совпадения: сначала сравнивается MatchTier, при равенстве — i64
/// (чем больше, тем релевантнее). `None` — приложение не подходит.
fn match_score(name_lower: &str, needle: &str) -> Option<(MatchTier, i64)> {
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

    if let Some(score) = fuzzy_subsequence_score(needle, name_lower) {
        return Some((MatchTier::Subsequence, score));
    }

    let similarity = char_multiset_similarity(needle, name_lower);
    (similarity > FUZZY_SIMILARITY_THRESHOLD)
        .then(|| (MatchTier::Fuzzy, (similarity * 1000.0) as i64))
}

fn char_freq(s: &str) -> HashMap<char, i32> {
    s.chars().fold(HashMap::new(), |mut acc, c| {
        *acc.entry(c).or_insert(0) += 1;
        acc
    })
}

fn char_multiset_similarity(a: &str, b: &str) -> f64 {
    let freq_a = char_freq(a);
    let freq_b = char_freq(b);

    let common: i32 = freq_a
        .iter()
        .map(|(c, &count_a)| freq_b.get(c).map_or(0, |&count_b| count_a.min(count_b)))
        .sum();

    let total = a.chars().count() + b.chars().count();
    if total == 0 {
        0.0
    } else {
        2.0 * common as f64 / total as f64
    }
}

struct FuzzyState {
    h_idx: usize,
    prev_idx: Option<usize>,
    run: i64,
    score: i64,
}

fn fuzzy_subsequence_score(needle: &str, haystack: &str) -> Option<i64> {
    let h: Vec<char> = haystack.chars().collect();
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
                i if matches!(h[i - 1], ' ' | '-' | '_' | '.') => 15,
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

/// Фильтрует и сортирует приложения по релевантности запросу.
/// Пустой запрос — алфавитный порядок без пересчёта скора.
fn filter_and_sort_apps<'a>(apps: &'a [App], input: &str) -> Vec<&'a App> {
    let needle = input.to_lowercase();

    if needle.is_empty() {
        let mut all: Vec<&App> = apps.iter().collect();
        all.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
        return all;
    }

    let mut scored: Vec<(&App, (MatchTier, i64))> = apps
        .iter()
        .filter_map(|entry| match_score(&entry.0.to_lowercase(), &needle).map(|s| (entry, s)))
        .collect();

    scored.sort_by(|(name_a, score_a), (name_b, score_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| name_a.0.len().cmp(&name_b.0.len()))
            .then_with(|| name_a.0.cmp(&name_b.0))
    });

    scored.into_iter().map(|(entry, _)| entry).collect()
}

fn draw_ui(
    terminal: &mut DefaultTerminal,
    input: &str,
    filtered: &[&App],
    selected: usize,
) -> std::io::Result<()> {
    terminal.draw(|f| {
        let [top, bottom] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(f.area());

        f.render_widget(
            Paragraph::new(input)
                .block(Block::bordered().title("Search"))
                .style(Color::Cyan),
            top,
        );

        let mut state = ListState::default();
        if !filtered.is_empty() {
            state.select(Some(selected));
        }
        f.render_stateful_widget(
            List::new(filtered.iter().map(|(name, _)| name.as_str()))
                .style(Color::Cyan)
                .block(Block::bordered().title("Applications"))
                .highlight_symbol("-> "),
            bottom,
            &mut state,
        );
    })?;
    Ok(())
}

enum Action {
    Continue,
    Launch(String),
    Quit,
}

fn handle_key(
    code: KeyCode,
    input: &mut String,
    selected: &mut usize,
    filtered: &[&App],
) -> Action {
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
            *selected = (*selected + filtered.len() - 1) % filtered.len();
        }
        KeyCode::Enter if !filtered.is_empty() => {
            return Action::Launch(filtered[*selected].1.clone());
        }
        KeyCode::Esc => return Action::Quit,
        _ => {}
    }
    Action::Continue
}

fn run(terminal: &mut DefaultTerminal, to_launch: &mut Option<String>) -> std::io::Result<()> {
    let apps = load_apps();
    let mut input = String::new();
    let mut selected = 0usize;

    loop {
        let filtered = filter_and_sort_apps(&apps, &input);
        if selected >= filtered.len() {
            selected = filtered.len().saturating_sub(1);
        }

        draw_ui(terminal, &input, &filtered, selected)?;

        if let event::Event::Key(k) = event::read()? {
            match handle_key(k.code, &mut input, &mut selected, &filtered) {
                Action::Continue => {}
                Action::Launch(exec) => {
                    *to_launch = Some(exec);
                    break;
                }
                Action::Quit => break,
            }
        }
    }

    Ok(())
}
