use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;
use smallvec::SmallVec;

use crate::diff_model::{DiffEntry, DiffKind};
use crate::solver::{SolveDiffResult, SolveResult};
use crate::widgets::{kind_prefix, kind_style};

/// State machine for the LP solver overlay.
#[derive(Debug)]
pub enum SolveState {
    /// No solver activity.
    Idle,
    /// Showing the file picker popup (choose file 1 or 2).
    Picking,
    /// Solver is running in a background thread.
    Running { file: String },
    /// Solve completed successfully.
    Done(Box<SolveResult>),
    /// Both solvers running in parallel; optional results filled as they complete.
    RunningBoth { file1: String, file2: String, result1: Option<Box<SolveResult>>, result2: Option<Box<SolveResult>> },
    /// Both solves completed; showing comparison view.
    DoneBoth(Box<SolveDiffResult>),
    /// Solve failed with an error message.
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
}

impl SolveTab {
    pub const ALL: [Self; 4] = [Self::Summary, Self::Variables, Self::Constraints, Self::Log];

    pub const fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Variables => 1,
            Self::Constraints => 2,
            Self::Log => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Variables => "Variables",
            Self::Constraints => "Constraints",
            Self::Log => "Log",
        }
    }

    /// Cycle to the next tab, wrapping around.
    pub const fn next(self) -> Self {
        match self {
            Self::Summary => Self::Variables,
            Self::Variables => Self::Constraints,
            Self::Constraints => Self::Log,
            Self::Log => Self::Summary,
        }
    }

    /// Cycle to the previous tab, wrapping around.
    pub const fn prev(self) -> Self {
        match self {
            Self::Summary => Self::Log,
            Self::Variables => Self::Summary,
            Self::Constraints => Self::Variables,
            Self::Log => Self::Constraints,
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
    pub scroll: [u16; 4],
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
            scroll: [0; 4],
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
    pub match_indices: SmallVec<[usize; 8]>,
    /// Index into the pre-built `search_haystack` for name/kind resolution.
    pub haystack_index: usize,
    /// Diff kind for badge rendering.
    pub kind: DiffKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Summary,
    Variables,
    Constraints,
    Objectives,
}

impl Section {
    pub const ALL: [Self; 4] = [Self::Summary, Self::Variables, Self::Constraints, Self::Objectives];

    pub const fn index(self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Variables => 1,
            Self::Constraints => 2,
            Self::Objectives => 3,
        }
    }

    pub fn from_index(i: usize) -> Self {
        debug_assert!(i < Self::ALL.len(), "Section::from_index called with out-of-range index {i}");
        match i {
            0 => Self::Summary,
            1 => Self::Variables,
            2 => Self::Constraints,
            3 => Self::Objectives,
            _ => unreachable!("Section::from_index called with out-of-range index {i}"),
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Variables => "Variables",
            Self::Constraints => "Constraints",
            Self::Objectives => "Objectives",
        }
    }

    /// Index into the `section_states` array (0-based, Summary excluded).
    /// Returns `None` for Summary.
    pub(crate) const fn list_index(self) -> Option<usize> {
        match self {
            Self::Summary => None,
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
}

impl DiffFilter {
    pub fn matches(self, kind: DiffKind) -> bool {
        match self {
            Self::All => true,
            Self::Added => kind == DiffKind::Added,
            Self::Removed => kind == DiffKind::Removed,
            Self::Modified => kind == DiffKind::Modified,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Added => "Added",
            Self::Removed => "Removed",
            Self::Modified => "Modified",
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

    /// Recompute the filtered indices and cached sidebar lines from the given entries and filter.
    pub(crate) fn recompute<T: DiffEntry>(&mut self, entries: &[T], filter: DiffFilter) {
        debug_assert!(self.dirty, "recompute called on non-dirty SectionViewState");
        self.filtered_indices.clear();
        self.cached_lines.clear();
        for (i, entry) in entries.iter().enumerate() {
            if filter.matches(entry.kind()) {
                self.filtered_indices.push(i);
                let kind = entry.kind();
                let style = kind_style(kind);
                let line =
                    Line::from(vec![Span::styled(kind_prefix(kind), style), Span::raw(" "), Span::styled(entry.name().to_owned(), style)]);
                self.cached_lines.push(line);
            }
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
    /// Raw LP file text side-by-side view.
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
