use std::time::Instant;

use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, DiffKind, sort_indices_by_delta};
use crate::solver::{InfeasibilityDiagnosis, SolveDiffResult, SolveResult};
use crate::widgets::{delta_column, kind_prefix, kind_style, text};

/// State machine for the LP solver overlay.
#[derive(Debug)]
pub enum SolveState {
    /// No solver activity.
    Idle,
    /// Showing the file picker popup (choose file 1 or 2).
    Picking,
    /// Solver is running in a background thread. `started` is recorded at launch
    /// so the overlay can show elapsed time during long solves.
    Running { file: String, started: Instant },
    /// Solve completed successfully.
    Done(Box<SolveResult>),
    /// Both solvers running in parallel; optional results filled as they complete.
    RunningBoth { file1: String, file2: String, result1: Option<Box<SolveResult>>, result2: Option<Box<SolveResult>>, started: Instant },
    /// Both solves completed; showing comparison view.
    DoneBoth(Box<SolveDiffResult>),
    /// Solve failed with an error message.
    Failed(String),
}

/// State for the what-if prompt overlay (`E` on a selected constraint):
/// edit the constraint's RHS in memory and re-solve against the baseline.
#[derive(Debug, Clone)]
pub struct WhatIfPrompt {
    /// Name of the constraint being edited.
    pub constraint_name: String,
    /// The constraint's current RHS in the baseline problem (file 1).
    pub current_rhs: f64,
    /// Editable buffer for the new RHS value being typed.
    pub input: tui_input::Input,
    /// Validation error from the last confirm attempt, if any.
    pub error: Option<String>,
}

/// State machine for the infeasibility diagnosis (elastic relaxation) run.
#[derive(Debug)]
pub enum DiagnosisState {
    /// No diagnosis requested.
    Idle,
    /// Elastic relaxation running in a background thread. `started` is recorded
    /// at launch so the summary block can show elapsed time during long runs.
    Running { file: String, started: Instant },
    /// Diagnosis completed.
    Done { file: String, diagnosis: Box<InfeasibilityDiagnosis> },
    /// Diagnosis failed with an error message.
    Failed(String),
}

/// Active tab in the solve results popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SolveTab {
    #[default]
    Summary,
    Variables,
    Constraints,
    Log,
    Duals,
}

impl SolveTab {
    pub const ALL: [Self; 5] = [Self::Summary, Self::Variables, Self::Constraints, Self::Log, Self::Duals];

    pub const fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Variables => 1,
            Self::Constraints => 2,
            Self::Log => 3,
            Self::Duals => 4,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Variables => "Variables",
            Self::Constraints => "Constraints",
            Self::Log => "Log",
            Self::Duals => "Duals",
        }
    }

    /// Cycle to the next tab, wrapping around.
    pub const fn next(self) -> Self {
        match self {
            Self::Summary => Self::Variables,
            Self::Variables => Self::Constraints,
            Self::Constraints => Self::Log,
            Self::Log => Self::Duals,
            Self::Duals => Self::Summary,
        }
    }

    /// Cycle to the previous tab, wrapping around.
    pub const fn prev(self) -> Self {
        match self {
            Self::Summary => Self::Duals,
            Self::Variables => Self::Summary,
            Self::Constraints => Self::Variables,
            Self::Log => Self::Constraints,
            Self::Duals => Self::Log,
        }
    }
}

/// Preset delta thresholds for cycling with `t`/`T` in the diff view.
pub const DELTA_THRESHOLDS: [f64; 6] = [0.0, 0.0001, 0.001, 0.01, 0.1, 1.0];

/// Default index into `DELTA_THRESHOLDS` (0.0001).
const DEFAULT_THRESHOLD_INDEX: usize = 1;

/// Scroll state for the solve results panel.
#[derive(Debug)]
pub struct SolveViewState {
    pub tab: SolveTab,
    /// Per-tab scroll offsets, indexed by `SolveTab::index()`.
    pub scroll: [u16; 5],
    /// When `true`, the Variables and Constraints tabs in diff view show only changed rows.
    pub diff_only: bool,
    /// Delta threshold for filtering insignificant differences in the diff view.
    pub delta_threshold: f64,
    /// Current index into `DELTA_THRESHOLDS` for cycling.
    pub threshold_index: usize,
}

impl Default for SolveViewState {
    fn default() -> Self {
        Self {
            tab: SolveTab::default(),
            scroll: [0; 5],
            diff_only: false,
            delta_threshold: DELTA_THRESHOLDS[DEFAULT_THRESHOLD_INDEX],
            threshold_index: DEFAULT_THRESHOLD_INDEX,
        }
    }
}

impl SolveViewState {
    /// Cycle to the next preset threshold, wrapping around.
    pub const fn cycle_threshold_forward(&mut self) {
        self.threshold_index = (self.threshold_index + 1) % DELTA_THRESHOLDS.len();
        self.delta_threshold = DELTA_THRESHOLDS[self.threshold_index];
    }

    /// Cycle to the previous preset threshold, wrapping around.
    pub const fn cycle_threshold_backward(&mut self) {
        self.threshold_index = if self.threshold_index == 0 { DELTA_THRESHOLDS.len() - 1 } else { self.threshold_index - 1 };
        self.delta_threshold = DELTA_THRESHOLDS[self.threshold_index];
    }
}

/// A single result from the search pop-up, spanning all sections.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// Which section this entry belongs to.
    pub section: Section,
    /// Index into the section's `entries` vec.
    pub entry_index: usize,
    /// Fuzzy match score (0 for regex/substring modes).
    pub score: u16,
    /// Character positions in the name that matched (for highlighting).
    pub match_indices: Vec<usize>,
    /// Index into the pre-built `search_haystack` for name/kind resolution.
    pub haystack_index: usize,
    /// Diff kind for badge rendering.
    pub kind: DiffKind,
}

/// Which top-level mode the viewer is running in.
///
/// Selected once at startup from the number of positional file arguments and
/// never changes afterwards: one file → [`AppMode::Inspect`] (a single-model
/// explorer), two files → [`AppMode::Diff`] (the original diff viewer). An
/// explicit enum keeps mode-specific behaviour readable instead of scattered
/// `Option`/count checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Comparing two files: the original diff experience, unchanged.
    Diff,
    /// Exploring a single file: sections list every entry with no diff badges.
    Inspect,
}

impl AppMode {
    /// Whether diff-badges/kinds should be shown in the sidebar and detail panel.
    /// Inspect mode lists plain entries with no `[+]/[-]/[~]` prefixes or colours.
    pub const fn shows_diff_badges(self) -> bool {
        matches!(self, Self::Diff)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Summary,
    Variables,
    Constraints,
    Objectives,
    /// Per-file numerical conditioning view (scaling, ranges, issues).
    /// Static like Summary: no name list, content rendered from cached lines.
    Numerics,
}

impl Section {
    pub const ALL: [Self; 5] = [Self::Summary, Self::Variables, Self::Constraints, Self::Objectives, Self::Numerics];

    pub const fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Variables => 1,
            Self::Constraints => 2,
            Self::Objectives => 3,
            Self::Numerics => 4,
        }
    }

    pub fn from_index(i: usize) -> Self {
        debug_assert!(i < Self::ALL.len(), "Section::from_index called with out-of-range index {i}");
        match i {
            0 => Self::Summary,
            1 => Self::Variables,
            2 => Self::Constraints,
            3 => Self::Objectives,
            4 => Self::Numerics,
            _ => unreachable!("Section::from_index called with out-of-range index {i}"),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Variables => "Variables",
            Self::Constraints => "Constraints",
            Self::Objectives => "Objectives",
            Self::Numerics => "Numerics",
        }
    }

    /// Index into the `section_states` array (0-based, static sections excluded).
    /// Returns `None` for Summary and Numerics, which have no entry list.
    pub(crate) const fn list_index(self) -> Option<usize> {
        match self {
            Self::Summary | Self::Numerics => None,
            Self::Variables => Some(0),
            Self::Constraints => Some(1),
            Self::Objectives => Some(2),
        }
    }
}

/// Which panel currently has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The top-left section selector (4 items).
    SectionSelector,
    /// The name list below the section selector.
    NameList,
    /// The right-hand detail panel.
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFilter {
    All,
    Added,
    Removed,
    Modified,
    Renamed,
}

impl DiffFilter {
    pub fn matches(self, kind: DiffKind) -> bool {
        match self {
            Self::All => true,
            Self::Added => kind == DiffKind::Added,
            Self::Removed => kind == DiffKind::Removed,
            Self::Modified => kind == DiffKind::Modified,
            Self::Renamed => kind == DiffKind::Renamed,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Added => "Added",
            Self::Removed => "Removed",
            Self::Modified => "Modified",
            Self::Renamed => "Renamed",
        }
    }
}

/// Sort order for the sidebar name lists, cycled with `s` in normal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// Alphabetical by entry name (the report's natural order).
    #[default]
    Name,
    /// Descending absolute delta `|new - old|`; no-delta entries follow alphabetically.
    AbsDelta,
    /// Descending relative delta `|new - old| / max(|new|, |old|)`.
    RelDelta,
}

impl SortMode {
    /// Cycle to the next sort mode: Name → `AbsDelta` → `RelDelta` → Name.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Name => Self::AbsDelta,
            Self::AbsDelta => Self::RelDelta,
            Self::RelDelta => Self::Name,
        }
    }

    /// Short status-bar label. `None` for the default name sort (nothing to show).
    #[must_use]
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Name => None,
            Self::AbsDelta => Some("sort:|\u{394}|"),
            Self::RelDelta => Some("sort:rel\u{394}"),
        }
    }
}

/// Maximum number of entries in the jumplist before oldest entries are dropped.
const JUMPLIST_CAPACITY: usize = 100;

/// A recorded navigation position in the jumplist.
#[derive(Debug, Clone, Copy)]
pub struct JumpEntry {
    pub section: Section,
    pub entry_index: Option<usize>,
    pub detail_scroll: u16,
    pub filter: DiffFilter,
}

/// A vi-style jumplist for recording and navigating navigation positions.
///
/// Entries are appended when the user changes section, confirms a search selection,
/// or changes filter. `Ctrl+o` goes back, `Ctrl+i` goes forward.
#[derive(Debug)]
pub struct JumpList {
    entries: Vec<JumpEntry>,
    /// Points to the current position in the jumplist.
    /// When navigating back, cursor decreases; forward, it increases.
    cursor: usize,
}

impl JumpList {
    pub const fn new() -> Self {
        Self { entries: Vec::new(), cursor: 0 }
    }

    /// Push a new jump entry, discarding any forward history.
    pub fn push(&mut self, entry: JumpEntry) {
        debug_assert!(self.cursor <= self.entries.len(), "jumplist cursor {} exceeds entries len {}", self.cursor, self.entries.len());
        // Truncate forward history.
        self.entries.truncate(self.cursor);
        self.entries.push(entry);

        // Drop oldest if over capacity.
        if self.entries.len() > JUMPLIST_CAPACITY {
            let excess = self.entries.len() - JUMPLIST_CAPACITY;
            self.entries.drain(..excess);
        }

        self.cursor = self.entries.len();
    }

    /// Move cursor back and return the entry to restore, if any.
    pub fn go_back(&mut self) -> Option<&JumpEntry> {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.entries.get(self.cursor)
        } else {
            None
        }
    }

    /// Move cursor forward and return the entry to restore, if any.
    pub fn go_forward(&mut self) -> Option<&JumpEntry> {
        if self.cursor < self.entries.len() {
            let entry = self.entries.get(self.cursor);
            self.cursor += 1;
            debug_assert!(
                self.cursor <= self.entries.len(),
                "jumplist cursor {} exceeds entries len {} after go_forward",
                self.cursor,
                self.entries.len(),
            );
            entry
        } else {
            None
        }
    }
}

/// Per-section view state: list selection and cached filtered indices.
#[derive(Debug)]
pub struct SectionViewState {
    pub list_state: ListState,
    filtered_indices: Vec<usize>,
    /// Pre-built `Line<'static>` per filtered entry, used by the sidebar to
    /// avoid rebuilding `Vec<ListItem>` every frame.  Built in `recompute()`.
    cached_lines: Vec<Line<'static>>,
    dirty: bool,
}

impl SectionViewState {
    pub fn new() -> Self {
        Self { list_state: ListState::default(), filtered_indices: Vec::new(), cached_lines: Vec::new(), dirty: true }
    }

    /// Mark the cache as stale so it will be recomputed on next access.
    pub(crate) const fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Whether the cache is stale and needs recomputation.
    pub(crate) const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Recompute the filtered indices and cached sidebar lines from the given
    /// entries, filter, and sort mode.
    ///
    /// `show_badges` gates the `[+]/[-]/[~]` diff prefix and its colour: diff mode
    /// passes `true`; inspect mode passes `false` so entries render as plain,
    /// neutrally-coloured names with no diff decoration.
    pub(crate) fn recompute<T: DiffEntry>(
        &mut self,
        entries: &[T],
        filter: DiffFilter,
        ignore_order: bool,
        sort: SortMode,
        show_badges: bool,
    ) {
        // Width of the delta column shown under the delta sorts ("1.00e-12" is 8).
        const DELTA_WIDTH: usize = 8;
        debug_assert!(self.dirty, "recompute called on non-dirty SectionViewState");
        self.filtered_indices.clear();
        self.cached_lines.clear();
        for (i, entry) in entries.iter().enumerate() {
            if ignore_order && entry.is_order_only() {
                continue;
            }
            if filter.matches(entry.kind()) {
                self.filtered_indices.push(i);
            }
        }
        let relative = match sort {
            SortMode::Name => None, // entries are already name-sorted in the report
            SortMode::AbsDelta => Some(false),
            SortMode::RelDelta => Some(true),
        };
        if let Some(relative) = relative {
            sort_indices_by_delta(entries, &mut self.filtered_indices, relative);
        }
        for &i in &self.filtered_indices {
            let entry = &entries[i];
            let line = if show_badges {
                let kind = entry.kind();
                let style = kind_style(kind);
                let mut spans = vec![Span::styled(kind_prefix(kind), style), Span::raw(" ")];
                // Under a delta sort, surface the magnitude driving the order as
                // a fixed-width column so the ranking is readable at a glance.
                if let Some(relative) = relative {
                    let delta = match entry.sort_delta(relative) {
                        Some(delta) => format!("{delta:>DELTA_WIDTH$.2e} "),
                        None => format!("{:>DELTA_WIDTH$} ", ""),
                    };
                    spans.push(Span::styled(delta, delta_column()));
                }
                spans.push(Span::styled(entry.name().to_owned(), style));
                Line::from(spans)
            } else {
                // Inspect mode: plain name, no diff badge, default text colour.
                Line::from(vec![Span::raw("  "), Span::styled(entry.name().to_owned(), text())])
            };
            self.cached_lines.push(line);
        }
        debug_assert_eq!(self.filtered_indices.len(), self.cached_lines.len(), "filtered indices and cached lines must be in sync");
        self.dirty = false;
    }

    /// Return the cached filtered indices.
    /// Caller must ensure the cache is not dirty (call `ensure_active_section_cache` first).
    pub fn cached_indices(&self) -> &[usize] {
        debug_assert!(!self.dirty, "cached_indices called on dirty SectionViewState");
        &self.filtered_indices
    }

    /// Return cached indices, cached sidebar lines, and a mutable list state.
    pub fn indices_lines_and_state_mut(&mut self) -> (&[usize], &[Line<'static>], &mut ListState) {
        debug_assert!(!self.dirty, "indices_lines_and_state_mut called on dirty SectionViewState");
        (&self.filtered_indices, &self.cached_lines, &mut self.list_state)
    }
}

/// Which view to show in the detail panel for constraints and objectives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailView {
    /// Parsed (structured) diff view — the default.
    #[default]
    Parsed,
    /// Raw file text side-by-side view.
    Raw,
}

/// Pending yank state for multi-key chords (`yo`, `yn`, `yy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingYank {
    /// No pending yank operation.
    None,
    /// `y` was pressed; waiting for target key (`o`, `n`, or `y`).
    WaitingForTarget,
}

/// Which side of a diff entry to operate on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The old (file 1) version.
    Old,
    /// The new (file 2) version.
    New,
}

/// A command exposed in the `Ctrl+P` command palette.
///
/// Every entry maps onto an action that already has a direct keybinding; the
/// palette is a discoverable, fuzzy-searchable front-end to the same handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    GoSummary,
    GoVariables,
    GoConstraints,
    GoObjectives,
    GoNumerics,
    FilterAll,
    FilterAdded,
    FilterRemoved,
    FilterModified,
    FilterRenamed,
    ToggleRawView,
    ToggleIgnoreOrder,
    CycleSort,
    CycleRelTol,
    CycleAbsTol,
    OpenSearch,
    NextMatch,
    PrevMatch,
    JumpBack,
    JumpForward,
    Solve,
    WhatIf,
    ExportCsv,
    YankName,
    YankOld,
    YankNew,
    YankDetail,
    ShowHelp,
    Quit,
}

impl PaletteCommand {
    /// Every command, in display order.
    pub const ALL: [Self; 29] = [
        Self::GoSummary,
        Self::GoVariables,
        Self::GoConstraints,
        Self::GoObjectives,
        Self::GoNumerics,
        Self::FilterAll,
        Self::FilterAdded,
        Self::FilterRemoved,
        Self::FilterModified,
        Self::FilterRenamed,
        Self::ToggleRawView,
        Self::ToggleIgnoreOrder,
        Self::CycleSort,
        Self::CycleRelTol,
        Self::CycleAbsTol,
        Self::OpenSearch,
        Self::NextMatch,
        Self::PrevMatch,
        Self::JumpBack,
        Self::JumpForward,
        Self::Solve,
        Self::WhatIf,
        Self::ExportCsv,
        Self::YankName,
        Self::YankOld,
        Self::YankNew,
        Self::YankDetail,
        Self::ShowHelp,
        Self::Quit,
    ];

    /// Human-readable label shown in the palette and matched against the query.
    pub const fn label(self) -> &'static str {
        match self {
            Self::GoSummary => "Go to Summary",
            Self::GoVariables => "Go to Variables",
            Self::GoConstraints => "Go to Constraints",
            Self::GoObjectives => "Go to Objectives",
            Self::GoNumerics => "Go to Numerics",
            Self::FilterAll => "Filter: All",
            Self::FilterAdded => "Filter: Added",
            Self::FilterRemoved => "Filter: Removed",
            Self::FilterModified => "Filter: Modified",
            Self::FilterRenamed => "Filter: Renamed",
            Self::ToggleRawView => "Toggle raw text view",
            Self::ToggleIgnoreOrder => "Toggle ignore coefficient order",
            Self::CycleSort => "Cycle sort mode",
            Self::CycleRelTol => "Cycle relative tolerance",
            Self::CycleAbsTol => "Cycle absolute tolerance",
            Self::OpenSearch => "Search entries",
            Self::NextMatch => "Next search match",
            Self::PrevMatch => "Previous search match",
            Self::JumpBack => "Jump back (jumplist)",
            Self::JumpForward => "Jump forward (jumplist)",
            Self::Solve => "Solve problem (HiGHS)",
            Self::WhatIf => "What-if: edit constraint RHS & re-solve",
            Self::ExportCsv => "Export diff to CSV",
            Self::YankName => "Yank entry name",
            Self::YankOld => "Yank old side (file 1)",
            Self::YankNew => "Yank new side (file 2)",
            Self::YankDetail => "Yank detail panel",
            Self::ShowHelp => "Show help",
            Self::Quit => "Quit",
        }
    }

    /// Whether this command is offered in inspect (single-file) mode.
    ///
    /// Diff-only actions — kind filters, ignore-order, tolerance cycling, the raw
    /// side-by-side view, delta sorts, and the per-side (file 1 / file 2) yanks —
    /// are hidden from the palette in inspect mode (they also no-op with a status
    /// hint if their direct key is pressed). Everything else, including search,
    /// solve, CSV export, name/detail yank, help and quit, remains.
    pub const fn available_in_inspect(self) -> bool {
        !matches!(
            self,
            Self::FilterAll
                | Self::FilterAdded
                | Self::FilterRemoved
                | Self::FilterModified
                | Self::FilterRenamed
                | Self::ToggleRawView
                | Self::ToggleIgnoreOrder
                | Self::CycleSort
                | Self::CycleRelTol
                | Self::CycleAbsTol
                | Self::YankOld
                | Self::YankNew
        )
    }

    /// The equivalent direct keybinding, shown right-aligned in the palette.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::GoSummary => "1",
            Self::GoVariables => "2",
            Self::GoConstraints => "3",
            Self::GoObjectives => "4",
            Self::GoNumerics => "5",
            Self::FilterAll => "a",
            Self::FilterAdded => "+",
            Self::FilterRemoved => "-",
            Self::FilterModified => "m",
            Self::FilterRenamed => "=",
            Self::ToggleRawView => "r",
            Self::ToggleIgnoreOrder => "o",
            Self::CycleSort => "s",
            Self::CycleRelTol => "t",
            Self::CycleAbsTol => "T",
            Self::OpenSearch => "/",
            Self::NextMatch => "n",
            Self::PrevMatch => "N",
            Self::JumpBack => "^o",
            Self::JumpForward => "^i",
            Self::Solve => "S",
            Self::WhatIf => "E",
            Self::ExportCsv => "w",
            Self::YankName => "yy",
            Self::YankOld => "yo",
            Self::YankNew => "yn",
            Self::YankDetail => "Y",
            Self::ShowHelp => "?",
            Self::Quit => "q",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_mode_shows_badges_inspect_does_not() {
        assert!(AppMode::Diff.shows_diff_badges(), "diff mode decorates entries with kind badges");
        assert!(!AppMode::Inspect.shows_diff_badges(), "inspect mode lists plain entries");
    }

    #[test]
    fn test_diff_only_palette_commands_hidden_in_inspect() {
        // Diff-only actions: kind filters, ignore-order, tolerance, raw view, delta sorts.
        let disabled = [
            PaletteCommand::FilterAll,
            PaletteCommand::FilterAdded,
            PaletteCommand::FilterRemoved,
            PaletteCommand::FilterModified,
            PaletteCommand::FilterRenamed,
            PaletteCommand::ToggleRawView,
            PaletteCommand::ToggleIgnoreOrder,
            PaletteCommand::CycleSort,
            PaletteCommand::CycleRelTol,
            PaletteCommand::CycleAbsTol,
            PaletteCommand::YankOld,
            PaletteCommand::YankNew,
        ];
        for command in disabled {
            assert!(!command.available_in_inspect(), "{command:?} must be hidden in inspect mode");
        }
    }

    #[test]
    fn test_core_palette_commands_available_in_inspect() {
        // Everything that still does something in a single-file view stays offered.
        let available = [
            PaletteCommand::GoSummary,
            PaletteCommand::GoVariables,
            PaletteCommand::GoConstraints,
            PaletteCommand::GoObjectives,
            PaletteCommand::GoNumerics,
            PaletteCommand::OpenSearch,
            PaletteCommand::NextMatch,
            PaletteCommand::PrevMatch,
            PaletteCommand::JumpBack,
            PaletteCommand::JumpForward,
            PaletteCommand::Solve,
            PaletteCommand::WhatIf,
            PaletteCommand::ExportCsv,
            PaletteCommand::YankName,
            PaletteCommand::YankDetail,
            PaletteCommand::ShowHelp,
            PaletteCommand::Quit,
        ];
        for command in available {
            assert!(command.available_in_inspect(), "{command:?} must remain available in inspect mode");
        }
    }
}
