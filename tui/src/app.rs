use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, DiffKind, LpDiffReport};
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
    const LIST_SECTIONS: [Self; 3] = [Self::Variables, Self::Constraints, Self::Objectives];

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
    const fn list_index(self) -> Option<usize> {
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
    fn new() -> Self {
        Self { list_state: ListState::default(), filtered_indices: Vec::new(), dirty: true }
    }

    /// Mark the cache as stale so it will be recomputed on next access.
    const fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Recompute the filtered indices from the given entries/filter/search.
    fn recompute<T: DiffEntry>(&mut self, entries: &[T], filter: DiffFilter, compiled: &CompiledSearch, query_empty: bool) {
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
    /// and &mut ListState for `render_stateful_widget`).
    pub fn indices_and_state_mut(&mut self) -> (&[usize], &mut ListState) {
        debug_assert!(!self.dirty, "indices_and_state_mut called on dirty SectionViewState");
        (&self.filtered_indices, &mut self.list_state)
    }
}

pub struct App {
    pub report: LpDiffReport,
    pub active_section: Section,
    pub focus: Focus,
    pub filter: DiffFilter,
    pub should_quit: bool,

    /// Whether the help popup overlay is visible.
    pub show_help: bool,

    /// Whether the search bar is currently accepting input.
    pub search_active: bool,
    pub search_query: String,

    /// Scroll offset for the detail panel when it has focus.
    pub detail_scroll: u16,

    /// Section selector list state (tracks which of the 4 sections is highlighted).
    pub section_selector_state: ListState,

    /// Per-section view states: [Variables, Constraints, Objectives].
    pub section_states: [SectionViewState; 3],

    /// Compiled search, built lazily once per query change (shared across sections).
    compiled_search: Option<CompiledSearch>,
}

impl App {
    pub fn new(report: LpDiffReport) -> Self {
        let mut section_selector_state = ListState::default();
        section_selector_state.select(Some(0));

        Self {
            report,
            active_section: Section::Summary,
            focus: Focus::SectionSelector,
            filter: DiffFilter::All,
            should_quit: false,
            show_help: false,
            search_active: false,
            search_query: String::new(),
            detail_scroll: 0,
            section_selector_state,
            section_states: [SectionViewState::new(), SectionViewState::new(), SectionViewState::new()],
            compiled_search: None,
        }
    }

    /// Invalidate cached filtered indices for all sections.
    fn invalidate_cache(&mut self) {
        for state in &mut self.section_states {
            state.invalidate();
        }
        self.compiled_search = None;
    }

    /// Ensure the compiled search is built and return a reference to it.
    fn ensure_compiled_search(&mut self) -> &CompiledSearch {
        if self.compiled_search.is_none() {
            self.compiled_search = Some(CompiledSearch::compile(&self.search_query));
        }
        self.compiled_search.as_ref().expect("just populated")
    }

    /// Whether the current search query has a regex error.
    pub fn has_search_regex_error(&mut self) -> bool {
        if self.search_query.is_empty() {
            return false;
        }
        self.ensure_compiled_search().has_regex_error()
    }

    /// Ensure the active section's cache is fresh. Call once per frame before drawing.
    pub fn ensure_active_section_cache(&mut self) {
        let Some(idx) = self.active_section.list_index() else {
            return;
        };
        if !self.section_states[idx].dirty {
            return;
        }

        // Build compiled search if needed.
        if self.compiled_search.is_none() {
            self.compiled_search = Some(CompiledSearch::compile(&self.search_query));
        }
        let compiled = self.compiled_search.as_ref().expect("just populated");
        let query_empty = self.search_query.is_empty();
        let filter = self.filter;

        let section = Section::LIST_SECTIONS[idx];
        match section {
            Section::Variables => {
                self.section_states[idx].recompute(&self.report.variables.entries, filter, compiled, query_empty);
            }
            Section::Constraints => {
                self.section_states[idx].recompute(&self.report.constraints.entries, filter, compiled, query_empty);
            }
            Section::Objectives => {
                self.section_states[idx].recompute(&self.report.objectives.entries, filter, compiled, query_empty);
            }
            Section::Summary => unreachable!("Summary has no list_index"),
        }
    }

    /// Return the number of items in the name list for the current section.
    /// Must be called after `ensure_active_section_cache()`.
    pub fn name_list_len(&self) -> usize {
        match self.active_section.list_index() {
            Some(idx) => self.section_states[idx].cached_indices().len(),
            None => 0,
        }
    }

    /// Total number of entries visible in the current filtered view (used for status bar).
    /// Must be called after `ensure_active_section_cache()`.
    pub fn current_filter_count(&self) -> usize {
        self.name_list_len()
    }

    /// Return a mutable reference to the `ListState` for the active section's name list.
    pub const fn active_name_list_state_mut(&mut self) -> &mut ListState {
        match self.active_section.list_index() {
            Some(idx) => &mut self.section_states[idx].list_state,
            None => &mut self.section_selector_state,
        }
    }

    /// Whether the name list panel has selectable content for the current section.
    fn has_name_list(&self) -> bool {
        self.active_section != Section::Summary && self.name_list_len() > 0
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C is an unconditional quit regardless of any other mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.show_help {
            // Any key dismisses the help popup.
            self.show_help = false;
            return;
        }

        if self.search_active {
            self.handle_search_key(key);
            return;
        }

        self.handle_normal_key(key);
    }

    /// Handle a key event while the search bar is active.
    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_active = false;
                self.search_query.clear();
                self.invalidate_cache();
            }
            KeyCode::Enter => {
                // Commit the query as the active filter and return to normal mode.
                self.search_active = false;
                self.invalidate_cache();
                self.ensure_active_section_cache();
                self.reset_name_list_selection();
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.invalidate_cache();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.invalidate_cache();
            }
            _ => {}
        }
    }

    /// Handle a key event in normal (non-search) mode.
    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,

            // Tab cycles focus forward through panels; BackTab cycles backward.
            KeyCode::Tab => self.cycle_focus_forward(),
            KeyCode::BackTab => self.cycle_focus_backward(),

            // Direct section jump (always focuses the section selector).
            KeyCode::Char('1') => self.set_section(Section::Summary),
            KeyCode::Char('2') => self.set_section(Section::Variables),
            KeyCode::Char('3') => self.set_section(Section::Constraints),
            KeyCode::Char('4') => self.set_section(Section::Objectives),

            // Navigation (vi-style and arrow keys) — behaviour depends on focused panel.
            KeyCode::Char('j') | KeyCode::Down => self.navigate_down(),
            KeyCode::Char('k') | KeyCode::Up => self.navigate_up(),
            KeyCode::Char('g') | KeyCode::Home => self.jump_to_top(),
            KeyCode::Char('G') | KeyCode::End => self.jump_to_bottom(),

            // n/N for next/previous within a list.
            KeyCode::Char('n') => self.navigate_down(),
            KeyCode::Char('N') => self.navigate_up(),

            // Enter moves focus to the detail panel.
            KeyCode::Enter => self.handle_enter(),

            // Escape returns from detail → sidebar, or clears search.
            KeyCode::Esc => self.handle_escape(),

            // h/l as alternative focus movement (left/right between sidebar and detail).
            KeyCode::Char('l') => {
                if self.focus != Focus::Detail {
                    self.focus = Focus::Detail;
                    self.detail_scroll = 0;
                }
            }
            KeyCode::Char('h') => {
                if self.focus == Focus::Detail {
                    // Return to whichever sidebar panel was last active.
                    if self.has_name_list() && self.active_name_list_state_mut().selected().is_some() {
                        self.focus = Focus::NameList;
                    } else {
                        self.focus = Focus::SectionSelector;
                    }
                }
            }

            // Filter shortcuts.
            KeyCode::Char('a') => self.set_filter(DiffFilter::All),
            KeyCode::Char('+') => self.set_filter(DiffFilter::Added),
            KeyCode::Char('-') => self.set_filter(DiffFilter::Removed),
            KeyCode::Char('m') => self.set_filter(DiffFilter::Modified),

            // Toggle help popup.
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
            }

            // Open the search bar.
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query.clear();
                self.invalidate_cache();
            }

            _ => {}
        }
    }

    /// Cycle focus: SectionSelector → NameList → Detail → SectionSelector.
    /// Skips NameList when the current section has no selectable entries.
    fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::SectionSelector => {
                if self.has_name_list() {
                    // Ensure the name list has a selection when entering it.
                    if self.active_name_list_state_mut().selected().is_none() {
                        self.active_name_list_state_mut().select(Some(0));
                    }
                    Focus::NameList
                } else {
                    self.detail_scroll = 0;
                    Focus::Detail
                }
            }
            Focus::NameList => {
                self.detail_scroll = 0;
                Focus::Detail
            }
            Focus::Detail => Focus::SectionSelector,
        };
    }

    /// Cycle focus backward: Detail → NameList → SectionSelector.
    fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            Focus::Detail => {
                if self.has_name_list() {
                    if self.active_name_list_state_mut().selected().is_none() {
                        self.active_name_list_state_mut().select(Some(0));
                    }
                    Focus::NameList
                } else {
                    Focus::SectionSelector
                }
            }
            Focus::NameList => Focus::SectionSelector,
            Focus::SectionSelector => {
                self.detail_scroll = 0;
                Focus::Detail
            }
        };
    }

    fn navigate_down(&mut self) {
        match self.focus {
            Focus::SectionSelector => {
                let current = self.section_selector_state.selected().unwrap_or(0);
                let new_idx = (current + 1).min(Section::ALL.len() - 1);
                self.section_selector_state.select(Some(new_idx));

                // Changing the highlighted section changes the active section.
                let new_section = Section::from_index(new_idx);
                if self.active_section != new_section {
                    self.active_section = new_section;
                    self.invalidate_cache();
                    self.ensure_active_section_cache();
                    self.reset_name_list_selection();
                    self.detail_scroll = 0;
                }
            }
            Focus::NameList => {
                let len = self.name_list_len();
                if len == 0 {
                    return;
                }
                let state = self.active_name_list_state_mut();
                let current = state.selected().unwrap_or(0);
                let new = (current + 1).min(len - 1);
                state.select(Some(new));
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_add(1);
            }
        }
    }

    fn navigate_up(&mut self) {
        match self.focus {
            Focus::SectionSelector => {
                let current = self.section_selector_state.selected().unwrap_or(0);
                let new_idx = current.saturating_sub(1);
                self.section_selector_state.select(Some(new_idx));

                let new_section = Section::from_index(new_idx);
                if self.active_section != new_section {
                    self.active_section = new_section;
                    self.invalidate_cache();
                    self.ensure_active_section_cache();
                    self.reset_name_list_selection();
                    self.detail_scroll = 0;
                }
            }
            Focus::NameList => {
                let state = self.active_name_list_state_mut();
                let current = state.selected().unwrap_or(0);
                let new = current.saturating_sub(1);
                state.select(Some(new));
                self.detail_scroll = 0;
            }
            Focus::Detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(1);
            }
        }
    }

    fn jump_to_top(&mut self) {
        match self.focus {
            Focus::SectionSelector => {
                self.section_selector_state.select(Some(0));
                let new_section = Section::Summary;
                if self.active_section != new_section {
                    self.active_section = new_section;
                    self.invalidate_cache();
                    self.ensure_active_section_cache();
                    self.reset_name_list_selection();
                    self.detail_scroll = 0;
                }
            }
            Focus::NameList => {
                let len = self.name_list_len();
                if len > 0 {
                    self.active_name_list_state_mut().select(Some(0));
                    self.detail_scroll = 0;
                }
            }
            Focus::Detail => {
                self.detail_scroll = 0;
            }
        }
    }

    fn jump_to_bottom(&mut self) {
        match self.focus {
            Focus::SectionSelector => {
                self.section_selector_state.select(Some(Section::ALL.len() - 1));
                let new_section = Section::Objectives;
                if self.active_section != new_section {
                    self.active_section = new_section;
                    self.invalidate_cache();
                    self.ensure_active_section_cache();
                    self.detail_scroll = 0;
                    self.reset_name_list_selection();
                }
            }
            Focus::NameList => {
                let len = self.name_list_len();
                if len > 0 {
                    self.active_name_list_state_mut().select(Some(len - 1));
                    self.detail_scroll = 0;
                }
            }
            Focus::Detail => {
                self.detail_scroll = u16::MAX;
            }
        }
    }

    /// Enter drops focus deeper: SectionSelector → NameList → Detail.
    /// On the Summary section (which has no name list), Enter does nothing.
    fn handle_enter(&mut self) {
        match self.focus {
            Focus::SectionSelector => {
                if self.has_name_list() {
                    if self.active_name_list_state_mut().selected().is_none() {
                        self.active_name_list_state_mut().select(Some(0));
                    }
                    self.focus = Focus::NameList;
                } else if self.active_section == Section::Summary {
                    self.focus = Focus::Detail;
                    self.detail_scroll = 0;
                }
            }
            Focus::NameList => {
                self.focus = Focus::Detail;
                self.detail_scroll = 0;
            }
            Focus::Detail => {}
        }
    }

    fn handle_escape(&mut self) {
        if !self.search_query.is_empty() {
            self.search_query.clear();
            self.invalidate_cache();
            self.ensure_active_section_cache();
            self.reset_name_list_selection();
        } else if self.focus == Focus::Detail {
            // Return to whichever sidebar panel makes sense.
            if self.has_name_list() && self.active_name_list_state_mut().selected().is_some() {
                self.focus = Focus::NameList;
            } else {
                self.focus = Focus::SectionSelector;
            }
        } else if self.focus == Focus::NameList {
            self.focus = Focus::SectionSelector;
        }
    }

    fn set_section(&mut self, section: Section) {
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.reset_name_list_selection();
        self.detail_scroll = 0;
        self.focus = Focus::SectionSelector;
    }

    const fn reset_name_list_selection(&mut self) {
        if let Some(idx) = self.active_section.list_index() {
            self.section_states[idx].list_state.select(None);
        }
    }

    fn set_filter(&mut self, filter: DiffFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.invalidate_cache();
            self.ensure_active_section_cache();
            self.reset_name_list_selection();
        }
    }
}
