use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
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
pub use crate::state::{AppMode, DiffFilter, Focus, SearchResult, Section, SectionViewState};
use crate::state::{
    DetailView, DiagnosisState, JumpEntry, JumpList, PendingYank, Side, SolveState, SolveViewState, SortMode, WhatIfPrompt,
};
use crate::watch::{WatchSession, WatchState};

/// State for the `Ctrl+P` command palette overlay.
pub struct CommandPaletteState {
    /// Whether the palette overlay is visible.
    pub visible: bool,
    /// Current fuzzy-filter query (readline-style editable input).
    pub query: tui_input::Input,
    /// Indices into [`PaletteCommand::ALL`](crate::state::PaletteCommand::ALL)
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

/// Yank (clipboard) flash state.
pub struct YankState {
    /// Timestamp of the last successful yank, used for the flash message.
    pub flash: Option<Instant>,
    /// Message displayed in the status bar after a successful yank.
    pub message: String,
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
    /// The modified problem behind side 2 of a what-if comparison solve, kept
    /// so the diagnosis targets the edited model rather than `problem2`.
    pub what_if_problem: Option<Arc<LpProblem>>,
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
        }
    }

    /// Discard any in-flight or completed diagnosis (new solve or overlay closed).
    pub(crate) fn reset_diagnosis(&mut self) {
        self.diagnosis = DiagnosisState::Idle;
        self.receive_diagnosis = None;
    }
}

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

    /// Scroll offset for the detail panel when it has focus.
    pub detail_scroll: u16,

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

    /// Pre-computed diff summary. Built once in `App::new()` since
    /// the report data never changes, avoiding repeated recomputation.
    pub(crate) cached_summary: DiffSummary,

    /// Pre-computed section selector labels, avoiding per-frame `format!` allocations.
    pub(crate) section_labels: [TabLabel; 5],

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
    let kind_counts =
        [(counts.added, "+", t.added), (counts.removed, "-", t.removed), (counts.modified, "~", t.modified), (counts.renamed, ">", t.info)];
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

/// Build pre-computed tab bar labels: list sections carry their entry/change counts.
pub(crate) fn build_section_labels(summary: &DiffSummary, mode: AppMode, filter: DiffFilter) -> [TabLabel; 5] {
    Section::ALL.map(|section| {
        let counts = match section {
            Section::Summary | Section::Numerics => None,
            Section::Variables => Some(&summary.variables),
            Section::Constraints => Some(&summary.constraints),
            Section::Objectives => Some(&summary.objectives),
        };
        match (mode, counts) {
            (_, None) => TabLabel { name: Cow::Borrowed(section.label()), counts: Vec::new() },
            (AppMode::Inspect, Some(counts)) => {
                TabLabel { name: Cow::Owned(format!("{} ({})", section.label(), counts.changed())), counts: Vec::new() }
            }
            (AppMode::Diff, Some(counts)) => TabLabel { name: Cow::Borrowed(section.label()), counts: tab_count_spans(counts, filter) },
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

        // Pre-build summary lines once (report data never changes).
        let report_summary = report.summary();
        let summary_lines = build_mode_summary_lines(mode, &report, &report_summary, &problem1);
        let numerics_lines = build_mode_numerics_lines(mode, &report, &problem1);

        // Pre-compute tab bar labels.
        let section_labels = build_section_labels(&report_summary, mode, DiffFilter::All);

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
            yank: YankState { flash: None, message: String::new() },
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
            cached_summary: report_summary,
            section_labels,
            ignore_order: false,
            sort_mode: SortMode::default(),
            diff_options,
            line_map1,
            line_map2,
            watch: WatchSession::disabled(),
        }
    }

    /// Set the kind filter and rebuild the tab labels that display it.
    /// The single mutation point for `filter`, so the labels can never drift.
    pub(crate) fn apply_filter(&mut self, filter: DiffFilter) {
        self.filter = filter;
        self.section_labels = build_section_labels(&self.cached_summary, self.mode, self.filter);
    }

    /// Flash a transient status-bar message (reuses the yank flash channel).
    pub(crate) fn flash_status(&mut self, message: impl Into<String>) {
        self.yank.message = message.into();
        self.yank.flash = Some(Instant::now());
    }

    /// A diff-only action was pressed in inspect mode: brief no-op hint.
    pub(crate) fn flash_diff_only(&mut self) {
        debug_assert!(matches!(self.mode, AppMode::Inspect), "flash_diff_only is only reachable in inspect mode");
        self.flash_status("Not available in inspect mode (single file)");
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
        self.ignore_order = !self.ignore_order;
        self.invalidate_cache();
        self.rebuild_summary();
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
        self.section_labels = build_section_labels(&summary, self.mode, self.filter);
        self.cached_summary = summary;
    }

    /// Cycle the sidebar sort mode: Name → `AbsDelta` → `RelDelta` → Name.
    pub fn cycle_sort_mode(&mut self) {
        self.sort_mode = self.sort_mode.next();
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.reset_name_list_selection();
        let label = match self.sort_mode {
            SortMode::Name => "Sort: name",
            SortMode::AbsDelta => "Sort: |\u{394}| (largest first)",
            SortMode::RelDelta => "Sort: rel\u{394} (largest first)",
        };
        label.clone_into(&mut self.yank.message);
        self.yank.flash = Some(Instant::now());
    }

    /// Cycle the relative tolerance through the presets and rebuild the diff.
    pub fn cycle_rel_tol(&mut self) {
        let value = next_tolerance_preset(self.diff_options.rel_tol);
        self.diff_options.rel_tol = value;
        self.rebuild_report_inner(false);
        self.yank.message = format!("rel_tol = {}", format_tolerance(value));
        self.yank.flash = Some(Instant::now());
    }

    /// Cycle the absolute tolerance through the presets and rebuild the diff.
    pub fn cycle_abs_tol(&mut self) {
        let value = next_tolerance_preset(self.diff_options.abs_tol);
        self.diff_options.abs_tol = value;
        self.rebuild_report_inner(false);
        self.yank.message = format!("abs_tol = {}", format_tolerance(value));
        self.yank.flash = Some(Instant::now());
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
        }

        // Filtered indices, cached sidebar lines, and coefficient row cache.
        self.invalidate_cache();
        self.detail_scroll = 0;
        self.ensure_active_section_cache();
        self.clamp_active_selection();

        // The search pop-up references haystack indices — refresh if it is open.
        if self.search_popup.visible {
            self.recompute_search_popup();
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

    pub(crate) const fn reset_name_list_selection(&mut self) {
        if let Some(index) = self.active_section.list_index() {
            self.section_states[index].list_state.select(None);
        }
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
                debug_assert!(u16::try_from(n).is_ok(), "page scroll step {n} exceeds u16::MAX");
                #[allow(clippy::cast_possible_truncation)]
                let step = n as u16;
                self.detail_scroll = self.detail_scroll.saturating_add(step).min(self.max_detail_scroll());
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
                debug_assert!(u16::try_from(n).is_ok(), "page scroll step {n} exceeds u16::MAX");
                #[allow(clippy::cast_possible_truncation)]
                let step = n as u16;
                self.detail_scroll = self.detail_scroll.saturating_sub(step);
            }
        }
    }

    /// Copy `text` to the system clipboard and show a flash message in the status bar.
    ///
    /// `label` is a short description shown on success (e.g. "Yanked: x1").
    pub(crate) fn set_yank_flash(&mut self, label: &str, text: &str) {
        thread_local! {
            static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
        }

        let result: Result<(), String> = CLIPBOARD.with_borrow_mut(|cb| {
            if cb.is_none() {
                // Surface initialisation failure (common on SSH/Wayland sessions) instead of
                // silently appearing to succeed; the next yank will retry initialisation.
                *cb = Some(arboard::Clipboard::new().map_err(|error| format!("Clipboard unavailable: {error}"))?);
            }
            cb.as_mut().expect("clipboard initialised above").set_text(text).map_err(|error| format!("Yank failed: {error}"))
        });

        match result {
            Ok(()) => {
                label.clone_into(&mut self.yank.message);
                self.yank.flash = Some(Instant::now());
            }
            Err(message) => {
                self.yank.message = message;
                self.yank.flash = Some(Instant::now());
            }
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
            msg.clone_into(&mut self.yank.message);
            self.yank.flash = Some(Instant::now());
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
    /// (`objectives.csv`, `constraints.csv`, `variables.csv`).
    pub fn export_csv(&mut self) {
        let dir = match std::env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                self.yank.message = format!("CSV export failed: {e}");
                self.yank.flash = Some(Instant::now());
                return;
            }
        };
        let result: Result<String, String> = match self.mode {
            AppMode::Diff => {
                crate::export::write_diff_csv(&self.report, &dir).map(|filename| format!("Wrote {filename}")).map_err(|e| e.to_string())
            }
            AppMode::Inspect => self
                .problem1
                .to_csv(&dir)
                .map(|()| "Wrote objectives.csv, constraints.csv, variables.csv".to_owned())
                .map_err(|e| e.to_string()),
        };
        match result {
            Ok(message) => self.yank.message = message,
            Err(e) => self.yank.message = format!("CSV export failed: {e}"),
        }
        self.yank.flash = Some(Instant::now());
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
    fn selected_entry_name(&self) -> Option<&str> {
        let entry_index = self.selected_entry_index()?;
        self.entry_name(self.active_section, entry_index)
    }

    /// Record the current navigation position in the jumplist.
    pub(crate) fn record_jump(&mut self) {
        let entry_index = self.active_section.list_index().and_then(|index| self.section_states[index].list_state.selected());
        self.jumplist.push(JumpEntry { section: self.active_section, entry_index, detail_scroll: self.detail_scroll, filter: self.filter });
    }

    /// Update active section, keeping the (now invisible) selector state in
    /// sync — it still backs keyboard navigation over the tab bar.
    pub(crate) const fn set_active_section(&mut self, section: Section) {
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));
    }

    /// Step back in the jumplist and restore that position (`Ctrl+o` / palette).
    pub(crate) fn jump_back(&mut self) {
        if let Some(entry) = self.jumplist.go_back() {
            let entry = *entry;
            self.restore_jump(entry);
        }
    }

    /// Step forward in the jumplist and restore that position (`Ctrl+i` / palette).
    pub(crate) fn jump_forward(&mut self) {
        if let Some(entry) = self.jumplist.go_forward() {
            let entry = *entry;
            self.restore_jump(entry);
        }
    }

    /// Navigate to a jumplist entry, restoring section, selection, scroll, and filter.
    pub(crate) fn restore_jump(&mut self, entry: JumpEntry) {
        self.set_active_section(entry.section);
        self.apply_filter(entry.filter);
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.detail_scroll = entry.detail_scroll;

        if let Some(index) = entry.section.list_index() {
            if let Some(selection) = entry.entry_index {
                let len = self.section_states[index].cached_indices().len();
                if selection < len {
                    self.section_states[index].list_state.select(Some(selection));
                } else if len > 0 {
                    self.section_states[index].list_state.select(Some(len - 1));
                } else {
                    self.section_states[index].list_state.select(None);
                }
            } else {
                self.section_states[index].list_state.select(None);
            }
        }

        self.focus =
            if entry.entry_index.is_some() && entry.section.list_index().is_some() { Focus::NameList } else { Focus::SectionSelector };
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
                    self.yank.message = format!("reload failed: {error}");
                    self.yank.flash = Some(Instant::now());
                }
                Err(mpsc::TryRecvError::Empty) => {} // still parsing
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.watch.receive = None;
                    "reload failed: parse thread disconnected".clone_into(&mut self.yank.message);
                    self.yank.flash = Some(Instant::now());
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
            // Receiver may be dropped if the app quit — this is expected.
            if sender.send(outcome).is_err() {
                eprintln!("reload result dropped: receiver closed");
            }
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

        self.solver = SolverSession::new();

        self.yank.message = format!("reloaded {}", chrono::Local::now().format("%H:%M:%S"));
        self.yank.flash = Some(Instant::now());
    }

    /// Whether any time-driven UI is active and needs tick-driven redraws:
    /// a running solve or diagnosis (elapsed-time display), an in-flight
    /// watch reload, or a visible yank flash. Everything else only changes
    /// in response to input, so the main loop skips idle-tick repaints.
    pub const fn is_animating(&self) -> bool {
        self.yank.flash.is_some()
            || self.watch.is_reloading()
            || matches!(self.solver.state, SolveState::Running { .. } | SolveState::RunningBoth { .. })
            || matches!(self.solver.diagnosis, DiagnosisState::Running { .. })
    }

    /// Poll the solver channel(s) for results, transitioning state when complete.
    pub fn poll_solve(&mut self) {
        if matches!(self.solver.state, SolveState::RunningBoth { .. }) {
            self.poll_solve_both();
        } else {
            self.poll_solve_single();
        }
        self.poll_diagnosis();
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
                self.solver.diagnosis = DiagnosisState::Failed("Diagnosis thread disconnected".to_owned());
                self.solver.receive_diagnosis = None;
            }
        }
    }

    /// Inner width of the solve results popup, derived from the last drawn
    /// layout. Mirrors the popup sizing in `widgets::solve` (4/5 of the frame
    /// width, at least 60 columns, minus the borders).
    fn solve_popup_inner_width(&self) -> u16 {
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
                self.solver.state = SolveState::Failed("Solver thread disconnected".to_owned());
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
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Solver thread 1 disconnected".to_owned())),
        });

        // Poll channel 2.
        let got2 = self.solver.receive2.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Solver thread 2 disconnected".to_owned())),
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
        let config = frizbee::Config { sort: true, ..Default::default() };
        let matches = frizbee::match_list_indices(self.search_popup.query.value(), &self.search_name_buffer, &config);

        for matched in matches {
            let haystack_index = matched.index as usize;
            let entry = &self.search_haystack[haystack_index];
            // frizbee returns indices in reverse order; sort ascending for highlighting.
            let mut indices = matched.indices;
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
            self.flash_status("No previous search (press / to search)");
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

    /// Largest useful detail-panel scroll offset: content height minus the
    /// visible window, from the layout recorded on the previous frame. Content
    /// height is stable for a given entry (and scroll resets on entry change),
    /// so last frame's value is the right bound for this frame's input.
    pub(crate) fn max_detail_scroll(&self) -> u16 {
        let visible = self.layout.detail_height.saturating_sub(2) as usize; // borders
        let max = self.layout.detail_content_lines.saturating_sub(visible);
        u16::try_from(max).unwrap_or(u16::MAX)
    }
}
