use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::widgets::ListState;

use crate::diff_model::{DiffEntry, DiffKind, LpDiffReport};
use crate::search::{self, CompiledSearch, SearchMode};
use crate::solver::SolveResult;
pub use crate::state::{DiffFilter, Focus, SearchResult, Section, SectionViewState};
use crate::state::{JumpEntry, JumpList, SolveState, SolveViewState};

/// State for the telescope-style search pop-up overlay.
pub struct SearchPopupState {
    /// Whether the search pop-up overlay is visible.
    pub visible: bool,
    /// Current query text in the search pop-up input.
    pub query: String,
    /// Ranked search results spanning all sections.
    pub results: Vec<SearchResult>,
    /// Currently highlighted result index in the pop-up.
    pub selected: usize,
    /// Scroll offset for the detail preview pane inside the pop-up.
    pub scroll: u16,
}

/// Layout rectangles and dimensions stored during draw for mouse hit-testing and scrolling.
pub struct LayoutRects {
    pub section_selector: Rect,
    pub name_list: Rect,
    pub detail: Rect,
    pub name_list_height: u16,
    pub detail_height: u16,
    pub detail_content_lines: usize,
}

/// Yank (clipboard) flash state.
pub struct YankState {
    /// Timestamp of the last successful yank, used for the flash message.
    pub flash: Option<Instant>,
    /// Message displayed in the status bar after a successful yank.
    pub message: String,
}

/// A single entry in the pre-built flat search haystack.
pub(crate) struct HaystackEntry {
    pub section: Section,
    pub index: usize,
    pub name: String,
    pub kind: DiffKind,
}

pub struct App {
    pub report: LpDiffReport,
    pub active_section: Section,
    pub focus: Focus,
    pub filter: DiffFilter,
    pub should_quit: bool,

    /// Whether the help pop-up overlay is visible.
    pub show_help: bool,

    /// Scroll offset for the detail panel when it has focus.
    pub detail_scroll: u16,

    /// Section selector list state (tracks which of the 4 sections is highlighted).
    pub section_selector_state: ListState,

    /// Per-section view states: [Variables, Constraints, Objectives].
    pub section_states: [SectionViewState; 3],

    /// Layout rectangles and dimensions stored during draw.
    pub layout: LayoutRects,

    /// Yank (clipboard) flash state.
    pub yank: YankState,

    /// Telescope-style search pop-up state.
    pub search_popup: SearchPopupState,

    /// Navigation jumplist for Ctrl+o / Ctrl+i.
    pub jumplist: JumpList,

    /// HiGHS solver state.
    pub solve_state: SolveState,

    /// Scroll state for the solve results panel.
    pub solve_view: SolveViewState,

    /// Channel for receiving solve results from the background thread.
    pub solve_rx: Option<mpsc::Receiver<Result<SolveResult, String>>>,

    /// Path to the first LP file.
    pub file1_path: PathBuf,

    /// Path to the second LP file.
    pub file2_path: PathBuf,

    /// Pre-built flat haystack for the search pop-up (built once in `App::new`).
    pub(crate) search_haystack: Vec<HaystackEntry>,

    /// Re-usable buffer for fuzzy search name references, avoiding per-keystroke allocation.
    /// Rebuilt once when the search pop-up opens (indices correspond 1:1 with `search_haystack`).
    pub(crate) search_name_buf: Vec<String>,
}

impl App {
    pub fn new(report: LpDiffReport, file1_path: PathBuf, file2_path: PathBuf) -> Self {
        let mut section_selector_state = ListState::default();
        section_selector_state.select(Some(0));

        // Pre-build the flat search haystack from all three sections.
        let mut haystack = Vec::new();
        for (i, e) in report.variables.entries.iter().enumerate() {
            haystack.push(HaystackEntry { section: Section::Variables, index: i, name: e.name().to_owned(), kind: e.kind() });
        }
        for (i, e) in report.constraints.entries.iter().enumerate() {
            haystack.push(HaystackEntry { section: Section::Constraints, index: i, name: e.name().to_owned(), kind: e.kind() });
        }
        for (i, e) in report.objectives.entries.iter().enumerate() {
            haystack.push(HaystackEntry { section: Section::Objectives, index: i, name: e.name().to_owned(), kind: e.kind() });
        }

        Self {
            report,
            active_section: Section::Summary,
            focus: Focus::SectionSelector,
            filter: DiffFilter::All,
            should_quit: false,
            show_help: false,
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
            },
            yank: YankState { flash: None, message: String::new() },
            search_popup: SearchPopupState { visible: false, query: String::new(), results: Vec::new(), selected: 0, scroll: 0 },
            jumplist: JumpList::new(),
            solve_state: SolveState::Idle,
            solve_view: SolveViewState::default(),
            solve_rx: None,
            file1_path,
            file2_path,
            search_haystack: haystack,
            search_name_buf: Vec::new(),
        }
    }

    /// Invalidate cached filtered indices for all sections.
    pub(crate) fn invalidate_cache(&mut self) {
        for state in &mut self.section_states {
            state.invalidate();
        }
    }

    /// Ensure the active section's cache is fresh. Call once per frame before drawing.
    pub fn ensure_active_section_cache(&mut self) {
        let Some(idx) = self.active_section.list_index() else {
            return;
        };
        if !self.section_states[idx].is_dirty() {
            return;
        }

        let filter = self.filter;
        let section = Section::LIST_SECTIONS[idx];
        match section {
            Section::Variables => {
                self.section_states[idx].recompute(&self.report.variables.entries, filter);
            }
            Section::Constraints => {
                self.section_states[idx].recompute(&self.report.constraints.entries, filter);
            }
            Section::Objectives => {
                self.section_states[idx].recompute(&self.report.objectives.entries, filter);
            }
            Section::Summary => unreachable!("Summary has no list_index"),
        }
    }

    /// Return the number of items in the name list for the current section.
    /// Must be called after `ensure_active_section_cache()`.
    pub fn name_list_len(&self) -> usize {
        self.active_section.list_index().map_or(0, |idx| self.section_states[idx].cached_indices().len())
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
                debug_assert!(n <= u16::MAX as usize, "page scroll step {n} exceeds u16::MAX");
                self.detail_scroll = self.detail_scroll.saturating_add(n as u16);
            }
        }
    }

    /// Yank the selected entry's name to the system clipboard.
    pub fn yank_name(&mut self) {
        let name = self.selected_entry_name();
        let Some(name) = name else { return };
        let name = name.to_owned();
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&name)) {
            Ok(()) => {
                self.yank.message = format!("Yanked: {name}");
                self.yank.flash = Some(Instant::now());
            }
            Err(e) => {
                self.yank.message = format!("Yank failed: {e}");
                self.yank.flash = Some(Instant::now());
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
                self.yank.message = format!("Yanked detail: {label}");
                self.yank.flash = Some(Instant::now());
            }
            Err(e) => {
                self.yank.message = format!("Yank failed: {e}");
                self.yank.flash = Some(Instant::now());
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

    /// Record the current navigation position in the jumplist.
    pub(crate) fn record_jump(&mut self) {
        let entry_index = self.active_section.list_index().and_then(|idx| self.section_states[idx].list_state.selected());
        self.jumplist.push(JumpEntry { section: self.active_section, entry_index, detail_scroll: self.detail_scroll, filter: self.filter });
    }

    /// Navigate to a jumplist entry, restoring section, selection, scroll, and filter.
    pub(crate) fn restore_jump(&mut self, entry: JumpEntry) {
        self.active_section = entry.section;
        self.section_selector_state.select(Some(entry.section.index()));
        self.filter = entry.filter;
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.detail_scroll = entry.detail_scroll;

        if let Some(idx) = entry.section.list_index() {
            if let Some(sel) = entry.entry_index {
                let len = self.section_states[idx].cached_indices().len();
                if sel < len {
                    self.section_states[idx].list_state.select(Some(sel));
                } else if len > 0 {
                    self.section_states[idx].list_state.select(Some(len - 1));
                } else {
                    self.section_states[idx].list_state.select(None);
                }
            } else {
                self.section_states[idx].list_state.select(None);
            }
        }

        self.focus =
            if entry.entry_index.is_some() && entry.section != Section::Summary { Focus::NameList } else { Focus::SectionSelector };
    }

    /// Poll the solver channel for a result, transitioning state if available.
    pub fn poll_solve(&mut self) {
        if let Some(rx) = &self.solve_rx {
            match rx.try_recv() {
                Ok(Ok(result)) => {
                    self.solve_state = SolveState::Done(Box::new(result));
                    self.solve_view.scroll = 0;
                    self.solve_rx = None;
                }
                Ok(Err(err)) => {
                    self.solve_state = SolveState::Failed(err);
                    self.solve_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {} // still running
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.solve_state = SolveState::Failed("Solver thread disconnected".to_owned());
                    self.solve_rx = None;
                }
            }
        }
    }

    /// Recompute search pop-up results from the current query.
    ///
    /// References the pre-built haystack rather than rebuilding it each time.
    pub fn recompute_search_popup(&mut self) {
        self.search_popup.results.clear();
        self.search_popup.selected = 0;
        self.search_popup.scroll = 0;

        let (mode, pattern) = search::parse_query(&self.search_popup.query);

        if self.search_popup.query.is_empty() {
            // Show all entries, no scoring.
            for (hi, entry) in self.search_haystack.iter().enumerate() {
                self.search_popup.results.push(SearchResult {
                    section: entry.section,
                    entry_index: entry.index,
                    score: 0,
                    match_indices: Vec::new(),
                    haystack_index: hi,
                    kind: entry.kind,
                });
            }
            return;
        }

        match mode {
            SearchMode::Fuzzy => {
                // Lazily populate the reusable name buffer on first fuzzy search.
                if self.search_name_buf.len() != self.search_haystack.len() {
                    self.search_name_buf = self.search_haystack.iter().map(|e| e.name.clone()).collect();
                }
                let names: Vec<&str> = self.search_name_buf.iter().map(String::as_str).collect();
                let config = frizbee::Config { sort: true, ..Default::default() };
                let matches = frizbee::match_list_indices(pattern, &names, &config);

                for m in matches {
                    let hi = m.index as usize;
                    let entry = &self.search_haystack[hi];
                    // frizbee returns indices in reverse order; sort ascending for highlighting.
                    let mut indices = m.indices;
                    indices.sort_unstable();
                    self.search_popup.results.push(SearchResult {
                        section: entry.section,
                        entry_index: entry.index,
                        score: m.score,
                        match_indices: indices,
                        haystack_index: hi,
                        kind: entry.kind,
                    });
                }
            }
            SearchMode::Regex | SearchMode::Substring => {
                let compiled = CompiledSearch::compile(&self.search_popup.query);
                for (hi, entry) in self.search_haystack.iter().enumerate() {
                    if compiled.matches(&entry.name) {
                        self.search_popup.results.push(SearchResult {
                            section: entry.section,
                            entry_index: entry.index,
                            score: 0,
                            match_indices: Vec::new(),
                            haystack_index: hi,
                            kind: entry.kind,
                        });
                    }
                }
            }
        }
    }

    /// Confirm the currently selected search pop-up result: close the pop-up,
    /// switch to the result's section, select the entry, and focus the name list.
    pub fn confirm_search_selection(&mut self) {
        let Some(result) = self.search_popup.results.get(self.search_popup.selected) else {
            // Nothing selected — just close.
            self.search_popup.visible = false;
            return;
        };

        let section = result.section;
        let entry_index = result.entry_index;

        // Record current position before jumping.
        self.record_jump();

        // Close pop-up.
        self.search_popup.visible = false;

        // Switch to the target section.
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));

        // Reset filter and recompute caches.
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
                debug_assert!(n <= u16::MAX as usize, "page scroll step {n} exceeds u16::MAX");
                self.detail_scroll = self.detail_scroll.saturating_sub(n as u16);
            }
        }
    }
}
