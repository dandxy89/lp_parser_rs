use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, LpDiffReport};
use crate::search::{self, CompiledSearch, SearchMode};
pub use crate::state::{DiffFilter, Focus, SearchResult, Section, SectionViewState};

pub struct App {
    pub report: LpDiffReport,
    pub active_section: Section,
    pub focus: Focus,
    pub filter: DiffFilter,
    pub should_quit: bool,

    /// Whether the help pop-up overlay is visible.
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

    /// Visible height of the name list panel (set during draw).
    pub name_list_height: u16,

    /// Visible height of the detail panel (set during draw).
    pub detail_height: u16,

    /// Total number of content lines in the current detail view (set during draw).
    pub detail_content_lines: usize,

    /// Layout rects stored during draw for mouse hit-testing.
    pub section_selector_rect: Rect,
    pub name_list_rect: Rect,
    pub detail_rect: Rect,

    /// Timestamp of the last successful yank, used for the flash message.
    pub yank_flash: Option<Instant>,
    /// Message displayed in the status bar after a successful yank.
    pub yank_message: String,

    // Search pop-up (Telescope-style)
    /// Whether the search pop-up overlay is visible.
    pub show_search_popup: bool,
    /// Current query text in the search pop-up input.
    pub search_popup_query: String,
    /// Ranked search results spanning all sections.
    pub search_popup_results: Vec<SearchResult>,
    /// Currently highlighted result index in the pop-up.
    pub search_popup_selected: usize,
    /// Scroll offset for the detail preview pane inside the pop-up.
    pub search_popup_scroll: u16,
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
            name_list_height: 0,
            detail_height: 0,
            detail_content_lines: 0,
            section_selector_rect: Rect::default(),
            name_list_rect: Rect::default(),
            detail_rect: Rect::default(),
            yank_flash: None,
            yank_message: String::new(),
            show_search_popup: false,
            search_popup_query: String::new(),
            search_popup_results: Vec::new(),
            search_popup_selected: 0,
            search_popup_scroll: 0,
        }
    }

    /// Invalidate cached filtered indices for all sections.
    pub(crate) fn invalidate_cache(&mut self) {
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
    ///
    /// Note: this may lazily compile the search as a side-effect.
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
        if !self.section_states[idx].is_dirty() {
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
        self.active_section.list_index().map_or(0, |idx| self.section_states[idx].cached_indices().len())
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
    pub(crate) fn has_name_list(&self) -> bool {
        self.active_section != Section::Summary && self.name_list_len() > 0
    }

    pub(crate) const fn reset_name_list_selection(&mut self) {
        if let Some(idx) = self.active_section.list_index() {
            self.section_states[idx].list_state.select(None);
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
                self.detail_scroll = self.detail_scroll.saturating_add(n as u16);
            }
        }
    }

    /// Advance the name list selection by 1 (wrapping), resetting detail scroll.
    /// Used for `n` when a search is committed.
    pub fn search_next(&mut self) {
        let len = self.name_list_len();
        if len == 0 {
            return;
        }
        let state = self.active_name_list_state_mut();
        let current = state.selected().unwrap_or(0);
        let new = if current + 1 >= len { 0 } else { current + 1 };
        state.select(Some(new));
        self.detail_scroll = 0;
    }

    /// Move the name list selection back by 1 (wrapping), resetting detail scroll.
    /// Used for `N` when a search is committed.
    pub fn search_prev(&mut self) {
        let len = self.name_list_len();
        if len == 0 {
            return;
        }
        let state = self.active_name_list_state_mut();
        let current = state.selected().unwrap_or(0);
        let new = if current == 0 { len - 1 } else { current - 1 };
        state.select(Some(new));
        self.detail_scroll = 0;
    }

    /// Return the currently selected name list index (0-based within filtered list).
    pub fn selected_name_index(&self) -> Option<usize> {
        self.active_section.list_index().and_then(|idx| self.section_states[idx].list_state.selected())
    }

    /// Yank the selected entry's name to the system clipboard.
    pub fn yank_name(&mut self) {
        let name = self.selected_entry_name();
        let Some(name) = name else { return };
        let name = name.to_owned();
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&name)) {
            Ok(()) => {
                self.yank_message = format!("Yanked: {name}");
                self.yank_flash = Some(Instant::now());
            }
            Err(e) => {
                self.yank_message = format!("Yank failed: {e}");
                self.yank_flash = Some(Instant::now());
            }
        }
    }

    /// Yank the full detail panel content as plain text to the system clipboard.
    pub fn yank_detail(&mut self) {
        let text = crate::detail_text::render_detail_plain(self);
        let Some(text) = text else { return };
        let label = self.selected_entry_name().unwrap_or("detail").to_owned();
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
            Ok(()) => {
                self.yank_message = format!("Yanked detail: {label}");
                self.yank_flash = Some(Instant::now());
            }
            Err(e) => {
                self.yank_message = format!("Yank failed: {e}");
                self.yank_flash = Some(Instant::now());
            }
        }
    }

    /// Return the name of the currently selected entry, if any.
    fn selected_entry_name(&self) -> Option<&str> {
        let idx = self.active_section.list_index()?;
        let sel = self.section_states[idx].list_state.selected()?;
        let entry_idx = *self.section_states[idx].cached_indices().get(sel)?;
        match self.active_section {
            Section::Variables => self.report.variables.entries.get(entry_idx).map(|e| e.name.as_str()),
            Section::Constraints => self.report.constraints.entries.get(entry_idx).map(|e| e.name.as_str()),
            Section::Objectives => self.report.objectives.entries.get(entry_idx).map(|e| e.name.as_str()),
            Section::Summary => None,
        }
    }

    /// Recompute search pop-up results from the current query.
    ///
    /// Builds a flat haystack from all three sections (variables, constraints,
    /// objectives), runs the appropriate search mode, and populates
    /// `search_popup_results` with ranked/filtered results.
    pub fn recompute_search_popup(&mut self) {
        self.search_popup_results.clear();
        self.search_popup_selected = 0;
        self.search_popup_scroll = 0;

        let (mode, pattern) = search::parse_query(&self.search_popup_query);

        // Build flat entry metadata: (Section, entry_index, name, kind).
        let mut entries: Vec<(Section, usize, String, crate::diff_model::DiffKind)> = Vec::new();
        for (i, e) in self.report.variables.entries.iter().enumerate() {
            entries.push((Section::Variables, i, e.name().to_owned(), e.kind()));
        }
        for (i, e) in self.report.constraints.entries.iter().enumerate() {
            entries.push((Section::Constraints, i, e.name().to_owned(), e.kind()));
        }
        for (i, e) in self.report.objectives.entries.iter().enumerate() {
            entries.push((Section::Objectives, i, e.name().to_owned(), e.kind()));
        }

        if self.search_popup_query.is_empty() {
            // Show all entries, no scoring.
            for (section, idx, name, kind) in entries {
                self.search_popup_results.push(SearchResult { section, entry_index: idx, score: 0, match_indices: Vec::new(), name, kind });
            }
            return;
        }

        match mode {
            SearchMode::Fuzzy => {
                let names: Vec<&str> = entries.iter().map(|(_, _, name, _)| name.as_str()).collect();
                let config = frizbee::Config { sort: true, ..Default::default() };
                let matches = frizbee::match_list_indices(pattern, &names, &config);

                for m in matches {
                    let (section, entry_index, name, kind) = &entries[m.index as usize];
                    // frizbee returns indices in reverse order; sort ascending for highlighting.
                    let mut indices = m.indices;
                    indices.sort_unstable();
                    self.search_popup_results.push(SearchResult {
                        section: *section,
                        entry_index: *entry_index,
                        score: m.score,
                        match_indices: indices,
                        name: name.clone(),
                        kind: *kind,
                    });
                }
            }
            SearchMode::Regex | SearchMode::Substring => {
                let compiled = CompiledSearch::compile(&self.search_popup_query);
                for (section, idx, name, kind) in entries {
                    if compiled.matches(&name) {
                        self.search_popup_results.push(SearchResult {
                            section,
                            entry_index: idx,
                            score: 0,
                            match_indices: Vec::new(),
                            name,
                            kind,
                        });
                    }
                }
            }
        }
    }

    /// Confirm the currently selected search pop-up result: close the pop-up,
    /// switch to the result's section, select the entry, and focus the name list.
    pub fn confirm_search_selection(&mut self) {
        let Some(result) = self.search_popup_results.get(self.search_popup_selected) else {
            // Nothing selected — just close.
            self.show_search_popup = false;
            return;
        };

        let section = result.section;
        let entry_index = result.entry_index;

        // Close pop-up.
        self.show_search_popup = false;

        // Switch to the target section.
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));

        // Clear any existing inline search and recompute caches with no filter.
        self.search_query.clear();
        self.filter = DiffFilter::All;
        self.invalidate_cache();
        self.ensure_active_section_cache();

        // Find the position of `entry_index` within the filtered indices and select it.
        let Some(list_idx) = section.list_index() else {
            return;
        };
        let filtered = self.section_states[list_idx].cached_indices();
        if let Some(pos) = filtered.iter().position(|&i| i == entry_index) {
            self.section_states[list_idx].list_state.select(Some(pos));
        }

        self.focus = Focus::NameList;
        self.detail_scroll = 0;
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
                self.detail_scroll = self.detail_scroll.saturating_sub(n as u16);
            }
        }
    }
}
