mod app;
mod cli_output;
mod detail_model;
mod detail_text;
mod diff_model;
mod event;
mod input;
mod line_index;
mod parse;
mod search;
mod solver;
mod state;
mod theme;
mod ui;
mod widgets;

use std::io::{self, Write as _, stderr};
use std::path::PathBuf;
use std::sync::Arc;
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
    /// Print a structured summary to stdout and exit without launching the TUI
    #[arg(long)]
    summary: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Cli::parse();

    // Validate file existence at the CLI boundary before doing any work.
    if !args.file1.exists() {
        return Err(format!("file not found: '{}'", args.file1.display()).into());
    }
    if !args.file2.exists() {
        return Err(format!("file not found: '{}'", args.file2.display()).into());
    }

    // Parse both files in parallel using scoped threads
    let start = Instant::now();
    eprintln!("Parsing both files in parallel...");
    stderr().flush()?;

    let (result1, result2) = std::thread::scope(|s| {
        let h1 = s.spawn(|| parse_lp_file(&args.file1));
        let h2 = s.spawn(|| parse_lp_file(&args.file2));
        (h1.join(), h2.join())
    });

    let (owned1, analysis1, line_map1) = result1.expect("file1 parse thread panicked")?;
    let (owned2, analysis2, line_map2) = result2.expect("file2 parse thread panicked")?;

    eprintln!(
        "Parsed {} ({} vars, {} cons) and {} ({} vars, {} cons) in {:.1}s",
        args.file1.display(),
        owned1.variable_count(),
        owned1.constraint_count(),
        args.file2.display(),
        owned2.variable_count(),
        owned2.constraint_count(),
        start.elapsed().as_secs_f64(),
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

    // Non-interactive summary mode: print and exit.
    if args.summary {
        cli_output::print_summary(&report);
        return Ok(());
    }

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

    // Wrap parsed problems in Arc for sharing with solver threads.
    let problem1 = Arc::new(owned1);
    let problem2 = Arc::new(owned2);

    // Create app and event handler
    let mut app = App::new(report, args.file1, args.file2, problem1, problem2);
    let events = EventHandler::new(Duration::from_millis(50));

    // Main loop — draw then process the next event
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // Clear yank flash after 1.5 seconds.
        if let Some(flash_time) = app.yank.flash
            && flash_time.elapsed() >= Duration::from_millis(1500)
        {
            app.yank.flash = None;
            app.yank.message.clear();
        }

        match events.next()? {
            Event::Key(key) => app.handle_key(key),
            Event::Mouse(mouse) => app.handle_mouse(mouse),
            Event::Resize => {} // ratatui handles resize automatically
            Event::Tick => {
                app.poll_solve();
            }
            Event::Error(e) => return Err(e.into()),
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
