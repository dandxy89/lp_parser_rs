use std::sync::{Arc, mpsc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use lp_parser_rs::problem::LpProblem;

use crate::app::App;
use crate::detail_text::{format_solve_diff_result, format_solve_result};
use crate::state::{DiffFilter, Focus, PendingYank, Section, Side, SolveState, SolveTab, SolveViewState};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C is an unconditional quit regardless of any other mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.search_popup.visible {
            self.handle_search_popup_key(key);
            return;
        }

        if !matches!(self.solver.state, SolveState::Idle) {
            self.handle_solve_key(key);
            return;
        }

        if self.show_help {
            // Any key dismisses the help pop-up.
            self.show_help = false;
            return;
        }

        self.handle_normal_key(key);
    }

    /// Handle a key event while the search pop-up is visible.
    fn handle_search_popup_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_popup.visible = false;
            }
            KeyCode::Enter => {
                self.confirm_search_selection();
            }
            KeyCode::Backspace => {
                self.search_popup.query.pop();
                self.recompute_search_popup();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.search_popup.results.is_empty() {
                    self.search_popup.selected = (self.search_popup.selected + 1).min(self.search_popup.results.len() - 1);
                    self.search_popup.scroll = 0;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.search_popup.selected = self.search_popup.selected.saturating_sub(1);
                self.search_popup.scroll = 0;
            }
            KeyCode::Tab => {
                // Replace query with the selected result's full name.
                if let Some(result) = self.search_popup.results.get(self.search_popup.selected) {
                    self.search_popup.query = self.search_name_buffer[result.haystack_index].clone();
                    self.recompute_search_popup();
                }
            }
            KeyCode::Char(character) => {
                self.search_popup.query.push(character);
                self.recompute_search_popup();
            }
            _ => {}
        }
    }

    /// Handle a key event in normal (non-search) mode.
    fn handle_normal_key(&mut self, key: KeyEvent) {
        // Handle pending yank chord first.
        if self.pending_yank == PendingYank::WaitingForTarget {
            self.pending_yank = PendingYank::None;
            match key.code {
                KeyCode::Char('o') => {
                    self.yank_side(Side::Old);
                    return;
                }
                KeyCode::Char('n') => {
                    self.yank_side(Side::New);
                    return;
                }
                KeyCode::Char('y') => {
                    self.yank_name();
                    return;
                }
                _ => {} // fall through to process the key normally
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && self.handle_ctrl_key(key) {
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Tab => self.cycle_focus_forward(),
            KeyCode::BackTab => self.cycle_focus_backward(),

            // Direct section jump.
            KeyCode::Char('1') => self.set_section(Section::Summary),
            KeyCode::Char('2') => self.set_section(Section::Variables),
            KeyCode::Char('3') => self.set_section(Section::Constraints),
            KeyCode::Char('4') => self.set_section(Section::Objectives),

            // Navigation (vi-style and arrow keys).
            KeyCode::Char('j' | 'n') | KeyCode::Down => self.navigate_down(),
            KeyCode::Char('k' | 'N') | KeyCode::Up => self.navigate_up(),
            KeyCode::Char('g') | KeyCode::Home => self.jump_to_top(),
            KeyCode::Char('G') | KeyCode::End => self.jump_to_bottom(),

            KeyCode::Enter => self.handle_enter(),
            KeyCode::Esc => self.handle_escape(),

            // h/l focus movement (left/right between sidebar and detail).
            KeyCode::Char('l') => self.focus_detail(),
            KeyCode::Char('h') => self.focus_sidebar(),

            // Filter shortcuts.
            KeyCode::Char('a') => self.set_filter(DiffFilter::All),
            KeyCode::Char('+') => self.set_filter(DiffFilter::Added),
            KeyCode::Char('-') => self.set_filter(DiffFilter::Removed),
            KeyCode::Char('m') => self.set_filter(DiffFilter::Modified),

            KeyCode::Char('?') => self.show_help = !self.show_help,

            // Yank (clipboard): `y` begins a chord, `Y` yanks detail immediately.
            KeyCode::Char('y') => self.pending_yank = PendingYank::WaitingForTarget,
            KeyCode::Char('Y') => self.yank_detail(),

            // Open the solver file picker.
            KeyCode::Char('S') => self.solver.state = SolveState::Picking,

            // Export the diff report as CSV.
            KeyCode::Char('w') => self.export_csv(),

            // Open the search pop-up.
            KeyCode::Char('/') => self.open_search_popup(),

            _ => {}
        }
    }

    /// Handle Ctrl-modified keys. Returns `true` if the key was consumed.
    fn handle_ctrl_key(&mut self, key: KeyEvent) -> bool {
        let visible = match self.focus {
            Focus::NameList => self.layout.name_list_height.saturating_sub(2) as usize,
            Focus::Detail => self.layout.detail_height.saturating_sub(2) as usize,
            Focus::SectionSelector => 0,
        };
        match key.code {
            KeyCode::Char('d') => self.page_down(visible / 2),
            KeyCode::Char('u') => self.page_up(visible / 2),
            KeyCode::Char('f') => self.page_down(visible),
            KeyCode::Char('b') => self.page_up(visible),
            KeyCode::Char('o') => {
                if let Some(entry) = self.jumplist.go_back() {
                    let entry = *entry;
                    self.restore_jump(entry);
                }
            }
            KeyCode::Char('i') => {
                if let Some(entry) = self.jumplist.go_forward() {
                    let entry = *entry;
                    self.restore_jump(entry);
                }
            }
            _ => return false,
        }
        true
    }

    fn focus_detail(&mut self) {
        if self.focus != Focus::Detail {
            self.focus = Focus::Detail;
            self.detail_scroll = 0;
        }
    }

    fn focus_sidebar(&mut self) {
        if self.focus == Focus::Detail {
            if self.has_name_list() && self.active_name_list_state_mut().selected().is_some() {
                self.focus = Focus::NameList;
            } else {
                self.focus = Focus::SectionSelector;
            }
        }
    }

    fn open_search_popup(&mut self) {
        self.search_popup.visible = true;
        self.search_popup.query.clear();
        self.search_popup.results.clear();
        self.search_popup.selected = 0;
        self.search_popup.scroll = 0;
        self.recompute_search_popup();
    }

    /// Cycle focus: `SectionSelector` → `NameList` → Detail → `SectionSelector`.
    /// Skips `NameList` when the current section has no selectable entries.
    fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::SectionSelector => {
                if self.has_name_list() {
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

    /// Cycle focus backward: Detail → `NameList` → `SectionSelector`.
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
                let new_index = (current + 1).min(Section::ALL.len() - 1);
                let new_section = Section::from_index(new_index);
                if self.active_section != new_section {
                    self.set_active_section(new_section);
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
                let new_index = current.saturating_sub(1);
                let new_section = Section::from_index(new_index);
                if self.active_section != new_section {
                    self.set_active_section(new_section);
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
                let new_section = Section::Summary;
                if self.active_section != new_section {
                    self.set_active_section(new_section);
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
                let new_section = Section::Objectives;
                if self.active_section != new_section {
                    self.set_active_section(new_section);
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
                // Intentionally set to u16::MAX: ratatui's Paragraph::scroll clamps
                // to the actual content height, so this safely scrolls to the end
                // without needing to know the content height in advance.
                self.detail_scroll = u16::MAX;
            }
        }
    }

    /// Enter drops focus deeper: `SectionSelector` → `NameList` → Detail.
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
        if self.focus == Focus::Detail {
            if self.has_name_list() && self.active_name_list_state_mut().selected().is_some() {
                self.focus = Focus::NameList;
            } else {
                self.focus = Focus::SectionSelector;
            }
        } else if self.focus == Focus::NameList {
            self.focus = Focus::SectionSelector;
        }
    }

    /// Handle a key event while the solver overlay is visible.
    fn handle_solve_key(&mut self, key: KeyEvent) {
        match &self.solver.state {
            SolveState::Idle => unreachable!("handle_solve_key called in Idle state"),
            SolveState::Picking => self.handle_solve_picker_key(key),
            SolveState::Running { .. } | SolveState::RunningBoth { .. } => {
                if key.code == KeyCode::Esc {
                    self.solver.state = SolveState::Idle;
                    self.solver.receive = None;
                    self.solver.receive2 = None;
                }
            }
            SolveState::Done(_) => self.handle_solve_done_key(key),
            SolveState::DoneBoth(_) => self.handle_solve_done_both_key(key),
            SolveState::Failed(_) => {
                if key.code == KeyCode::Esc {
                    self.solver.state = SolveState::Idle;
                }
            }
        }
    }

    /// Handle keys in the solver file picker state.
    fn handle_solve_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.solver.state = SolveState::Idle,
            KeyCode::Char('1') => {
                let problem = Arc::clone(&self.problem1);
                let label = self.file1_path.display().to_string();
                self.spawn_solver(problem, label);
            }
            KeyCode::Char('2') => {
                let problem = Arc::clone(&self.problem2);
                let label = self.file2_path.display().to_string();
                self.spawn_solver(problem, label);
            }
            KeyCode::Char('3') => self.spawn_both_solvers(),
            _ => {}
        }
    }

    /// Handle shared navigation keys for solve results views (Done and `DoneBoth`).
    /// Returns `true` if the key was consumed by shared navigation.
    fn handle_solve_results_nav(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => self.solver.state = SolveState::Idle,
            KeyCode::Char('1') => self.switch_solve_tab(SolveTab::Summary),
            KeyCode::Char('2') => self.switch_solve_tab(SolveTab::Variables),
            KeyCode::Char('3') => self.switch_solve_tab(SolveTab::Constraints),
            KeyCode::Char('4') => self.switch_solve_tab(SolveTab::Log),
            KeyCode::Tab => self.switch_solve_tab(self.solver.view.tab.next()),
            KeyCode::BackTab => self.switch_solve_tab(self.solver.view.tab.prev()),
            KeyCode::Char('j') | KeyCode::Down => {
                let tab_index = self.solver.view.tab.index();
                self.solver.view.scroll[tab_index] = self.solver.view.scroll[tab_index].saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let tab_index = self.solver.view.tab.index();
                self.solver.view.scroll[tab_index] = self.solver.view.scroll[tab_index].saturating_sub(1);
            }
            _ => return false,
        }
        true
    }

    /// Handle keys in the solver done/results state.
    fn handle_solve_done_key(&mut self, key: KeyEvent) {
        if self.handle_solve_results_nav(key) {
            return;
        }
        if key.code == KeyCode::Char('y')
            && let SolveState::Done(result) = &self.solver.state
        {
            let text = format_solve_result(result);
            self.set_yank_flash("Yanked solve results", &text);
        }
    }

    /// Spawn the solver in a background thread for the given problem.
    fn spawn_solver(&mut self, problem: Arc<LpProblem>, file_label: String) {
        self.solver.state = SolveState::Running { file: file_label };
        self.solver.view = SolveViewState::default();

        let (sender, receiver) = mpsc::channel();
        self.solver.receive = Some(receiver);

        std::thread::spawn(move || {
            let result = crate::solver::solve_problem(&problem);
            // Receiver may be dropped if the user dismissed the overlay — this is expected.
            if sender.send(result).is_err() {
                eprintln!("solve result dropped: receiver closed");
            }
        });
    }

    /// Handle keys in the solver done-both/comparison state.
    fn handle_solve_done_both_key(&mut self, key: KeyEvent) {
        if self.handle_solve_results_nav(key) {
            return;
        }
        match key.code {
            KeyCode::Char('d') => {
                self.solver.view.diff_only = !self.solver.view.diff_only;
            }
            KeyCode::Char('t') => {
                self.solver.view.cycle_threshold_forward();
                self.recompute_solve_diff();
            }
            KeyCode::Char('T') => {
                self.solver.view.cycle_threshold_backward();
                self.recompute_solve_diff();
            }
            KeyCode::Char('y') => {
                if let SolveState::DoneBoth(diff) = &self.solver.state {
                    let text = format_solve_diff_result(diff);
                    self.set_yank_flash("Yanked solve comparison", &text);
                }
            }
            KeyCode::Char('w') => {
                if let SolveState::DoneBoth(diff) = &self.solver.state {
                    let dir = match std::env::current_dir() {
                        Ok(d) => d,
                        Err(e) => {
                            self.yank.message = format!("CSV write failed: {e}");
                            self.yank.flash = Some(std::time::Instant::now());
                            return;
                        }
                    };
                    match crate::solver::write_diff_csv(diff, &dir) {
                        Ok((var_file, con_file)) => {
                            self.yank.message = format!("Wrote {var_file} and {con_file}");
                            self.yank.flash = Some(std::time::Instant::now());
                        }
                        Err(e) => {
                            self.yank.message = format!("CSV write failed: {e}");
                            self.yank.flash = Some(std::time::Instant::now());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Spawn both solvers in parallel for the "Both (diff)" option.
    fn spawn_both_solvers(&mut self) {
        let label1 = self.file1_path.display().to_string();
        let label2 = self.file2_path.display().to_string();
        let problem1 = Arc::clone(&self.problem1);
        let problem2 = Arc::clone(&self.problem2);

        self.solver.state = SolveState::RunningBoth { file1: label1, file2: label2, result1: None, result2: None };
        self.solver.view = SolveViewState::default();

        let (sender1, receiver1) = mpsc::channel();
        let (sender2, receiver2) = mpsc::channel();
        self.solver.receive = Some(receiver1);
        self.solver.receive2 = Some(receiver2);

        std::thread::spawn(move || {
            let result = crate::solver::solve_problem(&problem1);
            // Receiver may be dropped if the user dismissed the overlay — this is expected.
            if sender1.send(result).is_err() {
                eprintln!("solve result 1 dropped: receiver closed");
            }
        });
        std::thread::spawn(move || {
            let result = crate::solver::solve_problem(&problem2);
            // Receiver may be dropped if the user dismissed the overlay — this is expected.
            if sender2.send(result).is_err() {
                eprintln!("solve result 2 dropped: receiver closed");
            }
        });
    }

    /// Switch the solve popup to a different tab, preserving per-tab scroll position.
    const fn switch_solve_tab(&mut self, tab: SolveTab) {
        self.solver.view.tab = tab;
    }

    pub(crate) fn set_section(&mut self, section: Section) {
        self.record_jump();
        self.set_active_section(section);
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.reset_name_list_selection();
        self.detail_scroll = 0;
        self.focus = Focus::SectionSelector;
    }

    pub(crate) fn set_filter(&mut self, filter: DiffFilter) {
        if self.filter != filter {
            self.record_jump();
            self.filter = filter;
            self.invalidate_cache();
            self.ensure_active_section_cache();
            self.reset_name_list_selection();
        }
    }

    /// Handle a mouse event: scroll wheels and left-click panel selection.
    pub fn handle_mouse(&mut self, event: MouseEvent) {
        if self.search_popup.visible {
            return;
        }

        if self.show_help {
            if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.show_help = false;
            }
            return;
        }

        let column = event.column;
        let row = event.row;

        let over_section_selector = self.layout.section_selector.contains((column, row).into());
        let over_name_list = self.layout.name_list.contains((column, row).into());
        let over_detail = self.layout.detail.contains((column, row).into());

        match event.kind {
            MouseEventKind::ScrollDown => self.handle_mouse_scroll_down(over_section_selector, over_name_list, over_detail),
            MouseEventKind::ScrollUp => self.handle_mouse_scroll_up(over_section_selector, over_name_list, over_detail),
            MouseEventKind::Down(MouseButton::Left) => self.handle_mouse_click(over_section_selector, over_name_list, over_detail, row),
            _ => {}
        }
    }

    /// Scroll the name list by one step without touching focus.
    /// `down` controls direction: `true` moves selection down, `false` moves up.
    fn scroll_name_list(&mut self, down: bool) {
        let len = self.name_list_len();
        if len == 0 {
            return;
        }
        let state = self.active_name_list_state_mut();
        let current = state.selected().unwrap_or(0);
        let new = if down { (current + 1).min(len - 1) } else { current.saturating_sub(1) };
        state.select(Some(new));
        self.detail_scroll = 0;
    }

    fn handle_mouse_scroll_down(&mut self, over_section_selector: bool, over_name_list: bool, over_detail: bool) {
        if over_section_selector {
            self.navigate_down();
        } else if over_name_list {
            if self.has_name_list() && self.active_name_list_state_mut().selected().is_none() {
                self.active_name_list_state_mut().select(Some(0));
            }
            self.scroll_name_list(true);
        } else if over_detail {
            self.detail_scroll = self.detail_scroll.saturating_add(3);
        }
    }

    fn handle_mouse_scroll_up(&mut self, over_section_selector: bool, over_name_list: bool, over_detail: bool) {
        if over_section_selector {
            self.navigate_up();
        } else if over_name_list {
            self.scroll_name_list(false);
        } else if over_detail {
            self.detail_scroll = self.detail_scroll.saturating_sub(3);
        }
    }

    fn handle_mouse_click(&mut self, over_section_selector: bool, over_name_list: bool, over_detail: bool, row: u16) {
        if over_section_selector {
            self.focus = Focus::SectionSelector;
            let relative_row = row.saturating_sub(self.layout.section_selector.y + 1);
            let index = (relative_row as usize).min(Section::ALL.len() - 1);
            let new_section = Section::from_index(index);
            if self.active_section != new_section {
                self.set_section(new_section);
            }
        } else if over_name_list {
            self.focus = Focus::NameList;
            let len = self.name_list_len();
            if len > 0 {
                let relative_row = row.saturating_sub(self.layout.name_list.y + 1) as usize;
                let scroll_offset = self.active_name_list_state_mut().offset();
                let clicked_index = relative_row + scroll_offset;
                if clicked_index < len {
                    self.active_name_list_state_mut().select(Some(clicked_index));
                    self.detail_scroll = 0;
                }
            }
        } else if over_detail {
            self.focus = Focus::Detail;
        }
    }
}
