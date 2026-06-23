//! `lp_diff` — interactive terminal viewer for diffing two LP or MPS files.
//!
//! Parses both files, computes a structural diff (variables, constraints,
//! objectives), and launches a TUI for exploring the changes. With `--summary`
//! it prints a structured summary to stdout and exits without the TUI.
//!
//! # Exit codes
//!
//! - `0` — success (including `--summary` mode).
//! - `1` — runtime error: missing input file, invalid tolerance or rename
//!   pattern, parse failure, or a terminal/IO error.
//! - `2` — command-line usage error (reported by clap).

mod app;
mod cli_output;
mod detail_model;
mod detail_text;
mod diff_model;
mod event;
mod export;
mod input;
mod line_index;
mod parse;
mod search;
mod solver;
mod state;
mod theme;
mod ui;
mod watch;
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
use crate::diff_model::{DiffInput, DiffOptions, build_diff_report};
use crate::event::{Event, EventHandler};
use crate::parse::parse_file;

/// Interactive diff viewer for LP and MPS files
#[derive(Parser)]
#[command(name = "lp_diff", version, about)]
struct Cli {
    /// First file (base)
    file1: PathBuf,
    /// Second file (compare against)
    file2: PathBuf,
    /// Print a structured summary to stdout and exit without launching the TUI
    #[arg(long)]
    summary: bool,

    /// Reload and re-diff automatically when either input file changes on disk
    #[arg(long)]
    watch: bool,

    /// Absolute tolerance for numeric comparisons (RHS & coefficients).
    /// Two values compare equal when |a - b| <= abs_tol. A tiny epsilon floor
    /// is always applied so ordinary float noise never registers as a change.
    #[arg(long, default_value_t = 0.0)]
    abs_tol: f64,

    /// Relative tolerance for numeric comparisons, scaled by magnitude.
    /// Two values compare equal when |a - b| <= rel_tol * max(|a|, |b|).
    #[arg(long, default_value_t = 0.0)]
    rel_tol: f64,

    /// Regex rewrite applied to names in BOTH files before matching.
    /// Takes two values: PATTERN REPLACEMENT. May be repeated; rules apply in order.
    /// Example: --rename '\[\d+\]' '[i]' rewrites x[1] and x[2] to x[i],
    /// so index-shifted entries are matched as the same name.
    #[arg(long, num_args = 2, value_names = ["PATTERN", "REPLACEMENT"], action = clap::ArgAction::Append)]
    rename: Vec<String>,

    /// Colour palette: detect from the terminal background, or force light/dark
    #[arg(long, value_enum, default_value_t = ThemeArg::Auto)]
    theme: ThemeArg,
}

/// `--theme` argument values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum ThemeArg {
    /// Detect from `COLORFGBG`, falling back to dark
    Auto,
    Dark,
    Light,
}

impl ThemeArg {
    /// Resolve to a concrete theme mode, consulting the environment for `Auto`.
    ///
    /// `NO_COLOR` (<https://no-color.org>) forces monochrome, but only when the
    /// user left `--theme` at its default: an explicit `--theme dark|light` is
    /// an intentional override and wins.
    fn resolve(self) -> theme::ThemeMode {
        match self {
            Self::Dark => theme::ThemeMode::Dark,
            Self::Light => theme::ThemeMode::Light,
            Self::Auto => {
                if std::env::var_os("NO_COLOR").is_some() {
                    return theme::ThemeMode::Mono;
                }
                std::env::var("COLORFGBG")
                    .ok()
                    .and_then(|value| theme::detect_mode_from_colorfgbg(&value))
                    .unwrap_or(theme::ThemeMode::Dark)
            }
        }
    }
}

/// Compile the `--rename` pairs into a list of `(Regex, replacement)` tuples.
///
/// Fails on an odd number of values or an invalid regex — both are surfaced to the
/// user before any expensive parsing happens.
fn build_rename_rules(raw: &[String]) -> Result<Vec<(regex::Regex, String)>, Box<dyn std::error::Error + Send + Sync>> {
    if !raw.len().is_multiple_of(2) {
        return Err("--rename requires pairs of PATTERN REPLACEMENT".into());
    }
    let mut rules = Vec::with_capacity(raw.len() / 2);
    for chunk in raw.chunks_exact(2) {
        let re = regex::Regex::new(&chunk[0]).map_err(|e| format!("invalid --rename pattern '{}': {e}", chunk[0]))?;
        rules.push((re, chunk[1].clone()));
    }
    Ok(rules)
}

/// Suspend the process (Ctrl+Z) and resume cleanly.
///
/// Leaves raw mode and the alternate screen so the parent shell is intact while
/// stopped, raises `SIGTSTP`, and — once the shell foregrounds us again — re-enters
/// the alternate screen and forces a full redraw. Execution resumes right after the
/// `raise` call, so no separate `SIGCONT` handler is needed.
#[cfg(unix)]
fn suspend<W: io::Write>(terminal: &mut Terminal<CrosstermBackend<W>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture, crossterm::cursor::Show)?;

    // SAFETY: raise() is async-signal-safe and SIGTSTP is a valid signal number;
    // it merely stops this process until a SIGCONT is delivered.
    unsafe {
        libc::raise(libc::SIGTSTP);
    }

    enable_raw_mode()?;
    execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Cli::parse();

    // Fix the palette before any cached lines are built against it.
    theme::init_theme(args.theme.resolve());

    // Validate file existence at the CLI boundary before doing any work.
    if !args.file1.exists() {
        return Err(format!("file not found: '{}'", args.file1.display()).into());
    }
    if !args.file2.exists() {
        return Err(format!("file not found: '{}'", args.file2.display()).into());
    }

    // Validate + compile comparison options before parsing (fails fast on bad regex).
    if !args.abs_tol.is_finite() || args.abs_tol < 0.0 {
        return Err(format!("--abs-tol must be a finite non-negative number, got {}", args.abs_tol).into());
    }
    if !args.rel_tol.is_finite() || args.rel_tol < 0.0 {
        return Err(format!("--rel-tol must be a finite non-negative number, got {}", args.rel_tol).into());
    }
    let rename_rules = build_rename_rules(&args.rename)?;
    let diff_options = DiffOptions { abs_tol: args.abs_tol, rel_tol: args.rel_tol, rename_rules };

    // Parse both files in parallel using scoped threads
    let start = Instant::now();
    eprintln!("Parsing both files in parallel...");
    stderr().flush()?;

    let (result1, result2) = std::thread::scope(|s| {
        let h1 = s.spawn(|| parse_file(&args.file1));
        let h2 = s.spawn(|| parse_file(&args.file2));
        (h1.join(), h2.join())
    });

    let (owned1, analysis1, line_map1, raw_text1) = result1.expect("file1 parse thread panicked")?;
    let (owned2, analysis2, line_map2, raw_text2) = result2.expect("file2 parse thread panicked")?;

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
        // Cloned so the original options stay available for live rebuilds in the TUI.
        options: diff_options.clone(),
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
    // Errors are deliberately ignored here: we are already panicking and must not
    // double-panic, so each restoration step is attempted independently to ensure
    // one failure cannot skip the rest.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
        let _ = execute!(io::stderr(), DisableMouseCapture);
        let _ = execute!(io::stderr(), crossterm::cursor::Show);
        original_hook(panic_info);
    }));

    // Wrap parsed problems in Arc for sharing with solver threads.
    let problem1 = Arc::new(owned1);
    let problem2 = Arc::new(owned2);

    // Create app and event handler
    let raw_text1: Arc<str> = raw_text1.into();
    let raw_text2: Arc<str> = raw_text2.into();
    let mut app = App::new(report, args.file1, args.file2, problem1, problem2, raw_text1, raw_text2, diff_options, line_map1, line_map2);
    if args.watch {
        app.enable_watch();
    }
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
            Event::Key(key) => {
                // Ctrl+Z: in raw mode the terminal does not generate SIGTSTP, so
                // suspend ourselves explicitly, restoring the terminal first.
                #[cfg(unix)]
                if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && key.code == crossterm::event::KeyCode::Char('z') {
                    suspend(&mut terminal)?;
                    continue;
                }
                app.handle_key(key);
            }
            Event::Mouse(mouse) => app.handle_mouse(mouse),
            Event::Resize => {} // ratatui handles resize automatically
            Event::Tick => {
                app.poll_solve();
                app.poll_watch();
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
