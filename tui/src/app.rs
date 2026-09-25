use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use lp_parser_rs::interner::NameId;
use lp_parser_rs::problem::LpProblem;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::ListState;

use crate::detail_model::{CoefficientRow, build_coeff_rows};
use crate::diff_model::{DiffEntry, DiffInput, DiffKind, DiffOptions, DiffSummary, LpDiffReport, build_diff_report, next_tolerance_preset};
use crate::parse::ParsedFile;
use crate::search::{self, CompiledSearch, SearchMode};
use crate::solver::{InfeasibilityDiagnosis, SolveResult};
use crate::state::{
    AnalysisState, DetailView, DiagnosisState, JumpEntry, JumpList, PendingYank, ScrollPane, Side, SolveState, SolveViewState, SortMode,
    WhatIfPrompt,
};
pub use crate::state::{AppMode, DiffFilter, Focus, SearchResult, Section, SectionViewState};
use crate::watch::{WatchSession, WatchState};

/// State for the `Ctrl+P` command palette overlay.
pub struct CommandPaletteState {
    /// Whether the palette overlay is visible.
    pub visible: bool,
    /// Current fuzzy-filter query (readline-style editable input).
    pub query: tui_input::Input,
    /// Palette command indices (see [`PaletteCommand::at`](crate::state::PaletteCommand::at))
    /// matching the query, in rank order.
    pub filtered: Vec<usize>,
    /// Currently highlighted row within `filtered`.
    pub selected: usize,
}

/// State for the telescope-style search pop-up overlay.
pub struct SearchPopupState {
    /// Whether the search pop-up overlay is visible.
    pub visible: bool,
    /// Current query text in the search pop-up input (readline-style editable).
    pub query: tui_input::Input,
    /// Ranked search results spanning all sections.
    pub results: Vec<SearchResult>,
    /// Currently highlighted result index in the pop-up.
    pub selected: usize,
    /// Scroll offset for the detail preview pane inside the pop-up.
    pub scroll: u16,
    /// Pre-built styled lines for each search result, avoiding per-frame
    /// `format!` allocations. Rebuilt in `recompute_search_popup`.
    pub cached_result_lines: Vec<Line<'static>>,
    /// Compact regex compilation error for the current query, if any.
    /// Shown under the query input so an invalid pattern is not mistaken
    /// for a query that simply matches nothing.
    pub regex_error: Option<String>,
}

/// Layout rectangles and dimensions stored during draw for mouse hit-testing and scrolling.
pub struct LayoutRects {
    pub section_selector: Rect,
    pub name_list: Rect,
    pub detail: Rect,
    pub name_list_height: u16,
    pub detail_height: u16,
    pub detail_content_lines: usize,
    /// Per-tab `(start_x, end_x)` column ranges in the tab bar, exclusive end.
    /// Updated each frame by the tab bar renderer for mouse hit-testing.
    pub tab_bounds: [(u16, u16); 5],
}

/// How a status-bar flash is coloured, and how long it stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlashLevel {
    /// Neutral feedback: a mode changed, a match was reached.
    #[default]
    Info,
    /// An action succeeded: a yank, a file written.
    Ok,
    /// The action was refused or did nothing, but nothing failed.
    Warn,
    /// An action failed. Stays until the next key press rather than expiring,
    /// so it is not missed.
    Err,
}

/// Status-bar flash state (it began as the yank confirmation, hence the name).
pub struct YankState {
    /// When the flash was raised; `None` when no flash is showing.
    pub flash: Option<Instant>,
    /// Message displayed in the status bar while the flash shows.
    pub message: String,
    /// Severity: picks the colour, and whether the flash expires on its own.
    pub level: FlashLevel,
}

impl YankState {
    /// Whether the flash clears itself on a timer (everything but errors).
    pub(crate) const fn expires(&self) -> bool {
        self.flash.is_some() && !matches!(self.level, FlashLevel::Err)
    }

    /// Drop the flash.
    pub(crate) fn clear(&mut self) {
        self.flash = None;
        self.message.clear();
    }
}

/// A single entry in the pre-built flat search haystack.
///
/// Names are stored separately in `search_name_buffer` at the same index
/// to avoid duplicating every entry name as an owned `String`.
pub struct HaystackEntry {
    pub section: Section,
    pub index: usize,
    pub kind: DiffKind,
}

/// A pre-formatted diff row line with its `changed` flag for filtering.
pub struct CachedDiffRow {
    pub line: Line<'static>,
    pub changed: bool,
}

/// Cached formatted lines for the solve overlay, avoiding per-frame `format!` allocations.
///
/// Built once when transitioning to `Done`/`DoneBoth` state and invalidated on state change.
pub enum SolveRenderCache {
    /// No cache available.
    Empty,
    /// Single-solve result: pre-formatted tab lines `[summary, variables, constraints, log, duals]`.
    Single([Vec<Line<'static>>; 5]),
    /// Diff-solve result: pre-formatted summary, log, duals, and per-row lines.
    Diff {
        summary: Vec<Line<'static>>,
        log: Vec<Line<'static>>,
        duals: Vec<Line<'static>>,
        variable_rows: Vec<CachedDiffRow>,
        constraint_rows: Vec<CachedDiffRow>,
        /// Pre-formatted variable counts summary label.
        variable_count_label: String,
        /// Pre-formatted constraint counts summary label.
        constraint_count_label: String,
    },
}

/// A completed solve retained after its overlay was closed, so reopening it
/// does not re-run the solver.
pub struct CachedSolve {
    /// Labels of the solved side(s) — identifies which input produced this.
    pub key: String,
    /// Always [`SolveState::Done`] or [`SolveState::DoneBoth`].
    pub state: SolveState,
    /// Problem behind a single-solve result, for the infeasibility diagnosis.
    pub solved_problem: Option<Arc<LpProblem>>,
    /// Modified problem behind side 2 of a comparison solve — a what-if RHS
    /// edit or a presolve rewrite.
    pub what_if_problem: Option<Arc<LpProblem>>,
    /// The overlay's view when it was closed. A comparison's rows were diffed
    /// at this view's threshold, so the two must be restored together or the
    /// threshold label would describe a different diff.
    pub view: SolveViewState,
}

/// Cache capacity: file 1, file 2, both, and one what-if edit.
const SOLVE_CACHE_CAPACITY: usize = 4;

/// Bundles solver-related state: lifecycle, view, and result channel.
pub struct SolverSession {
    /// `HiGHS` solver state machine.
    pub state: SolveState,
    /// Scroll state for the solve results panel.
    pub view: SolveViewState,
    /// Channel for receiving solve results from the background thread.
    pub receive: Option<mpsc::Receiver<Result<SolveResult, String>>>,
    /// Second channel for the "both" solve mode.
    pub receive2: Option<mpsc::Receiver<Result<SolveResult, String>>>,
    /// Cached formatted lines for the solve overlay.
    pub render_cache: SolveRenderCache,
    /// Infeasibility diagnosis state for the current result (key `e`).
    pub diagnosis: DiagnosisState,
    /// Channel for receiving the elastic-relaxation diagnosis from its background thread.
    pub receive_diagnosis: Option<mpsc::Receiver<Result<InfeasibilityDiagnosis, String>>>,
    /// The problem behind the current single-solve result, kept so the
    /// diagnosis can rebuild the model without re-parsing.
    pub solved_problem: Option<Arc<LpProblem>>,
    /// The modified problem behind side 2 of a comparison solve — a what-if
    /// RHS edit or a presolve rewrite — kept so the diagnosis targets the
    /// modified model rather than `problem2`.
    pub what_if_problem: Option<Arc<LpProblem>>,
    /// Labels of the solve currently running or displayed — the cache key the
    /// result is filed under when its overlay is closed.
    pub key: String,
    /// Completed solves kept across overlay closes, newest last. Never
    /// invalidated piecemeal: an input change reloads the files, which
    /// replaces the whole session (see `apply_reload`), and any edit that does
    /// not reload (a what-if RHS) produces a different key.
    ///
    /// ponytail: linear scan over at most `SOLVE_CACHE_CAPACITY` entries; make
    /// it a map if the number of distinct solve targets ever grows.
    pub cache: Vec<CachedSolve>,
    /// Cancel flag shared with the most recent solve's worker thread. Setting
    /// it interrupts `HiGHS`; the worker's clone is dropped when the thread
    /// ends, which is how a solve still winding down is detected.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Cancel flag shared with the most recent diagnosis's worker thread, on
    /// the same terms as `cancel`.
    pub diagnosis_cancel: Option<Arc<AtomicBool>>,
    /// `q` was pressed while a solve runs: the running pop-up asks to confirm.
    pub confirm_quit: bool,
}

impl SolverSession {
    fn new() -> Self {
        Self {
            state: SolveState::Idle,
            view: SolveViewState::default(),
            receive: None,
            receive2: None,
            render_cache: SolveRenderCache::Empty,
            diagnosis: DiagnosisState::Idle,
            receive_diagnosis: None,
            solved_problem: None,
            what_if_problem: None,
            key: String::new(),
            cache: Vec::new(),
            cancel: None,
            diagnosis_cancel: None,
            confirm_quit: false,
        }
    }

    /// Whether a diagnosis's worker thread is still running — including one
    /// that was discarded and has not yet stopped.
    pub(crate) fn diagnosis_in_flight(&self) -> bool {
        self.diagnosis_cancel.as_ref().is_some_and(|flag| Arc::strong_count(flag) > 1)
    }

    /// A fresh cancel flag for a diagnosis about to start, kept here and
    /// returned for the worker thread.
    pub(crate) fn arm_diagnosis(&mut self) -> Arc<AtomicBool> {
        debug_assert!(!self.diagnosis_in_flight(), "a diagnosis must not start while another is still running");
        let flag = Arc::new(AtomicBool::new(false));
        self.diagnosis_cancel = Some(Arc::clone(&flag));
        flag
    }

    /// Whether a solve's worker thread is still running — including one that
    /// was cancelled and has not yet stopped.
    pub(crate) fn solve_in_flight(&self) -> bool {
        self.cancel.as_ref().is_some_and(|flag| Arc::strong_count(flag) > 1)
    }

    /// Stop the running solve: interrupt `HiGHS` and drop the result channels,
    /// so whatever the worker sends when it stops is discarded.
    pub(crate) fn cancel_running(&mut self) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
        self.state = SolveState::Idle;
        self.receive = None;
        self.receive2 = None;
        self.confirm_quit = false;
    }

    /// A fresh cancel flag for a solve about to start, kept here and returned
    /// for the worker thread.
    pub(crate) fn arm_cancel(&mut self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel = Some(Arc::clone(&flag));
        self.confirm_quit = false;
        flag
    }

    /// Discard any in-flight or completed diagnosis (new solve or overlay
    /// closed), interrupting a running one. Its flag is kept, so the worker
    /// still counts as in flight until it has stopped.
    pub(crate) fn reset_diagnosis(&mut self) {
        if let Some(flag) = &self.diagnosis_cancel {
            flag.store(true, Ordering::Relaxed);
        }
        self.diagnosis = DiagnosisState::Idle;
        self.receive_diagnosis = None;
    }

    /// Reset to `Idle`, filing a completed result under the current key so the
    /// overlay can be reopened without re-solving. In-flight, failed and
    /// picker states are simply dropped.
    pub(crate) fn close_overlay(&mut self) {
        let state = std::mem::replace(&mut self.state, SolveState::Idle);
        if matches!(state, SolveState::Done(_) | SolveState::DoneBoth(_)) {
            let key = std::mem::take(&mut self.key);
            self.cache.retain(|entry| entry.key != key);
            if self.cache.len() >= SOLVE_CACHE_CAPACITY {
                self.cache.remove(0);
            }
            self.cache.push(CachedSolve {
                key,
                state,
                solved_problem: self.solved_problem.clone(),
                what_if_problem: self.what_if_problem.clone(),
                view: std::mem::take(&mut self.view),
            });
        }
        self.reset_diagnosis();
    }

    /// Take back the cached solve for `key`, if one is held.
    pub(crate) fn take_cached(&mut self, key: &str) -> Option<CachedSolve> {
        let index = self.cache.iter().position(|entry| entry.key == key)?;
        Some(self.cache.remove(index))
    }
}

#[allow(clippy::struct_excessive_bools)] // independent toggles (help, ignore-order, mouse capture, quit), not a hidden state machine
pub struct App {
    /// Diff (two files) or Inspect (single file). Fixed at startup.
    pub mode: AppMode,
    pub report: LpDiffReport,
    pub active_section: Section,
    pub focus: Focus,
    pub filter: DiffFilter,
    pub should_quit: bool,

    /// Whether the help pop-up overlay is visible.
    pub show_help: bool,

    /// Scroll offset for the help overlay (clamped to content height at draw time).
    pub help_scroll: u16,

    /// `Ctrl+P` command palette state.
    pub palette: CommandPaletteState,

    /// What-if prompt overlay (`E` on a selected constraint), when open.
    pub what_if: Option<WhatIfPrompt>,

    /// Diagnostics pane (`D`), when open.
    pub diagnostics: Option<ScrollPane>,
    /// A slow read-only analysis (solve profile, unbounded ray, ranging) that
    /// runs off-thread and renders into a pane. Not part of `SolverSession`:
    /// these run against the model, not against a solve, and must survive the
    /// solve overlay opening and closing.
    pub analysis: AnalysisState,
    /// Channel carrying the finished pane back from the analysis worker thread.
    /// The worker builds the lines, so the UI thread never formats a large
    /// report.
    pub receive_analysis: Option<mpsc::Receiver<Result<ScrollPane, String>>>,

    /// Presolve rule picker overlay (`P`): the highlighted rule when open.
    pub presolve_cursor: Option<usize>,

    /// Which presolve rules are enabled, persisted across openings of the picker.
    pub presolve_rules: crate::presolve::RuleSet,

    /// Stats from the most recent presolve run, shown when the picker reopens.
    pub last_presolve: Option<crate::presolve::PresolveStats>,

    /// Presolve log pane (`l` from the picker): what the rewrite removed, row
    /// by row and column by column.
    pub presolve_log: Option<ScrollPane>,

    /// Scroll offset for the detail panel when it has focus.
    pub detail_scroll: usize,

    /// Section selector list state (tracks which of the 5 sections is highlighted).
    pub section_selector_state: ListState,

    /// Per-section view states: [Variables, Constraints, Objectives].
    pub section_states: [SectionViewState; 3],

    /// Layout rectangles and dimensions stored during draw.
    pub layout: LayoutRects,

    /// Yank (clipboard) flash state.
    pub yank: YankState,

    /// Pending yank chord state (`y` → waiting for `o`, `n`, or `y`).
    pub pending_yank: PendingYank,

    /// Telescope-style search pop-up state.
    pub search_popup: SearchPopupState,

    /// Matches of the last confirmed search, in rank order, for `n`/`N` repeat.
    /// Cleared on report rebuilds (the entry indices go stale).
    pub(crate) last_search: Vec<(Section, usize)>,

    /// Cursor into `last_search`: the match most recently jumped to.
    pub(crate) last_search_cursor: usize,

    /// Navigation jumplist for Ctrl+o / Ctrl+i.
    pub jumplist: JumpList,

    /// `HiGHS` solver session (state + view + channel).
    pub solver: SolverSession,

    /// Path to the first file.
    pub file1_path: PathBuf,

    /// Path to the second file.
    pub file2_path: PathBuf,

    /// Parsed problem for the first file (shared with solver threads).
    pub problem1: Arc<LpProblem>,

    /// Parsed problem for the second file (shared with solver threads).
    pub problem2: Arc<LpProblem>,

    /// Raw source text of the first file (for raw text diff view).
    pub raw_text1: Arc<str>,

    /// Raw source text of the second file (for raw text diff view).
    pub raw_text2: Arc<str>,

    /// Whether to show parsed diff or raw text side-by-side in the detail panel.
    pub detail_view: DetailView,

    /// Pre-built flat haystack for the search pop-up (built once in `App::new`).
    pub(crate) search_haystack: Vec<HaystackEntry>,

    /// Re-usable buffer for fuzzy search name references, avoiding per-keystroke `Vec` allocation.
    /// Rebuilt when the haystack changes (indices correspond 1:1 with `search_haystack`).
    pub(crate) search_name_buffer: Vec<String>,

    /// Per-entry content text for the `c:` content search mode (indices
    /// correspond 1:1 with `search_haystack`). Built lazily on the first
    /// content query and cleared when the report is rebuilt; empty = unbuilt.
    pub(crate) search_content_buffer: Vec<String>,

    /// Cached coefficient rows for the detail panel, avoiding per-frame `BTreeMap` + String allocations.
    /// Invalidated when the selected entry changes.
    pub(crate) coeff_row_cache: Option<CoeffRowCache>,

    /// Pre-built summary lines, avoiding per-frame `format!` allocations.
    /// Built once in `App::new()` since the report data never changes.
    pub(crate) summary_lines: Vec<Line<'static>>,

    /// Pre-built Numerics section lines (per-file conditioning view).
    /// Rebuilt in `rebuild_report()` since the analyses change on watch reloads.
    pub(crate) numerics_lines: Vec<Line<'static>>,

    /// The Numerics tab's issue badge (see `numerics_badge`), rebuilt with
    /// `numerics_lines`.
    pub(crate) numerics_badge: Option<(usize, lp_parser_rs::analysis::IssueSeverity)>,

    /// Pre-computed diff summary. Built once in `App::new()` since
    /// the report data never changes, avoiding repeated recomputation.
    pub(crate) cached_summary: DiffSummary,

    /// When `true`, entries whose only change is coefficient ordering are hidden.
    pub ignore_order: bool,

    /// Active sort order for the sidebar name lists (cycled with `s`).
    pub sort_mode: SortMode,

    /// Comparison options used to build (and rebuild) the diff report.
    /// Tolerances are mutated live by the `t` / `T` keys.
    pub diff_options: DiffOptions,

    /// Constraint `NameId` → 1-based line number for file 1, kept for rebuilds.
    pub(crate) line_map1: HashMap<NameId, usize>,

    /// Constraint `NameId` → 1-based line number for file 2, kept for rebuilds.
    pub(crate) line_map2: HashMap<NameId, usize>,

    /// Watch-mode session (`--watch`): debounce state + in-flight reload channel.
    pub watch: WatchSession,

    /// Columns added to (or taken from) the sidebar's automatic width with
    /// `>` / `<`.
    pub sidebar_adjust: i16,

    /// Whether the TUI captures the mouse (`M` toggles it). Off, the terminal
    /// handles the mouse itself, so text can be selected and copied natively.
    /// The main loop applies changes to the terminal.
    pub mouse_capture: bool,

    /// Display width of the longest entry name in the report, which the
    /// sidebar grows towards. Recomputed with the report.
    pub(crate) longest_name: usize,
}

/// Cached coefficient rows keyed on (section, `entry_index`).
pub struct CoeffRowCache {
    pub section: Section,
    pub entry_index: usize,
    pub rows: Vec<CoefficientRow>,
}

/// Append entries from a single section into the haystack and name buffer.
fn append_section_haystack<T: DiffEntry>(haystack: &mut Vec<HaystackEntry>, names: &mut Vec<String>, section: Section, entries: &[T]) {
    for (index, entry) in entries.iter().enumerate() {
        haystack.push(HaystackEntry { section, index, kind: entry.kind() });
        names.push(entry.name().to_owned());
    }
}

/// Build the flat search haystack and name buffer from all three sections of the report.
///
/// The haystack and name buffer are built in lockstep so that `names[i]` is the
/// display name for `haystack[i]`. This avoids cloning each name twice.
/// Display width of the longest entry name across all three sections.
fn longest_entry_name(report: &LpDiffReport) -> usize {
    use unicode_width::UnicodeWidthStr as _;
    let variables = report.variables.entries.iter().map(|entry| entry.name.width());
    let constraints = report.constraints.entries.iter().map(|entry| entry.name.width());
    let objectives = report.objectives.entries.iter().map(|entry| entry.name.width());
    variables.chain(constraints).chain(objectives).max().unwrap_or(0)
}

fn build_haystack(report: &LpDiffReport) -> (Vec<HaystackEntry>, Vec<String>) {
    let total = report.variables.entries.len() + report.constraints.entries.len() + report.objectives.entries.len();
    let mut haystack = Vec::with_capacity(total);
    let mut names = Vec::with_capacity(total);

    append_section_haystack(&mut haystack, &mut names, Section::Variables, &report.variables.entries);
    append_section_haystack(&mut haystack, &mut names, Section::Constraints, &report.constraints.entries);
    append_section_haystack(&mut haystack, &mut names, Section::Objectives, &report.objectives.entries);

    debug_assert_eq!(haystack.len(), names.len(), "haystack and name buffer must have equal length");
    (haystack, names)
}

/// Format a tolerance value compactly: "off" for zero, scientific notation otherwise.
pub(crate) fn format_tolerance(value: f64) -> String {
    debug_assert!(value.is_finite() && value >= 0.0, "tolerance must be finite and non-negative");
    if value == 0.0 { "off".to_owned() } else { format!("{value:e}") }
}

/// Build the Summary-section lines for the active mode.
fn build_mode_summary_lines(mode: AppMode, report: &LpDiffReport, summary: &DiffSummary, problem: &LpProblem) -> Vec<Line<'static>> {
    match mode {
        AppMode::Diff => crate::widgets::summary::build_summary_lines(report, summary, &report.analysis1, &report.analysis2),
        AppMode::Inspect => crate::widgets::summary::build_inspect_summary_lines(&report.file1, problem, summary, &report.analysis1),
    }
}

/// Build the Numerics-section lines for the active mode.
fn build_mode_numerics_lines(mode: AppMode, report: &LpDiffReport, _problem: &LpProblem) -> Vec<Line<'static>> {
    match mode {
        AppMode::Diff => crate::widgets::numerics::build_numerics_lines(report),
        AppMode::Inspect => crate::widgets::numerics::build_inspect_numerics_lines(&report.file1, &report.analysis1),
    }
}

/// One pre-computed tab bar label: the section name plus optional pre-styled
/// per-kind change counts (diff mode only) rendered after the name.
pub(crate) struct TabLabel {
    /// Section name; inspect mode appends its entry count (e.g. "Variables (8)").
    pub name: Cow<'static, str>,
    /// The compact form drawn when the full labels do not fit (e.g. "Vars (8)").
    pub short: Cow<'static, str>,
    /// Coloured count spans (e.g. `+2 -1 ~5`, or `~5/12` under a kind filter).
    /// Empty for static sections, inspect mode, and sections with no changes.
    pub counts: Vec<ratatui::text::Span<'static>>,
}

/// Build the coloured change-count spans for one list section's tab.
///
/// With no kind filter, shows the non-zero per-kind counts in the same
/// `+`/`-`/`~`/`>` vocabulary as the status bar. Under a kind filter, shows
/// only that kind's count over the section total (e.g. `~5/12`) so a filtered
/// list is never mistaken for the whole section.
fn tab_count_spans(counts: &crate::diff_model::DiffCounts, filter: DiffFilter) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::style::Style;
    use ratatui::text::Span;
    let t = crate::theme::theme();
    let kind_counts = [
        (counts.added, "+", t.added),
        (counts.removed, "-", t.removed),
        (counts.modified, "~", t.modified),
        (counts.renamed, crate::widgets::status_bar::RENAMED, t.accent),
    ];
    match filter {
        DiffFilter::All => {
            let mut spans = Vec::new();
            for (count, prefix, colour) in kind_counts {
                if count > 0 {
                    if !spans.is_empty() {
                        spans.push(Span::raw(" "));
                    }
                    spans.push(Span::styled(format!("{prefix}{count}"), Style::default().fg(colour)));
                }
            }
            spans
        }
        DiffFilter::Added | DiffFilter::Removed | DiffFilter::Modified | DiffFilter::Renamed => {
            let index = match filter {
                DiffFilter::Added => 0,
                DiffFilter::Removed => 1,
                DiffFilter::Modified => 2,
                _ => 3,
            };
            let (count, prefix, colour) = kind_counts[index];
            vec![
                Span::styled(format!("{prefix}{count}"), Style::default().fg(colour)),
                Span::styled(format!("/{}", counts.total()), Style::default().fg(t.muted)),
            ]
        }
    }
}

/// Build pre-computed tab bar labels: list sections carry their entry/change
/// counts, and Numerics its issue badge (`!N`, coloured by the worst severity).
pub(crate) fn build_section_labels(
    summary: &DiffSummary,
    mode: AppMode,
    filter: DiffFilter,
    numerics_badge: Option<(usize, lp_parser_rs::analysis::IssueSeverity)>,
) -> [TabLabel; 5] {
    Section::ALL.map(|section| {
        if section == Section::Numerics {
            let counts = numerics_badge.map_or_else(Vec::new, |(count, severity)| {
                let style = ratatui::style::Style::default().fg(crate::widgets::severity_colour(severity));
                vec![ratatui::text::Span::styled(format!("!{count}"), style)]
            });
            return TabLabel { name: Cow::Borrowed(section.label()), short: Cow::Borrowed(section.short_label()), counts };
        }
        let counts = match section {
            Section::Summary | Section::Numerics => None,
            Section::Variables => Some(&summary.variables),
            Section::Constraints => Some(&summary.constraints),
            Section::Objectives => Some(&summary.objectives),
        };
        match (mode, counts) {
            (_, None) => TabLabel { name: Cow::Borrowed(section.label()), short: Cow::Borrowed(section.short_label()), counts: Vec::new() },
            (AppMode::Inspect, Some(counts)) => TabLabel {
                name: Cow::Owned(format!("{} ({})", section.label(), counts.changed())),
                short: Cow::Owned(format!("{} ({})", section.short_label(), counts.changed())),
                counts: Vec::new(),
            },
            (AppMode::Diff, Some(counts)) => TabLabel {
                name: Cow::Borrowed(section.label()),
                short: Cow::Borrowed(section.short_label()),
                counts: tab_count_spans(counts, filter),
            },
        }
    })
}

impl App {
    /// Construct the diff-mode app (two files).
    #[allow(clippy::too_many_arguments)] // constructor mirrors main.rs wiring; a params struct adds noise
    pub fn new(
        report: LpDiffReport,
        file1_path: PathBuf,
        file2_path: PathBuf,
        problem1: Arc<LpProblem>,
        problem2: Arc<LpProblem>,
        raw_text1: Arc<str>,
        raw_text2: Arc<str>,
        diff_options: DiffOptions,
        line_map1: HashMap<NameId, usize>,
        line_map2: HashMap<NameId, usize>,
    ) -> Self {
        Self::build(
            AppMode::Diff,
            report,
            file1_path,
            file2_path,
            problem1,
            problem2,
            raw_text1,
            raw_text2,
            diff_options,
            line_map1,
            line_map2,
        )
    }

    /// Construct the inspect-mode app (single file).
    ///
    /// The unused "file 2" slots are populated from the single file so the shared
    /// diff-oriented plumbing (solver problems, raw-text lookups, watch mtimes)
    /// has valid values; inspect-mode presentation never surfaces them as a
    /// second file.
    pub fn new_inspect(
        report: LpDiffReport,
        file_path: PathBuf,
        problem: Arc<LpProblem>,
        raw_text: Arc<str>,
        line_map: HashMap<NameId, usize>,
    ) -> Self {
        Self::build(
            AppMode::Inspect,
            report,
            file_path.clone(),
            file_path,
            Arc::clone(&problem),
            problem,
            Arc::clone(&raw_text),
            raw_text,
            DiffOptions::default(),
            line_map.clone(),
            line_map,
        )
    }

    #[allow(clippy::too_many_arguments)] // constructor mirrors main.rs wiring; a params struct adds noise
    fn build(
        mode: AppMode,
        report: LpDiffReport,
        file1_path: PathBuf,
        file2_path: PathBuf,
        problem1: Arc<LpProblem>,
        problem2: Arc<LpProblem>,
        raw_text1: Arc<str>,
        raw_text2: Arc<str>,
        diff_options: DiffOptions,
        line_map1: HashMap<NameId, usize>,
        line_map2: HashMap<NameId, usize>,
    ) -> Self {
        let mut section_selector_state = ListState::default();
        section_selector_state.select(Some(0));

        let (haystack, names) = build_haystack(&report);
        let longest_name = longest_entry_name(&report);

        // Pre-build summary lines once (report data never changes).
        let report_summary = report.summary();
        let summary_lines = build_mode_summary_lines(mode, &report, &report_summary, &problem1);
        let numerics_lines = build_mode_numerics_lines(mode, &report, &problem1);
        let numerics_badge = crate::widgets::numerics::numerics_badge(mode, &report);

        Self {
            mode,
            report,
            active_section: Section::Summary,
            focus: Focus::SectionSelector,
            filter: DiffFilter::All,
            should_quit: false,
            show_help: false,
            help_scroll: 0,
            palette: CommandPaletteState { visible: false, query: tui_input::Input::default(), filtered: Vec::new(), selected: 0 },
            what_if: None,
            diagnostics: None,
            analysis: AnalysisState::Idle,
            receive_analysis: None,
            presolve_cursor: None,
            presolve_rules: crate::presolve::DEFAULT_RULES,
            last_presolve: None,
            presolve_log: None,
            detail_scroll: 0,
            section_selector_state,
            section_states: [SectionViewState::new(), SectionViewState::new(), SectionViewState::new()],
            layout: LayoutRects {
                section_selector: Rect::default(),
                name_list: Rect::default(),
                detail: Rect::default(),
                name_list_height: 0,
                detail_height: 0,
                detail_content_lines: 0,
                tab_bounds: [(0, 0); 5],
            },
            yank: YankState { flash: None, message: String::new(), level: FlashLevel::Info },
            pending_yank: PendingYank::None,
            search_popup: SearchPopupState {
                visible: false,
                query: tui_input::Input::default(),
                results: Vec::new(),
                selected: 0,
                scroll: 0,
                cached_result_lines: Vec::new(),
                regex_error: None,
            },
            last_search: Vec::new(),
            last_search_cursor: 0,
            jumplist: JumpList::new(),
            solver: SolverSession::new(),
            file1_path,
            file2_path,
            problem1,
            problem2,
            raw_text1,
            raw_text2,
            detail_view: DetailView::default(),
            search_name_buffer: names,
            search_haystack: haystack,
            search_content_buffer: Vec::new(),
            coeff_row_cache: None,
            summary_lines,
            numerics_lines,
            numerics_badge,
            cached_summary: report_summary,
            ignore_order: false,
            sort_mode: SortMode::default(),
            diff_options,
            line_map1,
            line_map2,
            watch: WatchSession::disabled(),
            sidebar_adjust: 0,
            mouse_capture: true,
            longest_name,
        }
    }

    /// Set the kind filter. The tab bar reads it directly at draw time.
    pub(crate) const fn apply_filter(&mut self, filter: DiffFilter) {
        self.filter = filter;
    }

    /// Flash a status-bar message at the given severity.
    pub(crate) fn flash(&mut self, level: FlashLevel, message: impl Into<String>) {
        self.yank.message = message.into();
        self.yank.level = level;
        self.yank.flash = Some(Instant::now());
    }

    /// Flash neutral feedback.
    pub(crate) fn flash_status(&mut self, message: impl Into<String>) {
        self.flash(FlashLevel::Info, message);
    }

    /// Flash a success.
    pub(crate) fn flash_ok(&mut self, message: impl Into<String>) {
        self.flash(FlashLevel::Ok, message);
    }

    /// Flash a refusal or no-op.
    pub(crate) fn flash_warn(&mut self, message: impl Into<String>) {
        self.flash(FlashLevel::Warn, message);
    }

    /// Flash a failure; it stays until the next key press.
    pub(crate) fn flash_error(&mut self, message: impl Into<String>) {
        self.flash(FlashLevel::Err, message);
    }

    /// A diff-only action was pressed in inspect mode: brief no-op hint.
    pub(crate) fn flash_diff_only(&mut self) {
        debug_assert!(matches!(self.mode, AppMode::Inspect), "flash_diff_only is only reachable in inspect mode");
        self.flash_warn("Not available in inspect mode (single file)");
    }

    /// Toggle between parsed and raw text detail views.
    pub const fn toggle_detail_view(&mut self) {
        self.detail_view = match self.detail_view {
            DetailView::Parsed => DetailView::Raw,
            DetailView::Raw => DetailView::Parsed,
        };
        self.detail_scroll = 0;
    }

    /// Toggle hiding of order-only diff entries.
    pub fn toggle_ignore_order(&mut self) {
        let selected = self.selected_entry_index();
        self.ignore_order = !self.ignore_order;
        self.invalidate_cache();
        self.rebuild_summary();
        self.ensure_active_section_cache();
        self.reselect_entry(selected);
    }

    /// Rebuild the cached summary and summary lines, adjusting counts when
    /// `ignore_order` is active (order-only entries move from modified to unchanged).
    fn rebuild_summary(&mut self) {
        let mut summary = self.report.summary();
        if self.ignore_order {
            for counts in [&mut summary.variables, &mut summary.constraints, &mut summary.objectives] {
                counts.modified -= counts.order_only;
                counts.unchanged += counts.order_only;
            }
        }
        self.summary_lines = build_mode_summary_lines(self.mode, &self.report, &summary, &self.problem1);
        self.cached_summary = summary;
    }

    /// Cycle the sidebar sort mode: Name → `AbsDelta` → `RelDelta` → Name.
    pub fn cycle_sort_mode(&mut self) {
        let selected = self.selected_entry_index();
        self.sort_mode = self.sort_mode.next();
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.reselect_entry(selected);
        let label = match self.sort_mode {
            SortMode::Name => "Sort: name",
            SortMode::AbsDelta => "Sort: |\u{394}| (largest first)",
            SortMode::RelDelta => "Sort: rel\u{394} (largest first)",
        };
        self.flash_status(label);
    }

    /// Cycle the relative tolerance through the presets and rebuild the diff.
    pub fn cycle_rel_tol(&mut self) {
        let value = next_tolerance_preset(self.diff_options.rel_tol);
        self.diff_options.rel_tol = value;
        self.rebuild_report_inner(false);
        self.flash_status(format!("rel_tol = {}", format_tolerance(value)));
    }

    /// Cycle the absolute tolerance through the presets and rebuild the diff.
    pub fn cycle_abs_tol(&mut self) {
        let value = next_tolerance_preset(self.diff_options.abs_tol);
        self.diff_options.abs_tol = value;
        self.rebuild_report_inner(false);
        self.flash_status(format!("abs_tol = {}", format_tolerance(value)));
    }

    /// Rebuild the diff report from the stored problems with the current
    /// `diff_options`, then refresh every report-derived cache.
    ///
    /// Self-contained on purpose: the single rebuild path shared by live
    /// tolerance changes and watch reloads (`poll_watch` re-parses, replaces
    /// `problem1`/`problem2`/line maps, then calls this).
    pub fn rebuild_report(&mut self) {
        self.rebuild_report_inner(true);
    }

    /// Rebuild the report, optionally skipping the numerics cache.
    ///
    /// `analyses_changed` is `false` for tolerance-only changes: the per-file
    /// analyses (and hence `numerics_lines`) are unaffected by tolerances, so
    /// rebuilding them every keystroke is wasted work. Watch reloads pass
    /// `true` because they install fresh analyses.
    fn rebuild_report_inner(&mut self, analyses_changed: bool) {
        // Captured by name: the rebuild reshuffles entries (a tolerance change
        // hides some and reveals others), so a list position would land on a
        // different entry.
        let selected_name = self.selected_entry_name().map(str::to_owned);

        match self.mode {
            AppMode::Diff => {
                let file1 = self.file1_path.display().to_string();
                let file2 = self.file2_path.display().to_string();
                self.report = build_diff_report(&DiffInput {
                    file1: &file1,
                    file2: &file2,
                    p1: &self.problem1,
                    p2: &self.problem2,
                    line_map1: &self.line_map1,
                    line_map2: &self.line_map2,
                    analysis1: self.report.analysis1.clone(),
                    analysis2: self.report.analysis2.clone(),
                    options: self.diff_options.clone(),
                });
            }
            AppMode::Inspect => {
                let file = self.file1_path.display().to_string();
                self.report =
                    crate::inspect_model::build_inspect_report(&file, &self.problem1, &self.line_map1, self.report.analysis1.clone());
            }
        }

        // Report-derived caches: search haystack + name buffer. The content
        // buffer is lazy — clear it and let the next `c:` query rebuild it.
        let (haystack, names) = build_haystack(&self.report);
        self.longest_name = longest_entry_name(&self.report);
        self.search_haystack = haystack;
        self.search_name_buffer = names;
        self.search_content_buffer.clear();

        // The `n`/`N` repeat list holds entry indices into the old report.
        self.last_search.clear();
        self.last_search_cursor = 0;

        // Summary lines + cached summary (respects the active ignore_order setting).
        self.rebuild_summary();

        // Numerics lines depend only on the analyses, which change on watch
        // reloads but not on tolerance changes -- skip the rebuild otherwise.
        if analyses_changed {
            self.numerics_lines = build_mode_numerics_lines(self.mode, &self.report, &self.problem1);
            self.numerics_badge = crate::widgets::numerics::numerics_badge(self.mode, &self.report);
        }

        // Filtered indices, cached sidebar lines, and coefficient row cache.
        self.invalidate_cache();
        self.detail_scroll = 0;
        self.ensure_active_section_cache();
        self.clamp_active_selection();
        self.reselect_by_name(selected_name.as_deref());

        // The search pop-up references haystack indices — refresh if it is open.
        if self.search_popup.visible {
            self.recompute_search_popup();
        }
    }

    /// Select the entry named `name` in the active section if it is still
    /// visible; otherwise leave the selection as it is. Must be called after
    /// `ensure_active_section_cache`.
    fn reselect_by_name(&mut self, name: Option<&str>) {
        let (Some(name), Some(index)) = (name, self.active_section.list_index()) else {
            return;
        };
        let Some(entry) = self.entry_index_by_name(self.active_section, name) else {
            return;
        };
        if let Some(position) = self.section_states[index].cached_indices().iter().position(|&i| i == entry) {
            self.section_states[index].list_state.select(Some(position));
        }
    }

    /// Clamp the active section's list selection to the freshly recomputed
    /// filtered length. Must be called after `ensure_active_section_cache`.
    fn clamp_active_selection(&mut self) {
        let Some(index) = self.active_section.list_index() else {
            return;
        };
        let len = self.section_states[index].cached_indices().len();
        let state = &mut self.section_states[index].list_state;
        match state.selected() {
            Some(_) if len == 0 => state.select(None),
            Some(selected) if selected >= len => state.select(Some(len - 1)),
            _ => {}
        }
    }

    /// Extract raw LP text for the currently selected entry from both files.
    ///
    /// Returns `(old_text, new_text)` where each is `None` if the entry
    /// does not exist in that file or is a variable (not supported).
    pub fn extract_raw_texts(&self) -> (Option<&str>, Option<&str>) {
        let Some(entry_index) = self.selected_entry_index() else {
            return (None, None);
        };
        match self.active_section {
            Section::Constraints => {
                let entry = &self.report.constraints.entries[entry_index];
                let name = &entry.name;
                let old = Self::lookup_constraint_text(name, &self.problem1, &self.raw_text1);
                let new = Self::lookup_constraint_text(name, &self.problem2, &self.raw_text2);
                (old, new)
            }
            Section::Objectives => {
                let entry = &self.report.objectives.entries[entry_index];
                let name = &entry.name;
                let old = Self::lookup_objective_text(name, &self.problem1, &self.raw_text1);
                let new = Self::lookup_objective_text(name, &self.problem2, &self.raw_text2);
                (old, new)
            }
            _ => (None, None),
        }
    }

    /// Look up a constraint by name in a problem and extract its raw text.
    fn lookup_constraint_text<'a>(name: &str, problem: &LpProblem, raw_text: &'a str) -> Option<&'a str> {
        let name_id = problem.name_id(name)?;
        let constraint = problem.constraints.get(&name_id)?;
        let offset = constraint.byte_offset()?;
        Some(crate::widgets::raw_diff::extract_entry_text(raw_text, offset))
    }

    /// Look up an objective by name in a problem and extract its raw text.
    fn lookup_objective_text<'a>(name: &str, problem: &LpProblem, raw_text: &'a str) -> Option<&'a str> {
        let name_id = problem.name_id(name)?;
        let objective = problem.objectives.get(&name_id)?;
        let offset = objective.byte_offset?;
        Some(crate::widgets::raw_diff::extract_entry_text(raw_text, offset))
    }

    /// Invalidate cached filtered indices for all sections and the coefficient row cache.
    pub(crate) fn invalidate_cache(&mut self) {
        for state in &mut self.section_states {
            state.invalidate();
        }
        self.coeff_row_cache = None;
    }

    /// Recompute filtered indices for a given list section.
    /// Panics (in debug) if `section` is a static section (Summary, Numerics).
    fn recompute_section_cache(&mut self, section: Section) {
        debug_assert!(section.list_index().is_some(), "static section {section:?} has no list entries to recompute");
        let index = section.list_index().expect("list section has list_index");
        let filter = self.filter;
        let ignore_order = self.ignore_order;
        let sort = self.sort_mode;
        let badges = self.mode.shows_diff_badges();
        match section {
            Section::Variables => self.section_states[index].recompute(&self.report.variables.entries, filter, ignore_order, sort, badges),
            Section::Constraints => {
                self.section_states[index].recompute(&self.report.constraints.entries, filter, ignore_order, sort, badges);
            }
            Section::Objectives => {
                self.section_states[index].recompute(&self.report.objectives.entries, filter, ignore_order, sort, badges);
            }
            Section::Summary | Section::Numerics => unreachable!("static sections have no list_index"),
        }
    }

    /// Whether any modal overlay is open — a pop-up, a prompt, or one of the
    /// analysis panes. The draw dispatcher dims the screen behind them.
    pub const fn has_overlay(&self) -> bool {
        self.overlay_above_help() || self.show_help
    }

    /// Whether an overlay other than help is open. Help sits lowest in the
    /// key and mouse priority order, so any of these covers it.
    pub(crate) const fn overlay_above_help(&self) -> bool {
        self.search_popup.visible
            || self.palette.visible
            || !matches!(self.solver.state, crate::state::SolveState::Idle)
            || self.what_if.is_some()
            || self.presolve_cursor.is_some()
            || self.presolve_log.is_some()
            || self.diagnostics.is_some()
            || self.analysis.is_open()
    }

    /// Ensure the active section's cache is fresh. Call once per frame before drawing.
    pub fn ensure_active_section_cache(&mut self) {
        let Some(index) = self.active_section.list_index() else {
            return;
        };
        if self.section_states[index].is_dirty() {
            self.recompute_section_cache(self.active_section);
        }
    }

    /// Ensure the coefficient row cache is fresh for the currently selected entry.
    /// Call once per frame before drawing the detail panel.
    pub fn ensure_coeff_row_cache(&mut self) {
        let section = self.active_section;
        let Some(entry_index) = self.selected_entry_index() else {
            return;
        };

        // Check if the cache is already valid for this selection.
        if let Some(cache) = &self.coeff_row_cache
            && cache.section == section
            && cache.entry_index == entry_index
        {
            return;
        }

        // Build coefficient rows based on the active section and entry.
        let rows = match section {
            Section::Constraints => {
                let entry = &self.report.constraints.entries[entry_index];
                match &entry.detail {
                    crate::diff_model::ConstraintDiffDetail::Standard { coeff_changes, old_coefficients, new_coefficients, .. } => {
                        build_coeff_rows(coeff_changes, old_coefficients, new_coefficients, &self.report.interner)
                    }
                    crate::diff_model::ConstraintDiffDetail::Sos { weight_changes, old_weights, new_weights, .. } => {
                        build_coeff_rows(weight_changes, old_weights, new_weights, &self.report.interner)
                    }
                    _ => return,
                }
            }
            Section::Objectives => {
                let entry = &self.report.objectives.entries[entry_index];
                build_coeff_rows(&entry.coeff_changes, &entry.old_coefficients, &entry.new_coefficients, &self.report.interner)
            }
            _ => return,
        };

        self.coeff_row_cache = Some(CoeffRowCache { section, entry_index, rows });
    }

    /// Return cached coefficient rows for the currently selected entry, if available.
    pub(crate) fn cached_coeff_rows(&self) -> Option<&[CoefficientRow]> {
        let section = self.active_section;
        let entry_index = self.selected_entry_index()?;
        let cache = self.coeff_row_cache.as_ref()?;
        if cache.section == section && cache.entry_index == entry_index { Some(&cache.rows) } else { None }
    }

    /// Return the number of items in the name list for the current section.
    /// Must be called after `ensure_active_section_cache()`.
    pub fn name_list_len(&self) -> usize {
        self.active_section.list_index().map_or(0, |index| self.section_states[index].cached_indices().len())
    }

    /// Return a mutable reference to the `ListState` for the active section's name list.
    pub const fn active_name_list_state_mut(&mut self) -> &mut ListState {
        match self.active_section.list_index() {
            Some(index) => &mut self.section_states[index].list_state,
            None => &mut self.section_selector_state,
        }
    }

    /// Return the report-level entry index for the currently selected name list item.
    ///
    /// Returns `None` if the active section is static (Summary, Numerics) or nothing is selected.
    pub(crate) fn selected_entry_index(&self) -> Option<usize> {
        let section_index = self.active_section.list_index()?;
        let state = &self.section_states[section_index];
        let selected = state.list_state.selected()?;
        state.cached_indices().get(selected).copied()
    }

    /// Whether the name list panel has selectable content for the current section.
    pub(crate) fn has_name_list(&self) -> bool {
        self.active_section.list_index().is_some() && self.name_list_len() > 0
    }

    /// Re-select `entry` (a report index) in the active section's freshly
    /// recomputed list, or the first row when it is no longer visible.
    ///
    /// A filter or sort change must not drop the selection: the list would
    /// read `0/N` and the detail panel fall back to the cheat sheet, as though
    /// the user had never picked anything. Must be called after
    /// `ensure_active_section_cache`.
    pub(crate) fn reselect_entry(&mut self, entry: Option<usize>) {
        let Some(index) = self.active_section.list_index() else {
            return;
        };
        let visible = self.section_states[index].cached_indices();
        let kept = entry.and_then(|entry| visible.iter().position(|&i| i == entry));
        let position = kept.or_else(|| (!visible.is_empty()).then_some(0));
        if kept.is_none() {
            self.detail_scroll = 0;
        }
        self.section_states[index].list_state.select(position);
    }

    /// Move down by `n` steps in the focused panel. No-op for `SectionSelector`.
    pub fn page_down(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        match self.focus {
            Focus::SectionSelector => {} // only 4 items, page scroll is not useful
            Focus::NameList => {
                let len = self.name_list_len();
                if len == 0 {
                    return;
                }
                let state = self.active_name_list_state_mut();
                let current = state.selected().unwrap_or(0);
                let new = (current + n).min(len - 1);
                state.select(Some(new));
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_add(n).min(self.max_detail_scroll());
            }
        }
    }

    /// Move up by `n` steps in the focused panel. No-op for `SectionSelector`.
    pub fn page_up(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        match self.focus {
            Focus::SectionSelector => {}
            Focus::NameList => {
                let len = self.name_list_len();
                if len == 0 {
                    return;
                }
                let state = self.active_name_list_state_mut();
                let current = state.selected().unwrap_or(0);
                let new = current.saturating_sub(n);
                state.select(Some(new));
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(n);
            }
        }
    }

    /// Copy `text` to the system clipboard and show a flash message in the status bar.
    ///
    /// `label` is a short description shown on success (e.g. "Yanked: x1").
    ///
    /// Over SSH or inside tmux the system clipboard `arboard` reaches is the
    /// wrong machine's (or none at all), so the text goes to the terminal
    /// instead, as an OSC 52 escape the terminal copies from. Locally,
    /// `arboard` is tried first and OSC 52 is the fallback when it fails. The
    /// flash names the route taken, since OSC 52 cannot confirm it landed.
    pub(crate) fn set_yank_flash(&mut self, label: &str, text: &str) {
        thread_local! {
            static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
        }

        let remote = std::env::var_os("SSH_TTY").is_some() || std::env::var_os("TMUX").is_some();
        if remote {
            match crate::clipboard::write_osc52(text) {
                Ok(()) => self.flash_ok(format!("{label} (via terminal, OSC 52)")),
                Err(error) => self.flash_error(format!("Yank failed: could not write OSC 52: {error}")),
            }
            return;
        }

        let result: Result<(), String> = CLIPBOARD.with_borrow_mut(|cb| {
            if cb.is_none() {
                // Surface initialisation failure (common on SSH/Wayland sessions) instead of
                // silently appearing to succeed; the next yank will retry initialisation.
                *cb = Some(arboard::Clipboard::new().map_err(|error| format!("clipboard unavailable: {error}"))?);
            }
            cb.as_mut().expect("clipboard initialised above").set_text(text).map_err(|error| format!("clipboard failed: {error}"))
        });

        match result {
            Ok(()) => self.flash_ok(format!("{label} (clipboard)")),
            Err(clipboard_error) => match crate::clipboard::write_osc52(text) {
                Ok(()) => self.flash_warn(format!("{label} (via terminal, OSC 52 \u{2014} {clipboard_error})")),
                Err(error) => self.flash_error(format!("Yank failed: {clipboard_error}; OSC 52: {error}")),
            },
        }
    }

    /// Yank the selected entry's name to the system clipboard.
    pub fn yank_name(&mut self) {
        let Some(name) = self.selected_entry_name() else { return };
        let name = name.to_owned();
        self.set_yank_flash(&format!("Yanked: {name}"), &name);
    }

    /// Yank a single side (old or new) of the selected entry to the system clipboard.
    pub fn yank_side(&mut self, side: Side) {
        if let Some(text) = crate::detail_text::render_side_plain(self, side) {
            let side_label = match side {
                Side::Old => "old",
                Side::New => "new",
            };
            let name = self.selected_entry_name().unwrap_or("entry").to_owned();
            self.set_yank_flash(&format!("Yanked {side_label}: {name}"), &text);
        } else {
            let msg = match side {
                Side::Old => "No old version",
                Side::New => "No new version",
            };
            self.flash_warn(msg);
        }
    }

    /// Yank the full detail panel content as plain text to the system clipboard.
    pub fn yank_detail(&mut self) {
        let Some(text) = crate::detail_text::render_detail_plain(self) else { return };
        let label = match self.active_section {
            Section::Summary => "summary".to_owned(),
            Section::Numerics => "numerics".to_owned(),
            _ => self.selected_entry_name().unwrap_or("detail").to_owned(),
        };
        self.set_yank_flash(&format!("Yanked detail: {label}"), &text);
    }

    /// Export to CSV in the current working directory.
    ///
    /// Diff mode writes the single `lp_diff_report_<timestamp>.csv`; inspect mode
    /// writes the model itself via the core crate's `to_csv`
    /// (`objectives.csv`, `constraints.csv`, `variables.csv`) into a fresh
    /// `<file stem>_csv_<timestamp>` folder, so nothing is overwritten. The
    /// flash gives the full path written.
    pub fn export_csv(&mut self) {
        let dir = match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                self.flash_error(format!("CSV export failed: {e}"));
                return;
            }
        };
        let result: Result<String, String> = match self.mode {
            AppMode::Diff => crate::export::write_diff_csv(&self.report, &dir)
                .map(|filename| format!("Wrote {}", dir.join(filename).display()))
                .map_err(|e| e.to_string()),
            AppMode::Inspect => {
                let stem = self.file1_path.file_stem().map_or_else(|| "model".to_owned(), |stem| stem.to_string_lossy().into_owned());
                crate::export::write_model_csv(&self.problem1, &dir, &stem)
                    .map(|folder| format!("Wrote objectives, constraints and variables CSVs to {}", folder.display()))
                    .map_err(|e| e.to_string())
            }
        };
        match result {
            Ok(message) => self.flash_ok(message),
            Err(e) => self.flash_error(format!("CSV export failed: {e}")),
        }
    }

    /// Return the name of an entry given section and entry index.
    ///
    /// Returns `None` for static sections (Summary, Numerics) or an out-of-bounds index.
    fn entry_name(&self, section: Section, entry_index: usize) -> Option<&str> {
        match section {
            Section::Variables => self.report.variables.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Constraints => self.report.constraints.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Objectives => self.report.objectives.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Summary | Section::Numerics => None,
        }
    }

    /// Return the name of the currently selected entry, if any.
    pub(crate) fn selected_entry_name(&self) -> Option<&str> {
        let entry_index = self.selected_entry_index()?;
        self.entry_name(self.active_section, entry_index)
    }

    /// The current navigation position, as a jumplist entry.
    fn current_jump(&self) -> JumpEntry {
        let entry_name = self.selected_entry_name().map(str::to_owned);
        JumpEntry { section: self.active_section, entry_name, detail_scroll: self.detail_scroll, filter: self.filter }
    }

    /// Record the current navigation position in the jumplist.
    pub(crate) fn record_jump(&mut self) {
        let current = self.current_jump();
        self.jumplist.push(current);
    }

    /// Report index of the entry named `name` in `section`, if it still exists.
    fn entry_index_by_name(&self, section: Section, name: &str) -> Option<usize> {
        match section {
            Section::Variables => self.report.variables.entries.iter().position(|e| e.name == name),
            Section::Constraints => self.report.constraints.entries.iter().position(|e| e.name == name),
            Section::Objectives => self.report.objectives.entries.iter().position(|e| e.name == name),
            Section::Summary | Section::Numerics => None,
        }
    }

    /// Update active section, keeping the (now invisible) selector state in
    /// sync — it still backs keyboard navigation over the tab bar.
    pub(crate) const fn set_active_section(&mut self, section: Section) {
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));
    }

    /// Step back in the jumplist and restore that position (`Ctrl+o` / palette).
    pub(crate) fn jump_back(&mut self) {
        let current = self.current_jump();
        if let Some(entry) = self.jumplist.go_back(current) {
            let entry = entry.clone();
            self.restore_jump(&entry);
        }
    }

    /// Step forward in the jumplist and restore that position (`Ctrl+i` / palette).
    pub(crate) fn jump_forward(&mut self) {
        if let Some(entry) = self.jumplist.go_forward() {
            let entry = entry.clone();
            self.restore_jump(&entry);
        }
    }

    /// Navigate to a jumplist entry, restoring section, selection, scroll, and filter.
    ///
    /// The entry is found by name among the rows now visible; when it has gone
    /// (a reload removed it) or is hidden (`ignore_order`), the first row is
    /// selected instead.
    pub(crate) fn restore_jump(&mut self, entry: &JumpEntry) {
        self.set_active_section(entry.section);
        self.apply_filter(entry.filter);
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.detail_scroll = entry.detail_scroll;

        if let Some(index) = entry.section.list_index() {
            let selection = entry.entry_name.as_deref().map(|name| {
                let visible = self.section_states[index].cached_indices();
                self.entry_index_by_name(entry.section, name)
                    .and_then(|entry_index| visible.iter().position(|&i| i == entry_index))
                    .or_else(|| (!visible.is_empty()).then_some(0))
            });
            self.section_states[index].list_state.select(selection.flatten());
        }

        self.focus =
            if entry.entry_name.is_some() && entry.section.list_index().is_some() { Focus::NameList } else { Focus::SectionSelector };
    }

    /// Enable watch mode, anchoring the debounce baseline at the current mtimes.
    pub fn enable_watch(&mut self) {
        self.watch.enabled = true;
        self.watch.state = WatchState::new(crate::watch::read_mtime(&self.file1_path), crate::watch::read_mtime(&self.file2_path));
    }

    /// Watch-mode tick: drain a finished background reload, or poll both files'
    /// mtimes and spawn a reload once a change has been stable for two ticks.
    ///
    /// While a reload is in flight further triggers are ignored; polling
    /// re-arms automatically on the tick after the result is applied, so a
    /// change made during the parse is still picked up.
    pub fn poll_watch(&mut self) {
        if !self.watch.enabled {
            return;
        }

        if let Some(receive) = &self.watch.receive {
            match receive.try_recv() {
                Ok(Ok(parsed)) => {
                    self.watch.receive = None;
                    self.apply_reload(*parsed);
                }
                Ok(Err(error)) => {
                    // Keep the old report; the watcher retries on the next change.
                    self.watch.receive = None;
                    self.flash_error(format!("reload failed: {error}"));
                }
                Err(mpsc::TryRecvError::Empty) => {} // still parsing
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.watch.receive = None;
                    self.flash_error(format!("reload failed: {}", crate::disconnected("parse")));
                }
            }
            return;
        }

        // Throttle the stat() pair to every 5th tick; see `WatchSession::ticks`.
        self.watch.ticks = self.watch.ticks.wrapping_add(1);
        if !self.watch.ticks.is_multiple_of(5) {
            return;
        }

        let mtime1 = crate::watch::read_mtime(&self.file1_path);
        let mtime2 = crate::watch::read_mtime(&self.file2_path);
        if self.watch.state.observe(mtime1, mtime2) {
            self.spawn_reload();
        }
    }

    /// Spawn a background thread that re-parses both files, mirroring the
    /// `spawn_solver` mpsc pattern so the UI stays responsive.
    fn spawn_reload(&mut self) {
        debug_assert!(self.watch.enabled, "spawn_reload called while watch mode is disabled");
        debug_assert!(self.watch.receive.is_none(), "spawn_reload called while a reload is already in flight");

        let path1 = self.file1_path.clone();
        let path2 = self.file2_path.clone();
        let (sender, receiver) = mpsc::channel();
        self.watch.receive = Some(receiver);

        std::thread::spawn(move || {
            let outcome = crate::watch::reload_files(&path1, &path2);
            // The receiver is dropped if the app quit, so a failed send is
            // expected and deliberately silent: stderr is the alternate screen
            // ratatui is drawing into, and printing there garbles the frame.
            drop(sender.send(outcome));
        });
    }

    /// Apply a completed reload on the main thread: replace the problems and
    /// derived inputs, rebuild the diff report, and reset the solver session
    /// (stale solve results and diagnoses would be misleading; old `Arc`s held
    /// by finished solver threads are harmless).
    fn apply_reload(&mut self, parsed: (ParsedFile, ParsedFile)) {
        let ((problem1, analysis1, line_map1, raw_text1), (problem2, analysis2, line_map2, raw_text2)) = parsed;

        self.problem1 = Arc::new(problem1);
        self.problem2 = Arc::new(problem2);
        self.line_map1 = line_map1;
        self.line_map2 = line_map2;
        self.raw_text1 = raw_text1.into();
        self.raw_text2 = raw_text2.into();

        // rebuild_report sources the analyses from the current report, so the
        // fresh ones must be installed first.
        self.report.analysis1 = analysis1;
        self.report.analysis2 = analysis2;
        self.rebuild_report();

        // Interrupt a running solve before the session goes, and carry its
        // cancel flag over: the worker still holds a clone until it exits, and
        // that is what keeps a new solve from starting alongside it.
        self.solver.cancel_running();
        self.solver.reset_diagnosis();
        let (in_flight, diagnosis_in_flight) = (self.solver.cancel.take(), self.solver.diagnosis_cancel.take());
        self.solver = SolverSession::new();
        self.solver.cancel = in_flight;
        self.solver.diagnosis_cancel = diagnosis_in_flight;
        self.discard_model_derived_state();

        self.flash_ok("reloaded");
    }

    /// Drop everything computed from the old models on a reload, so no stale
    /// result is presented as describing the new files.
    ///
    /// An in-flight analysis is discarded by dropping its receiver: the worker's
    /// send then fails harmlessly. An open what-if prompt keeps the user's
    /// typing but takes the constraint's new RHS, closing if it is gone. The
    /// jumplist records entries by name, so it survives as is.
    fn discard_model_derived_state(&mut self) {
        self.analysis = AnalysisState::Idle;
        self.receive_analysis = None;
        self.diagnostics = None;
        self.presolve_log = None;
        self.last_presolve = None;
        if let Some(prompt) = &mut self.what_if {
            match crate::input::baseline_constraint_rhs(&self.problem1, &prompt.constraint_name) {
                Some(rhs) => prompt.current_rhs = rhs,
                None => self.what_if = None,
            }
        }
    }

    /// Whether any time-driven UI is active and needs tick-driven redraws:
    /// a running solve or diagnosis (elapsed-time display), an in-flight
    /// watch reload, or a visible yank flash. Everything else only changes
    /// in response to input, so the main loop skips idle-tick repaints.
    pub const fn is_animating(&self) -> bool {
        self.yank.expires()
            || self.watch.is_reloading()
            || matches!(self.solver.state, SolveState::Running { .. } | SolveState::RunningBoth { .. })
            || matches!(self.solver.diagnosis, DiagnosisState::Running { .. })
            || matches!(self.analysis, AnalysisState::Running { .. })
    }

    /// Poll the solver channel(s) for results, transitioning state when complete.
    pub fn poll_solve(&mut self) {
        if matches!(self.solver.state, SolveState::RunningBoth { .. }) {
            self.poll_solve_both();
        } else {
            self.poll_solve_single();
        }
        self.poll_diagnosis();
        self.poll_analysis();
    }

    /// Poll the analysis channel (`AnalysisState::Running`).
    fn poll_analysis(&mut self) {
        let Some(receive) = &self.receive_analysis else {
            return;
        };
        let AnalysisState::Running { label, .. } = self.analysis else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok(pane)) => {
                self.analysis = AnalysisState::Done { label, pane };
                self.receive_analysis = None;
            }
            Ok(Err(error)) => {
                self.analysis = AnalysisState::Failed { label, error };
                self.receive_analysis = None;
            }
            Err(mpsc::TryRecvError::Empty) => {} // still running
            Err(mpsc::TryRecvError::Disconnected) => {
                self.analysis = AnalysisState::Failed { label, error: crate::disconnected(label) };
                self.receive_analysis = None;
            }
        }
    }

    /// Poll the infeasibility-diagnosis channel (`DiagnosisState::Running`).
    fn poll_diagnosis(&mut self) {
        let Some(receive) = &self.solver.receive_diagnosis else {
            return;
        };
        let DiagnosisState::Running { file, .. } = &self.solver.diagnosis else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok(diagnosis)) => {
                self.solver.diagnosis = DiagnosisState::Done { file: file.clone(), diagnosis: Box::new(diagnosis) };
                self.solver.receive_diagnosis = None;
            }
            Ok(Err(error)) => {
                self.solver.diagnosis = DiagnosisState::Failed(error);
                self.solver.receive_diagnosis = None;
            }
            Err(mpsc::TryRecvError::Empty) => {} // still running
            Err(mpsc::TryRecvError::Disconnected) => {
                self.solver.diagnosis = DiagnosisState::Failed(crate::disconnected("Diagnosis"));
                self.solver.receive_diagnosis = None;
            }
        }
    }

    /// Inner width of the solve results popup, derived from the last drawn
    /// layout. Mirrors the popup sizing in `widgets::solve` (4/5 of the frame
    /// width, at least 60 columns, minus the borders).
    pub(crate) fn solve_popup_inner_width(&self) -> u16 {
        let frame_width = self.layout.detail.x + self.layout.detail.width;
        (frame_width * 4 / 5).max(60).min(frame_width).saturating_sub(2)
    }

    /// Poll a single solver channel (`Running` state).
    fn poll_solve_single(&mut self) {
        let Some(receive) = &self.solver.receive else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok(result)) => {
                let cache = crate::widgets::solve::build_single_solve_cache(&result, self.solve_popup_inner_width());
                self.solver.render_cache = SolveRenderCache::Single(cache);
                self.solver.state = SolveState::Done(Box::new(result));
                self.solver.view = SolveViewState::default();
                self.solver.receive = None;
            }
            Ok(Err(error)) => {
                self.solver.state = SolveState::Failed(error);
                self.solver.receive = None;
            }
            Err(mpsc::TryRecvError::Empty) => {} // still running
            Err(mpsc::TryRecvError::Disconnected) => {
                self.solver.state = SolveState::Failed(crate::disconnected("Solver"));
                self.solver.receive = None;
            }
        }
    }

    /// Poll both solver channels (`RunningBoth` state).
    fn poll_solve_both(&mut self) {
        // Poll channel 1.
        let got1 = self.solver.receive.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(crate::disconnected("Solver 1"))),
        });

        // Poll channel 2.
        let got2 = self.solver.receive2.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(crate::disconnected("Solver 2"))),
        });

        // Handle errors first.
        if let Some(Err(error)) = got1 {
            self.solver.state = SolveState::Failed(error);
            self.solver.receive = None;
            self.solver.receive2 = None;
            return;
        }
        if let Some(Err(error)) = got2 {
            self.solver.state = SolveState::Failed(error);
            self.solver.receive = None;
            self.solver.receive2 = None;
            return;
        }

        // Store successful results into the state variant.
        let SolveState::RunningBoth { result1, result2, .. } = &mut self.solver.state else {
            return;
        };

        if let Some(Ok(r)) = got1 {
            *result1 = Some(Box::new(r));
            self.solver.receive = None;
        }
        if let Some(Ok(r)) = got2 {
            *result2 = Some(Box::new(r));
            self.solver.receive2 = None;
        }

        // Check if both are done.
        let SolveState::RunningBoth { file1, file2, result1, result2, .. } = &mut self.solver.state else {
            return;
        };
        if result1.is_some() && result2.is_some() {
            let r1 = *result1.take().expect("checked Some above");
            let r2 = *result2.take().expect("checked Some above");
            let label1 = file1.clone();
            let label2 = file2.clone();
            let diff_start = Instant::now();
            let mut diff = crate::solver::diff_results(label1, label2, r1, r2, self.solver.view.delta_threshold);
            diff.diff_time = diff_start.elapsed();
            self.solver.render_cache = crate::widgets::solve::build_diff_solve_cache(&diff, self.solve_popup_inner_width());
            self.solver.state = SolveState::DoneBoth(Box::new(diff));
            self.solver.view = SolveViewState { diff_only: true, ..SolveViewState::default() };
            self.solver.receive = None;
            self.solver.receive2 = None;
        }
    }

    /// Recompute the solve diff with the current threshold from `SolveViewState`.
    ///
    /// Extracts `result1` and `result2` from the existing `SolveDiffResult`, rebuilds
    /// the diff with the updated threshold, and replaces the `DoneBoth` state.
    pub fn recompute_solve_diff(&mut self) {
        let SolveState::DoneBoth(old_diff) = std::mem::replace(&mut self.solver.state, SolveState::Idle) else {
            return;
        };
        let threshold = self.solver.view.delta_threshold;
        let diff_start = Instant::now();
        let mut new_diff =
            crate::solver::diff_results(old_diff.file1_label, old_diff.file2_label, old_diff.result1, old_diff.result2, threshold);
        new_diff.diff_time = diff_start.elapsed();
        self.solver.render_cache = crate::widgets::solve::build_diff_solve_cache(&new_diff, self.solve_popup_inner_width());
        self.solver.state = SolveState::DoneBoth(Box::new(new_diff));
    }

    /// Recompute search pop-up results from the current query.
    ///
    /// References the pre-built haystack rather than rebuilding it each time.
    pub fn recompute_search_popup(&mut self) {
        debug_assert!(self.search_popup.visible, "recompute_search_popup called while popup is not visible");
        debug_assert!(
            !self.search_haystack.is_empty()
                || self.report.variables.entries.is_empty()
                    && self.report.constraints.entries.is_empty()
                    && self.report.objectives.entries.is_empty(),
            "haystack must be populated when report has entries"
        );

        self.search_popup.results.clear();
        self.search_popup.selected = 0;
        self.search_popup.scroll = 0;
        self.search_popup.regex_error = None;

        if self.search_popup.query.value().is_empty() {
            self.populate_all_search_results();
            self.rebuild_search_result_lines();
            return;
        }

        // Parse mode and pattern. For fuzzy mode, the pattern is always the full
        // query (no prefix), so we can use the query length to detect that case
        // without holding a borrow across mutable calls.
        let mode = search::parse_query(self.search_popup.query.value()).0;

        match mode {
            SearchMode::Fuzzy => {
                // Fuzzy mode has no prefix — pattern is the entire query.
                self.populate_fuzzy_results();
            }
            SearchMode::Regex | SearchMode::Substring => self.populate_filtered_results(),
            SearchMode::Content => self.populate_content_results(),
        }

        self.rebuild_search_result_lines();
    }

    /// Rebuild the cached styled lines for the current search results.
    fn rebuild_search_result_lines(&mut self) {
        self.search_popup.cached_result_lines = crate::widgets::search_popup::build_result_lines(
            &self.search_popup.results,
            &self.search_name_buffer,
            self.mode.shows_diff_badges(),
        );
    }

    /// Populate search results with all entries (no query filter).
    fn populate_all_search_results(&mut self) {
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            self.search_popup.results.push(SearchResult {
                section: entry.section,
                entry_index: entry.index,
                score: 0,
                match_indices: Vec::new(),
                haystack_index,
                kind: entry.kind,
            });
        }
    }

    /// Populate search results using fuzzy matching.
    /// For fuzzy mode the pattern is always the full query (no prefix).
    fn populate_fuzzy_results(&mut self) {
        debug_assert!(
            self.search_name_buffer.len() == self.search_haystack.len(),
            "search_name_buffer out of sync with search_haystack ({} != {})",
            self.search_name_buffer.len(),
            self.search_haystack.len(),
        );
        let config = frizbee::Config::default();
        let matches = frizbee::Matcher::new(self.search_popup.query.value(), &config).match_list_indices(&self.search_name_buffer);

        for matched in matches {
            let haystack_index = matched.index as usize;
            let entry = &self.search_haystack[haystack_index];
            // frizbee returns indices in reverse order; sort ascending for highlighting.
            let mut indices: Vec<usize> = matched.indices.into_iter().map(|i| i as usize).collect();
            indices.sort_unstable();
            self.search_popup.results.push(SearchResult {
                section: entry.section,
                entry_index: entry.index,
                score: matched.score,
                match_indices: indices,
                haystack_index,
                kind: entry.kind,
            });
        }
    }

    /// Populate search results using regex or substring matching.
    ///
    /// Must not be called for fuzzy mode — that uses `populate_fuzzy_results` instead.
    fn populate_filtered_results(&mut self) {
        debug_assert!(
            !matches!(search::parse_query(self.search_popup.query.value()).0, SearchMode::Fuzzy),
            "populate_filtered_results called with Fuzzy query; use populate_fuzzy_results instead"
        );
        let compiled = CompiledSearch::compile(self.search_popup.query.value());
        // Surface an invalid regex to the pop-up UI — it would otherwise
        // silently match nothing.
        self.search_popup.regex_error = compiled.regex_error();
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            if compiled.matches(&self.search_name_buffer[haystack_index]) {
                self.search_popup.results.push(SearchResult {
                    section: entry.section,
                    entry_index: entry.index,
                    score: 0,
                    match_indices: Vec::new(),
                    haystack_index,
                    kind: entry.kind,
                });
            }
        }
    }

    /// Build the per-entry content text for the `c:` search mode, if not
    /// already built for the current haystack.
    fn ensure_search_content(&mut self) {
        if self.search_content_buffer.len() == self.search_haystack.len() {
            return;
        }
        self.search_content_buffer.clear();
        self.search_content_buffer.reserve(self.search_haystack.len());
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            // Seed with the entry name so `c:` is a superset of `s:`.
            let mut text = self.search_name_buffer[haystack_index].clone();
            match entry.section {
                Section::Variables => self.report.variables.entries[entry.index].write_content(&mut text),
                Section::Constraints => self.report.constraints.entries[entry.index].write_content(&mut text, &self.report.interner),
                Section::Objectives => self.report.objectives.entries[entry.index].write_content(&mut text, &self.report.interner),
                Section::Summary | Section::Numerics => {
                    debug_assert!(false, "haystack must only contain Variables/Constraints/Objectives entries");
                }
            }
            self.search_content_buffer.push(text);
        }
    }

    /// Populate search results using full-text content matching (`c:` mode).
    fn populate_content_results(&mut self) {
        debug_assert!(
            matches!(search::parse_query(self.search_popup.query.value()).0, SearchMode::Content),
            "populate_content_results called with a non-content query"
        );
        self.ensure_search_content();
        let compiled = CompiledSearch::compile(self.search_popup.query.value());
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            if compiled.matches(&self.search_content_buffer[haystack_index]) {
                self.search_popup.results.push(SearchResult {
                    section: entry.section,
                    entry_index: entry.index,
                    score: 0,
                    match_indices: Vec::new(),
                    haystack_index,
                    kind: entry.kind,
                });
            }
        }
    }

    /// Confirm the currently selected search pop-up result: close the pop-up,
    /// switch to the result's section, select the entry, and focus the name list.
    ///
    /// The result list is retained (as `(section, entry_index)` pairs) so `n`/`N`
    /// can step through the remaining matches without reopening the pop-up.
    pub fn confirm_search_selection(&mut self) {
        let Some(result) = self.search_popup.results.get(self.search_popup.selected) else {
            // Nothing selected — just close.
            self.search_popup.visible = false;
            return;
        };

        let section = result.section;
        let entry_index = result.entry_index;

        // Retain the match list for `n`/`N` repeat — but only for a real query;
        // an empty query lists every entry, which is not a search to repeat.
        if self.search_popup.query.value().is_empty() {
            self.last_search.clear();
            self.last_search_cursor = 0;
        } else {
            self.last_search = self.search_popup.results.iter().map(|r| (r.section, r.entry_index)).collect();
            self.last_search_cursor = self.search_popup.selected;
        }

        self.search_popup.visible = false;
        self.jump_to_entry(section, entry_index);
    }

    /// Jump to the next (`forward`) or previous match of the last confirmed
    /// search, wrapping around. Bound to `n`/`N` in normal mode.
    pub(crate) fn repeat_search(&mut self, forward: bool) {
        if self.last_search.is_empty() {
            self.flash_warn("No previous search (press / to search)");
            return;
        }
        let len = self.last_search.len();
        self.last_search_cursor = if forward { (self.last_search_cursor + 1) % len } else { (self.last_search_cursor + len - 1) % len };
        let (section, entry_index) = self.last_search[self.last_search_cursor];
        self.jump_to_entry(section, entry_index);
        self.flash_status(format!("match {}/{len}", self.last_search_cursor + 1));
    }

    /// Switch to `section`, reset the kind filter, select `entry_index` in the
    /// name list, and focus it. Shared by search confirm and `n`/`N` repeat.
    fn jump_to_entry(&mut self, section: Section, entry_index: usize) {
        // Record current position before jumping.
        self.record_jump();

        // Switch to the target section.
        self.set_active_section(section);

        // Reset filter and recompute caches.
        self.apply_filter(DiffFilter::All);
        self.invalidate_cache();
        self.ensure_active_section_cache();

        // Find the position of `entry_index` within the filtered indices and select it.
        let Some(list_index) = section.list_index() else {
            return;
        };
        // Search covers every entry, including the order-only ones `o` hides;
        // reveal them rather than land on whatever row is at the stale position.
        if self.ignore_order && !self.section_states[list_index].cached_indices().contains(&entry_index) {
            self.toggle_ignore_order();
            self.ensure_active_section_cache();
            self.flash_status("Showing order-only changes to reach the match");
        }
        let filtered = self.section_states[list_index].cached_indices();
        debug_assert!(
            filtered.contains(&entry_index),
            "search result entry_index {entry_index} not found in filtered indices for section {section:?}",
        );
        if let Some(position) = filtered.iter().position(|&i| i == entry_index) {
            self.section_states[list_index].list_state.select(Some(position));
        }

        self.focus = Focus::NameList;
        self.detail_scroll = 0;
    }

    /// Toggle mouse capture (`M`), saying what the new state is for.
    pub(crate) fn toggle_mouse_capture(&mut self) {
        self.mouse_capture = !self.mouse_capture;
        if self.mouse_capture {
            self.flash_status("Mouse on: scroll and click (M to select text instead)");
        } else {
            self.flash_status("Mouse off: select text with the terminal (M to restore)");
        }
    }

    /// Widen (`grow`) or narrow the sidebar by one step, within the bounds
    /// [`sidebar_width`](crate::ui::sidebar_width) enforces.
    pub(crate) fn resize_sidebar(&mut self, grow: bool) {
        const STEP: i16 = 4;
        // Bounded so a held key cannot wind the offset far past what the
        // layout will ever honour.
        const LIMIT: i16 = 200;
        let step = if grow { STEP } else { -STEP };
        self.sidebar_adjust = (self.sidebar_adjust + step).clamp(-LIMIT, LIMIT);
    }

    /// Largest useful detail-panel scroll offset: content height minus the
    /// visible window, from the layout recorded on the previous frame. Content
    /// height is stable for a given entry (and scroll resets on entry change),
    /// so last frame's value is the right bound for this frame's input.
    pub(crate) fn max_detail_scroll(&self) -> usize {
        let visible = self.layout.detail_height.saturating_sub(2) as usize; // borders
        self.layout.detail_content_lines.saturating_sub(visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: a watch reload reset only the solver session, so analyses,
    /// panes and presolve results of the old files were shown as current.
    #[test]
    fn a_reload_discards_results_computed_from_the_old_models() {
        let mut app = crate::snapshot_tests::diff_app_from(crate::snapshot_tests::BASE_LP, crate::snapshot_tests::BASE_LP);
        let pane = || ScrollPane { lines: Vec::new(), scroll: 0, export: None };
        let (_sender, receiver) = mpsc::channel();
        app.analysis = AnalysisState::Running { label: "Ranging", started: Instant::now() };
        app.receive_analysis = Some(receiver);
        app.diagnostics = Some(pane());
        app.presolve_log = Some(pane());
        app.last_presolve = Some(crate::presolve::presolve(&app.problem1, app.presolve_rules).1);
        app.what_if = Some(crate::state::WhatIfPrompt {
            constraint_name: "c1".to_owned(),
            current_rhs: 2.0,
            input: tui_input::Input::default(),
            error: None,
        });

        let reparse = |source: &str| crate::parse::parse_text(source, false, "a.lp").expect("fixture parses");
        let changed = crate::snapshot_tests::BASE_LP.replace("c1: x + y >= 2", "c1: x + y >= 7");
        app.apply_reload((reparse(&changed), reparse(&changed)));

        assert!(matches!(app.analysis, AnalysisState::Idle), "an in-flight analysis of the old model is discarded");
        assert!(app.receive_analysis.is_none(), "its result channel is dropped");
        assert!(app.diagnostics.is_none(), "the diagnostics pane described the old model");
        assert!(app.presolve_log.is_none(), "the presolve log described the old model");
        assert!(app.last_presolve.is_none(), "the presolve stats described the old model");
        assert_eq!(app.what_if.as_ref().map(|prompt| prompt.current_rhs), Some(7.0), "the what-if prompt shows the new RHS");
    }

    /// Regression: a reload replaced the solver session outright, dropping the
    /// cancel flag of a running solve — `HiGHS` kept running uncancelled, and a
    /// new solve could start alongside it.
    #[test]
    fn a_reload_cancels_a_running_solve_and_waits_for_it() {
        let mut app = crate::snapshot_tests::diff_app_from(crate::snapshot_tests::BASE_LP, crate::snapshot_tests::BASE_LP);
        let worker = app.solver.arm_cancel();
        app.solver.state = SolveState::Running { file: "model.lp".to_owned(), started: Instant::now() };

        let reparse = |source: &str| crate::parse::parse_text(source, false, "a.lp").expect("fixture parses");
        app.apply_reload((reparse(crate::snapshot_tests::BASE_LP), reparse(crate::snapshot_tests::BASE_LP)));

        assert!(worker.load(Ordering::Relaxed), "the reload interrupts HiGHS");
        assert!(matches!(app.solver.state, SolveState::Idle), "the stale solve's overlay is gone");
        assert!(app.solver.solve_in_flight(), "the worker is still winding down, so no new solve may start");
        drop(worker);
        assert!(!app.solver.solve_in_flight(), "the worker's exit frees the solver");
    }

    /// Regression: the jumplist stored list positions, so once the rows moved
    /// (here `o` hiding an order-only entry above) it restored the wrong entry.
    #[test]
    fn the_jumplist_returns_to_the_same_entry_after_the_rows_move() {
        let mut app = crate::snapshot_tests::diff_app_from(
            "min\nobj: x\nst\nc1: x + y >= 2\nc2: x <= 8\nc3: y <= 4\nend\n",
            "min\nobj: x\nst\nc1: y + x >= 2\nc2: x <= 9\nc3: y <= 5\nend\n",
        );
        app.set_section(Section::Constraints);
        app.active_name_list_state_mut().select(Some(1));
        assert_eq!(app.selected_entry_name(), Some("c2"), "fixture: c2 is the second row");
        app.record_jump();

        app.toggle_ignore_order();
        app.jump_back();

        assert_eq!(app.selected_entry_name(), Some("c2"), "the jump must land on the recorded entry");
    }

    /// Regression: a tolerance change kept the list position rather than the
    /// entry, so once a row above the selection became unchanged (and hidden)
    /// the selection silently moved to the next entry.
    #[test]
    fn a_tolerance_change_keeps_the_selected_entry() {
        let mut app = crate::snapshot_tests::diff_app_from(
            "min\nobj: x\nst\nc1: x + y >= 2\nc2: x <= 8\nc3: y <= 4\nend\n",
            "min\nobj: x\nst\nc1: x + y >= 2.0001\nc2: x <= 9\nc3: y <= 5\nend\n",
        );
        app.set_section(Section::Constraints);
        app.active_name_list_state_mut().select(Some(1));
        assert_eq!(app.selected_entry_name(), Some("c2"), "fixture: c2 is the second row");

        // abs_tol 0.001 makes c1's change vanish; c2 and c3 still differ.
        app.diff_options.abs_tol = 1e-3;
        app.rebuild_report_inner(false);

        assert_eq!(app.selected_entry_name(), Some("c2"), "the selection follows the entry, not the row");
    }

    /// Regression: `Ctrl+i` after `Ctrl+o` returned to the entry just jumped
    /// to rather than the newer position left behind, so it never went forward.
    #[test]
    fn jumping_back_then_forward_returns_to_the_newer_position() {
        let mut app = crate::snapshot_tests::diff_app_from(
            "min\nobj: x\nst\nc1: x + y >= 2\nc2: x <= 8\nc3: y <= 4\nend\n",
            "min\nobj: x\nst\nc1: x + y >= 3\nc2: x <= 9\nc3: y <= 5\nend\n",
        );
        app.set_section(Section::Constraints);
        app.active_name_list_state_mut().select(Some(0));
        app.set_section(Section::Variables);
        assert_eq!(app.active_section, Section::Variables, "fixture: the newest position is Variables");

        app.jump_back();
        assert_eq!(app.active_section, Section::Constraints, "Ctrl+o returns to Constraints");
        assert_eq!(app.selected_entry_name(), Some("c1"));

        app.jump_forward();
        assert_eq!(app.active_section, Section::Variables, "Ctrl+i returns to the newer position");

        app.jump_forward();
        assert_eq!(app.active_section, Section::Variables, "there is nothing newer to go forward to");
        app.jump_back();
        assert_eq!(app.active_section, Section::Constraints, "and Ctrl+o still goes back");
    }

    /// `M` hands the mouse to the terminal and back.
    #[test]
    fn m_toggles_mouse_capture() {
        let mut app = crate::snapshot_tests::inspect_app_from(crate::snapshot_tests::BASE_LP);
        assert!(app.mouse_capture, "the TUI starts with the mouse");
        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('M')));
        assert!(!app.mouse_capture, "M releases it for native selection");
        assert!(app.yank.message.contains("select text"), "and says what for");
        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('M')));
        assert!(app.mouse_capture, "M again takes it back");
    }

    /// An error flash does not expire on the timer; the next key press clears
    /// it. Other flashes expire, and only they keep the tick redrawing.
    #[test]
    fn an_error_flash_stays_until_the_next_key_press() {
        let mut app = crate::snapshot_tests::diff_app_from(crate::snapshot_tests::BASE_LP, crate::snapshot_tests::BASE_LP);
        app.flash_ok("Yanked: x");
        assert!(app.yank.expires() && app.is_animating(), "a success expires on the timer");

        app.flash_error("CSV export failed: disk full");
        assert_eq!(app.yank.level, FlashLevel::Err);
        assert!(!app.yank.expires(), "an error must not expire on its own");
        assert!(!app.is_animating(), "a standing error needs no redraw ticks");

        app.handle_key(crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('j')));
        assert!(app.yank.flash.is_none(), "the next key press clears the error");
    }

    /// Regression: a filter or sort change dropped the selection, so the list
    /// read `0/N` and the detail panel fell back to the cheat sheet.
    #[test]
    fn filter_and_sort_changes_keep_the_selected_entry() {
        let mut app = crate::snapshot_tests::diff_app_from(
            "min\nobj: x\nst\nc1: x + y >= 2\nc2: x <= 8\nc3: y <= 4\nend\n",
            "min\nobj: x\nst\nc1: x + y >= 3\nc2: x <= 9\nc4: y <= 5\nend\n",
        );
        app.set_section(Section::Constraints);
        let c2 = app.report.constraints.entries.iter().position(|entry| entry.name == "c2").expect("c2 is in the report");
        let position = app.section_states[1].cached_indices().iter().position(|&i| i == c2).expect("c2 is listed");
        app.active_name_list_state_mut().select(Some(position));

        app.cycle_sort_mode();
        assert_eq!(app.selected_entry_name(), Some("c2"), "a sort change keeps the entry");
        app.set_filter(DiffFilter::Modified);
        assert_eq!(app.selected_entry_name(), Some("c2"), "a filter that still shows the entry keeps it");
        app.set_filter(DiffFilter::Added);
        assert_eq!(app.active_name_list_state_mut().selected(), Some(0), "a filter hiding the entry falls back to the first row");
    }

    /// Regression: search covers order-only entries that `o` hides, and the
    /// jump found no row for them (a debug panic; the wrong entry in release).
    #[test]
    fn jumping_to_a_hidden_order_only_entry_reveals_it() {
        let mut app = crate::snapshot_tests::diff_app_from(
            "min\nobj: x\nst\nc1: x + y >= 2\nc2: x <= 8\nend\n",
            "min\nobj: x\nst\nc1: y + x >= 2\nc2: x <= 9\nend\n",
        );
        let c1 = app.report.constraints.entries.iter().position(|entry| entry.name == "c1").expect("c1 is in the report");
        assert!(app.report.constraints.entries[c1].order_only, "fixture: c1 differs only in term order");
        app.toggle_ignore_order();

        app.jump_to_entry(Section::Constraints, c1);

        assert!(!app.ignore_order, "the jump must reveal order-only entries");
        assert_eq!(app.selected_entry_index(), Some(c1), "the jump must land on the match");
    }

    /// Closing a completed overlay must file the result under its key, serve it
    /// back once for the same input, and never serve it for a different one.
    #[test]
    fn solve_cache_round_trip_and_eviction() {
        let problem = LpProblem::parse("min\nobj: x\nst\nc1: x >= 1\nend").expect("tiny LP must parse");
        let result = crate::solver::solve_problem(&problem).expect("tiny LP must solve");

        let mut session = SolverSession::new();
        session.key = "file1".to_owned();
        session.state = SolveState::Done(Box::new(result.clone()));
        session.close_overlay();

        assert!(matches!(session.state, SolveState::Idle), "closing must leave the overlay idle");
        assert!(session.take_cached("file2").is_none(), "a different input must miss the cache");

        let cached = session.take_cached("file1").expect("the closed result must be cached");
        assert!(matches!(cached.state, SolveState::Done(_)), "the cached state must be the completed solve");
        assert!(session.take_cached("file1").is_none(), "a restored entry must leave the cache");

        // An in-flight solve holds nothing worth keeping.
        session.key = "file1".to_owned();
        session.state = SolveState::Running { file: "file1".to_owned(), started: Instant::now() };
        session.close_overlay();
        assert!(session.take_cached("file1").is_none(), "an abandoned running solve must not be cached");

        // Past capacity the oldest entry is dropped, the newest kept.
        for index in 0..=SOLVE_CACHE_CAPACITY {
            session.key = format!("file{index}");
            session.state = SolveState::Done(Box::new(result.clone()));
            session.close_overlay();
        }
        assert_eq!(session.cache.len(), SOLVE_CACHE_CAPACITY, "the cache must not grow past its capacity");
        assert!(session.take_cached("file0").is_none(), "the oldest entry must be evicted");
        assert!(session.take_cached(&format!("file{SOLVE_CACHE_CAPACITY}")).is_some(), "the newest entry must survive");
    }
}
