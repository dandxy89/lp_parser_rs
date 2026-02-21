use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, DiffKind};
use crate::solver::SolveResult;

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
    /// Solve failed with an error message.
    Failed(String),
}

/// Scroll state for the solve results panel.
#[derive(Debug, Default)]
pub struct SolveViewState {
    pub scroll: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Summary,
    Variables,
    Constraints,
    Objectives,
}

impl Section {
    pub const ALL: [Self; 4] = [Self::Summary, Self::Variables, Self::Constraints, Self::Objectives];

    /// Sections with name lists (i.e. everything except Summary).
    pub(crate) const LIST_SECTIONS: [Self; 3] = [Self::Variables, Self::Constraints, Self::Objectives];

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
#[derive(Debug, Clone)]
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
    dirty: bool,
}

impl SectionViewState {
    pub fn new() -> Self {
        Self { list_state: ListState::default(), filtered_indices: Vec::new(), dirty: true }
    }

    /// Mark the cache as stale so it will be recomputed on next access.
    pub(crate) const fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Whether the cache is stale and needs recomputation.
    pub(crate) const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Recompute the filtered indices from the given entries and filter.
    pub(crate) fn recompute<T: DiffEntry>(&mut self, entries: &[T], filter: DiffFilter) {
        debug_assert!(self.dirty, "recompute called on non-dirty SectionViewState");
        self.filtered_indices.clear();
        self.filtered_indices.extend(entries.iter().enumerate().filter(|(_, e)| filter.matches(e.kind())).map(|(i, _)| i));
        self.dirty = false;
    }

    /// Return the cached filtered indices.
    /// Caller must ensure the cache is not dirty (call `ensure_active_section_cache` first).
    pub fn cached_indices(&self) -> &[usize] {
        debug_assert!(!self.dirty, "cached_indices called on dirty SectionViewState");
        &self.filtered_indices
    }

    /// Return both the cached indices and a mutable reference to the list state.
    /// This avoids borrow-checker conflicts when drawing (need indices for items
    /// and &mut `ListState` for `render_stateful_widget`).
    pub fn indices_and_state_mut(&mut self) -> (&[usize], &mut ListState) {
        debug_assert!(!self.dirty, "indices_and_state_mut called on dirty SectionViewState");
        (&self.filtered_indices, &mut self.list_state)
    }
}
