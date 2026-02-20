use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, DiffKind};
use crate::search::CompiledSearch;

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

    /// Recompute the filtered indices from the given entries/filter/search.
    pub(crate) fn recompute<T: DiffEntry>(&mut self, entries: &[T], filter: DiffFilter, compiled: &CompiledSearch, query_empty: bool) {
        debug_assert!(self.dirty, "recompute called on non-dirty SectionViewState");
        self.filtered_indices.clear();
        self.filtered_indices.extend(
            entries
                .iter()
                .enumerate()
                .filter(|(_, e)| filter.matches(e.kind()) && (query_empty || compiled.matches(e.searchable_text())))
                .map(|(i, _)| i),
        );
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
