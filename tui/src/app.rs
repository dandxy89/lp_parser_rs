use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::widgets::ListState;
use smallvec::SmallVec;

use crate::detail_model::{CoefficientRow, build_coeff_rows};
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
///
/// Names are stored separately in `search_name_buffer` at the same index
/// to avoid duplicating every entry name as an owned `String`.
pub struct HaystackEntry {
    pub section: Section,
    pub index: usize,
    pub kind: DiffKind,
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
}

impl SolverSession {
    fn new() -> Self {
        Self { state: SolveState::Idle, view: SolveViewState::default(), receive: None, receive2: None }
    }
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

    /// `HiGHS` solver session (state + view + channel).
    pub solver: SolverSession,

    /// Path to the first LP file.
    pub file1_path: PathBuf,

    /// Path to the second LP file.
    pub file2_path: PathBuf,

    /// Pre-built flat haystack for the search pop-up (built once in `App::new`).
    pub(crate) search_haystack: Vec<HaystackEntry>,

    /// Re-usable buffer for fuzzy search name references, avoiding per-keystroke `Vec` allocation.
    /// Rebuilt when the haystack changes (indices correspond 1:1 with `search_haystack`).
    pub(crate) search_name_buffer: Vec<String>,

    /// Cached coefficient rows for the detail panel, avoiding per-frame `BTreeMap` + String allocations.
    /// Invalidated when the selected entry changes.
    pub(crate) coeff_row_cache: Option<CoeffRowCache>,
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

impl App {
    pub fn new(report: LpDiffReport, file1_path: PathBuf, file2_path: PathBuf) -> Self {
        let mut section_selector_state = ListState::default();
        section_selector_state.select(Some(0));

        let (haystack, names) = build_haystack(&report);

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
            solver: SolverSession::new(),
            file1_path,
            file2_path,
            search_name_buffer: names,
            search_haystack: haystack,
            coeff_row_cache: None,
        }
    }

    /// Invalidate cached filtered indices for all sections and the coefficient row cache.
    pub(crate) fn invalidate_cache(&mut self) {
        for state in &mut self.section_states {
            state.invalidate();
        }
        self.coeff_row_cache = None;
    }

    /// Recompute filtered indices for a given list section.
    /// Panics (in debug) if `section` is `Summary`.
    fn recompute_section_cache(&mut self, section: Section) {
        debug_assert!(section != Section::Summary, "Summary has no list entries to recompute");
        let index = section.list_index().expect("non-Summary section has list_index");
        let filter = self.filter;
        match section {
            Section::Variables => self.section_states[index].recompute(&self.report.variables.entries, filter),
            Section::Constraints => self.section_states[index].recompute(&self.report.constraints.entries, filter),
            Section::Objectives => self.section_states[index].recompute(&self.report.objectives.entries, filter),
            Section::Summary => unreachable!("Summary has no list_index"),
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
                        build_coeff_rows(coeff_changes, old_coefficients, new_coefficients)
                    }
                    crate::diff_model::ConstraintDiffDetail::Sos { weight_changes, old_weights, new_weights, .. } => {
                        build_coeff_rows(weight_changes, old_weights, new_weights)
                    }
                    _ => return,
                }
            }
            Section::Objectives => {
                let entry = &self.report.objectives.entries[entry_index];
                build_coeff_rows(&entry.coeff_changes, &entry.old_coefficients, &entry.new_coefficients)
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
    /// Returns `None` if the active section is Summary or nothing is selected.
    pub(crate) fn selected_entry_index(&self) -> Option<usize> {
        let section_index = self.active_section.list_index()?;
        let state = &self.section_states[section_index];
        let selected = state.list_state.selected()?;
        state.cached_indices().get(selected).copied()
    }

    /// Whether the name list panel has selectable content for the current section.
    pub(crate) fn has_name_list(&self) -> bool {
        self.active_section != Section::Summary && self.name_list_len() > 0
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
                self.detail_scroll = self.detail_scroll.saturating_add(step);
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

        let result = CLIPBOARD.with_borrow_mut(|cb| {
            if cb.is_none() {
                *cb = arboard::Clipboard::new().ok();
            }
            cb.as_mut().map_or(Err(arboard::Error::ClipboardNotSupported), |clipboard| clipboard.set_text(text))
        });

        match result {
            Ok(()) => {
                label.clone_into(&mut self.yank.message);
                self.yank.flash = Some(Instant::now());
            }
            Err(error) => {
                self.yank.message = format!("Yank failed: {error}");
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

    /// Yank the full detail panel content as plain text to the system clipboard.
    pub fn yank_detail(&mut self) {
        let Some(text) = crate::detail_text::render_detail_plain(self) else { return };
        let label = self.selected_entry_name().unwrap_or("detail").to_owned();
        self.set_yank_flash(&format!("Yanked detail: {label}"), &text);
    }

    /// Return the name of an entry given section and entry index.
    ///
    /// Returns `None` if `section` is `Summary` or the index is out of bounds.
    fn entry_name(&self, section: Section, entry_index: usize) -> Option<&str> {
        match section {
            Section::Variables => self.report.variables.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Constraints => self.report.constraints.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Objectives => self.report.objectives.entries.get(entry_index).map(|e| e.name.as_str()),
            Section::Summary => None,
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

    /// Navigate to a jumplist entry, restoring section, selection, scroll, and filter.
    pub(crate) fn restore_jump(&mut self, entry: JumpEntry) {
        self.active_section = entry.section;
        self.section_selector_state.select(Some(entry.section.index()));
        self.filter = entry.filter;
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
            if entry.entry_index.is_some() && entry.section != Section::Summary { Focus::NameList } else { Focus::SectionSelector };
    }

    /// Poll the solver channel(s) for results, transitioning state when complete.
    pub fn poll_solve(&mut self) {
        if matches!(self.solver.state, SolveState::RunningBoth { .. }) {
            self.poll_solve_both();
        } else {
            self.poll_solve_single();
        }
    }

    /// Poll a single solver channel (`Running` state).
    fn poll_solve_single(&mut self) {
        let Some(receive) = &self.solver.receive else {
            return;
        };
        match receive.try_recv() {
            Ok(Ok(result)) => {
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
        let SolveState::RunningBoth { file1, file2, result1, result2 } = &mut self.solver.state else {
            return;
        };
        if result1.is_some() && result2.is_some() {
            let r1 = *result1.take().expect("checked Some above");
            let r2 = *result2.take().expect("checked Some above");
            let label1 = file1.clone();
            let label2 = file2.clone();
            let diff = crate::solver::diff_results(label1, label2, r1, r2, self.solver.view.delta_threshold);
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
        let new_diff =
            crate::solver::diff_results(old_diff.file1_label, old_diff.file2_label, old_diff.result1, old_diff.result2, threshold);
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

        if self.search_popup.query.is_empty() {
            self.populate_all_search_results();
            return;
        }

        // Parse mode and pattern. For fuzzy mode, the pattern is always the full
        // query (no prefix), so we can use the query length to detect that case
        // without holding a borrow across mutable calls.
        let mode = search::parse_query(&self.search_popup.query).0;

        match mode {
            SearchMode::Fuzzy => {
                // Fuzzy mode has no prefix — pattern is the entire query.
                self.populate_fuzzy_results();
            }
            SearchMode::Regex | SearchMode::Substring => self.populate_filtered_results(),
        }
    }

    /// Populate search results with all entries (no query filter).
    fn populate_all_search_results(&mut self) {
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            self.search_popup.results.push(SearchResult {
                section: entry.section,
                entry_index: entry.index,
                score: 0,
                match_indices: SmallVec::new(),
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
        let matches = frizbee::match_list_indices(&self.search_popup.query, &self.search_name_buffer, &config);

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
                match_indices: SmallVec::from_vec(indices),
                haystack_index,
                kind: entry.kind,
            });
        }
    }

    /// Populate search results using regex or substring matching.
    ///
    /// Must not be called for fuzzy mode — that uses `populate_fuzzy_results` instead.
    fn populate_filtered_results(&mut self) {
        let compiled = CompiledSearch::compile(&self.search_popup.query);
        debug_assert!(
            !matches!(compiled, CompiledSearch::Fuzzy(..)),
            "populate_filtered_results called with Fuzzy query; use populate_fuzzy_results instead"
        );
        for (haystack_index, entry) in self.search_haystack.iter().enumerate() {
            if compiled.matches(&self.search_name_buffer[haystack_index]) {
                self.search_popup.results.push(SearchResult {
                    section: entry.section,
                    entry_index: entry.index,
                    score: 0,
                    match_indices: SmallVec::new(),
                    haystack_index,
                    kind: entry.kind,
                });
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
}
