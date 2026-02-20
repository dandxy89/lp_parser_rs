mod app;
mod diff_model;
mod event;
mod search;
mod ui;
mod widgets;

use std::collections::HashMap;
use std::io::{self, Write as _, stderr};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::{LpProblem, LpProblemOwned};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;

use crate::app::App;
use crate::diff_model::{LineIndex, build_diff_report};
use crate::event::{Event, EventHandler};

/// Interactive diff viewer for LP files
#[derive(Parser)]
#[command(name = "lp_diff", version, about)]
struct Cli {
    /// First LP file (base)
    file1: PathBuf,
    /// Second LP file (compare against)
    file2: PathBuf,
}

/// Build a map from constraint name to 1-based line number using byte offsets
/// captured during parsing and a `LineIndex` built from the source text.
fn build_constraint_line_map(problem: &LpProblem, line_index: &LineIndex) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for constraint in problem.constraints.values() {
        if let Some(offset) = constraint.byte_offset()
            && let Some(line) = line_index.line_number(offset)
        {
            map.insert(constraint.name_ref().to_owned(), line);
        }
    }
    map
}

/// Parse an LP file, returning the owned problem and a constraint→line-number map.
fn parse_lp_file(path: &Path) -> Result<(LpProblemOwned, HashMap<String, usize>), Box<dyn std::error::Error>> {
    let content = parse_file(path).map_err(|e| format!("failed to read '{}': {e}", path.display()))?;
    let problem = LpProblem::parse(&content).map_err(|e| format!("failed to parse '{}': {e}", path.display()))?;
    let line_index = LineIndex::new(&content);
    let line_map = build_constraint_line_map(&problem, &line_index);
    let owned = problem.to_owned();
    Ok((owned, line_map))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    // Parse files with progress output to stderr
    let start = Instant::now();
    eprint!("Parsing {}... ", args.file1.display());
    stderr().flush()?;
    let (owned1, line_map1) = parse_lp_file(&args.file1)?;
    eprintln!(
        "done ({:.1}s, {} variables, {} constraints)",
        start.elapsed().as_secs_f64(),
        owned1.variable_count(),
        owned1.constraint_count(),
    );

    let start2 = Instant::now();
    eprint!("Parsing {}... ", args.file2.display());
    stderr().flush()?;
    let (owned2, line_map2) = parse_lp_file(&args.file2)?;
    eprintln!(
        "done ({:.1}s, {} variables, {} constraints)",
        start2.elapsed().as_secs_f64(),
        owned2.variable_count(),
        owned2.constraint_count(),
    );

    let diff_start = Instant::now();
    eprint!("Computing diff... ");
    stderr().flush()?;
    let file1_str = args.file1.display().to_string();
    let file2_str = args.file2.display().to_string();
    let report = build_diff_report(&file1_str, &file2_str, &owned1, &owned2, &line_map1, &line_map2);
    eprintln!("done ({:.1}s, {} changes found)", diff_start.elapsed().as_secs_f64(), report.summary().total_changes(),);

    // Free parsed problems — only the (small) diff report is needed for the TUI
    drop(owned1);
    drop(owned2);

    eprintln!("Launching viewer...");

    // Set up terminal — enable_raw_mode() must happen before EventHandler::new()
    // because the event thread immediately starts polling for key events.
    enable_raw_mode()?;
    let mut stderr_handle = stderr();
    execute!(stderr_handle, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stderr_handle);
    let mut terminal = Terminal::new(backend)?;

    // Set up panic hook to restore terminal before printing the panic message.
    // Best-effort: we log failures to stderr but cannot propagate from a panic hook.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        if let Err(e) = disable_raw_mode() {
            eprintln!("warning: failed to disable raw mode during panic: {e}");
        }
        if let Err(e) = execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture) {
            eprintln!("warning: failed to leave alternate screen during panic: {e}");
        }
        original_hook(panic_info);
    }));

    // Create app and event handler
    let mut app = App::new(report);
    let events = EventHandler::new(std::time::Duration::from_millis(50));

    // Main loop — draw then process the next event
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        match events.next()? {
            Event::Key(key) => app.handle_key(key),
            Event::Resize(_, _) => {} // ratatui handles resize automatically
            Event::Tick => {}
            Event::Error(e) => return Err(e.into()),
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
