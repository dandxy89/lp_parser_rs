mod app;
mod detail_text;
mod diff_model;
mod event;
mod input;
mod line_index;
mod parse;
mod search;
mod state;
mod ui;
mod widgets;

use std::io::{self, Write as _, stderr};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;

use crate::app::App;
use crate::diff_model::{DiffInput, build_diff_report};
use crate::event::{Event, EventHandler};
use crate::parse::parse_lp_file;

/// Interactive diff viewer for LP files
#[derive(Parser)]
#[command(name = "lp_diff", version, about)]
struct Cli {
    /// First LP file (base)
    file1: PathBuf,
    /// Second LP file (compare against)
    file2: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();

    // Validate file existence at the CLI boundary before doing any work.
    if !args.file1.exists() {
        return Err(format!("file not found: '{}'", args.file1.display()).into());
    }
    if !args.file2.exists() {
        return Err(format!("file not found: '{}'", args.file2.display()).into());
    }

    // Parse files with progress output to stderr
    let start = Instant::now();
    eprint!("Parsing {}... ", args.file1.display());
    stderr().flush()?;
    let (owned1, analysis1, line_map1) = parse_lp_file(&args.file1)?;
    eprintln!(
        "done ({:.1}s, {} variables, {} constraints)",
        start.elapsed().as_secs_f64(),
        owned1.variable_count(),
        owned1.constraint_count(),
    );

    let start2 = Instant::now();
    eprint!("Parsing {}... ", args.file2.display());
    stderr().flush()?;
    let (owned2, analysis2, line_map2) = parse_lp_file(&args.file2)?;
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
    let report = build_diff_report(&DiffInput {
        file1: &file1_str,
        file2: &file2_str,
        p1: &owned1,
        p2: &owned2,
        line_map1: &line_map1,
        line_map2: &line_map2,
        analysis1,
        analysis2,
    });
    eprintln!("done ({:.1}s, {} changes found)", diff_start.elapsed().as_secs_f64(), report.summary().total_changes(),);

    // Free parsed problems — only the (small) diff report is needed for the TUI
    drop(owned1);
    drop(owned2);

    eprintln!("Launching viewer...");

    // Set up terminal — enable_raw_mode() must happen before EventHandler::new()
    // because the event thread immediately starts polling for key events.
    enable_raw_mode()?;
    let mut stderr_handle = stderr();
    execute!(stderr_handle, EnterAlternateScreen, EnableMouseCapture)?;
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
    let events = EventHandler::new(Duration::from_millis(50));

    // Main loop — draw then process the next event
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // Clear yank flash after 1.5 seconds.
        if let Some(flash_time) = app.yank_flash
            && flash_time.elapsed() >= Duration::from_millis(1500)
        {
            app.yank_flash = None;
            app.yank_message.clear();
        }

        match events.next()? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(mouse) => app.handle_mouse(mouse),
            Event::Resize(_, _) | Event::Tick => {} // ratatui handles resize automatically
            Event::Error(e) => return Err(e.into()),
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
