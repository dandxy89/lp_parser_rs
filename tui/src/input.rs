use std::sync::{Arc, mpsc};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use lp_parser_rs::problem::LpProblem;

use crate::app::App;
use crate::detail_text::{format_solve_diff_result, format_solve_result};
use crate::state::{DiagnosisState, DiffFilter, Focus, PaletteCommand, PendingYank, Section, Side, SolveState, SolveTab, SolveViewState};

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

        if self.palette.visible {
            self.handle_palette_key(key);
            return;
        }

        if !matches!(self.solver.state, SolveState::Idle) {
            self.handle_solve_key(key);
            return;
        }

        if self.show_help {
            self.handle_help_key(key);
            return;
        }

        self.handle_normal_key(key);
    }

    /// Handle a key event while the help overlay is visible.
    ///
    /// Navigation keys scroll the help text (it can exceed the screen on small
    /// terminals); any other key dismisses the overlay. `G`'s `u16::MAX` is
    /// clamped to the real content height when the overlay is drawn.
    const fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.help_scroll = self.help_scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => self.help_scroll = self.help_scroll.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.help_scroll = 0,
            KeyCode::Char('G') | KeyCode::End => self.help_scroll = u16::MAX,
            _ => {
                self.show_help = false;
                self.help_scroll = 0;
            }
        }
    }

    /// Open the command palette, resetting its query and filtered list.
    pub(crate) fn open_command_palette(&mut self) {
        self.palette.visible = true;
        self.palette.query.clear();
        self.palette.selected = 0;
        self.recompute_palette();
    }

    /// Recompute the palette's filtered command list from the current query.
    fn recompute_palette(&mut self) {
        self.palette.selected = 0;
        self.palette.filtered.clear();
        if self.palette.query.is_empty() {
            self.palette.filtered.extend(0..PaletteCommand::ALL.len());
            return;
        }
        let labels: Vec<String> = PaletteCommand::ALL.iter().map(|c| c.label().to_owned()).collect();
        let config = frizbee::Config { sort: true, ..Default::default() };
        for matched in frizbee::match_list_indices(&self.palette.query, &labels, &config) {
            self.palette.filtered.push(matched.index as usize);
        }
    }

    /// Handle a key event while the command palette is visible.
    fn handle_palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.palette.visible = false,
            KeyCode::Enter => self.confirm_palette(),
            KeyCode::Backspace => {
                self.palette.query.pop();
                self.recompute_palette();
            }
            KeyCode::Down => {
                if !self.palette.filtered.is_empty() {
                    self.palette.selected = (self.palette.selected + 1).min(self.palette.filtered.len() - 1);
                }
            }
            KeyCode::Up => self.palette.selected = self.palette.selected.saturating_sub(1),
            KeyCode::Char(character) => {
                self.palette.query.push(character);
                self.recompute_palette();
            }
            _ => {}
        }
    }

    /// Execute the highlighted palette command and close the palette.
    fn confirm_palette(&mut self) {
        let command = self.palette.filtered.get(self.palette.selected).map(|&i| PaletteCommand::ALL[i]);
        self.palette.visible = false;
        if let Some(command) = command {
            self.run_palette_command(command);
        }
    }

    /// Dispatch a palette command to the same handler as its direct keybinding.
    fn run_palette_command(&mut self, command: PaletteCommand) {
        match command {
            PaletteCommand::GoSummary => self.set_section(Section::Summary),
            PaletteCommand::GoVariables => self.set_section(Section::Variables),
            PaletteCommand::GoConstraints => self.set_section(Section::Constraints),
            PaletteCommand::GoObjectives => self.set_section(Section::Objectives),
            PaletteCommand::GoNumerics => self.set_section(Section::Numerics),
            PaletteCommand::FilterAll => self.set_filter(DiffFilter::All),
            PaletteCommand::FilterAdded => self.set_filter(DiffFilter::Added),
            PaletteCommand::FilterRemoved => self.set_filter(DiffFilter::Removed),
            PaletteCommand::FilterModified => self.set_filter(DiffFilter::Modified),
            PaletteCommand::FilterRenamed => self.set_filter(DiffFilter::Renamed),
            PaletteCommand::ToggleRawView => self.toggle_detail_view(),
            PaletteCommand::ToggleIgnoreOrder => self.toggle_ignore_order(),
            PaletteCommand::CycleSort => self.cycle_sort_mode(),
            PaletteCommand::CycleRelTol => self.cycle_rel_tol(),
            PaletteCommand::CycleAbsTol => self.cycle_abs_tol(),
            PaletteCommand::OpenSearch => self.open_search_popup(),
            PaletteCommand::Solve => self.solver.state = SolveState::Picking,
            PaletteCommand::ExportCsv => self.export_csv(),
            PaletteCommand::YankName => self.yank_name(),
            PaletteCommand::YankOld => self.yank_side(Side::Old),
            PaletteCommand::YankNew => self.yank_side(Side::New),
            PaletteCommand::YankDetail => self.yank_detail(),
            PaletteCommand::ShowHelp => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            PaletteCommand::Quit => self.should_quit = true,
        }
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
            KeyCode::Char('j') | KeyCode::Down if !self.search_popup.results.is_empty() => {
                self.search_popup.selected = (self.search_popup.selected + 1).min(self.search_popup.results.len() - 1);
                self.search_popup.scroll = 0;
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
            // With only two cycle targets, backward equals forward.
            KeyCode::Tab | KeyCode::BackTab => self.cycle_focus_forward(),

            // Direct section jump.
            KeyCode::Char('1') => self.set_section(Section::Summary),
            KeyCode::Char('2') => self.set_section(Section::Variables),
            KeyCode::Char('3') => self.set_section(Section::Constraints),
            KeyCode::Char('4') => self.set_section(Section::Objectives),
            KeyCode::Char('5') => self.set_section(Section::Numerics),

            // Cycle sections from any focus (lazygit-style sub-tab navigation).
            KeyCode::Char(']') => self.cycle_section(true),
            KeyCode::Char('[') => self.cycle_section(false),

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
            KeyCode::Char('=') => self.set_filter(DiffFilter::Renamed),

            // Cycle the sidebar sort mode (name → |Δ| → relΔ).
            KeyCode::Char('s') => self.cycle_sort_mode(),

            // Live tolerance adjustment — rebuilds the diff in place.
            KeyCode::Char('t') => self.cycle_rel_tol(),
            KeyCode::Char('T') => self.cycle_abs_tol(),

            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                self.help_scroll = 0;
            }

            // Yank (clipboard): `y` begins a chord, `Y` yanks detail immediately.
            KeyCode::Char('y') => self.pending_yank = PendingYank::WaitingForTarget,
            KeyCode::Char('Y') => self.yank_detail(),

            // Open the solver file picker.
            KeyCode::Char('S') => self.solver.state = SolveState::Picking,

            // Export the diff report as CSV.
            KeyCode::Char('w') => self.export_csv(),

            // Toggle raw text side-by-side diff view.
            KeyCode::Char('r') => self.toggle_detail_view(),

            // Toggle hiding order-only coefficient changes.
            KeyCode::Char('o') => self.toggle_ignore_order(),

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
            KeyCode::Char('p') => self.open_command_palette(),
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

    /// Cycle the active section forward (`true`) or backward (`false`), wrapping
    /// around. Bound to `]` / `[` and reachable from any focus, so changing
    /// section never requires first parking focus on the tab bar.
    fn cycle_section(&mut self, forward: bool) {
        let count = Section::ALL.len();
        let current = self.active_section.index();
        let next = if forward { (current + 1) % count } else { (current + count - 1) % count };
        self.set_section(Section::from_index(next));
    }

    /// Move focus into the section's content after a section change: the name
    /// list when it has entries, otherwise the detail panel. This is the
    /// ergonomic win — after `2`/`]` you land on the variable list and `j`/`k`
    /// scroll it immediately, instead of parking on the tab bar.
    fn focus_section_content(&mut self) {
        if self.has_name_list() {
            if self.active_name_list_state_mut().selected().is_none() {
                self.active_name_list_state_mut().select(Some(0));
            }
            self.focus = Focus::NameList;
        } else {
            self.focus = Focus::Detail;
        }
        self.detail_scroll = 0;
    }

    /// Toggle focus between the name list and the detail panel.
    ///
    /// The tab bar (`SectionSelector`) is no longer part of the `Tab` cycle —
    /// sections are switched with `1`–`5` or `[`/`]`. A click on the tab bar can
    /// still leave focus there; `Tab` then moves into the content.
    fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::NameList => {
                self.detail_scroll = 0;
                Focus::Detail
            }
            Focus::Detail | Focus::SectionSelector => {
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
                let new_section = Section::Numerics;
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
                } else if matches!(self.active_section, Section::Summary | Section::Numerics) {
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
            KeyCode::Esc => {
                self.solver.state = SolveState::Idle;
                self.solver.reset_diagnosis();
            }
            KeyCode::Char('1') => self.switch_solve_tab(SolveTab::Summary),
            KeyCode::Char('2') => self.switch_solve_tab(SolveTab::Variables),
            KeyCode::Char('3') => self.switch_solve_tab(SolveTab::Constraints),
            KeyCode::Char('4') => self.switch_solve_tab(SolveTab::Log),
            KeyCode::Char('5') => self.switch_solve_tab(SolveTab::Duals),
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
        match key.code {
            KeyCode::Char('y') => {
                if let SolveState::Done(result) = &self.solver.state {
                    let text = format_solve_result(result);
                    self.set_yank_flash("Yanked solve results", &text);
                }
            }
            KeyCode::Char('e') => self.start_diagnosis_single(),
            _ => {}
        }
    }

    /// Start an infeasibility diagnosis for the single-solve result, if it is
    /// infeasible and no diagnosis is already running or complete.
    fn start_diagnosis_single(&mut self) {
        if !matches!(self.solver.diagnosis, DiagnosisState::Idle | DiagnosisState::Failed(_)) {
            return;
        }
        let SolveState::Done(result) = &self.solver.state else {
            return;
        };
        if !crate::solver::status_is_infeasible(&result.status) {
            return;
        }
        let Some(problem) = self.solver.solved_problem.clone() else {
            self.solver.diagnosis = DiagnosisState::Failed("No problem retained for diagnosis".to_owned());
            return;
        };
        // The solved problem is one of the two app problems; label it accordingly.
        let label = if Arc::ptr_eq(&problem, &self.problem2) {
            self.file2_path.display().to_string()
        } else {
            self.file1_path.display().to_string()
        };
        self.spawn_diagnosis(problem, label);
    }

    /// Start an infeasibility diagnosis in the comparison view.
    ///
    /// Diagnoses whichever side is infeasible; when both sides are infeasible,
    /// file 1 is diagnosed.
    fn start_diagnosis_both(&mut self) {
        if !matches!(self.solver.diagnosis, DiagnosisState::Idle | DiagnosisState::Failed(_)) {
            return;
        }
        let SolveState::DoneBoth(diff) = &self.solver.state else {
            return;
        };
        let (problem, label) = if crate::solver::status_is_infeasible(&diff.result1.status) {
            (Arc::clone(&self.problem1), self.file1_path.display().to_string())
        } else if crate::solver::status_is_infeasible(&diff.result2.status) {
            (Arc::clone(&self.problem2), self.file2_path.display().to_string())
        } else {
            return; // neither side is infeasible — nothing to diagnose
        };
        self.spawn_diagnosis(problem, label);
    }

    /// Spawn the elastic-relaxation diagnosis in a background thread,
    /// mirroring the `spawn_solver` mpsc pattern so the UI never blocks.
    fn spawn_diagnosis(&mut self, problem: Arc<LpProblem>, file_label: String) {
        debug_assert!(
            !matches!(self.solver.diagnosis, DiagnosisState::Running { .. }),
            "spawn_diagnosis called while a diagnosis is already running"
        );
        self.solver.diagnosis = DiagnosisState::Running { file: file_label, started: Instant::now() };

        let (sender, receiver) = mpsc::channel();
        self.solver.receive_diagnosis = Some(receiver);

        std::thread::spawn(move || {
            let result = crate::solver::diagnose_infeasibility(&problem);
            // Receiver may be dropped if the user dismissed the overlay — this is expected.
            if sender.send(result).is_err() {
                eprintln!("diagnosis result dropped: receiver closed");
            }
        });
    }

    /// Spawn the solver in a background thread for the given problem.
    fn spawn_solver(&mut self, problem: Arc<LpProblem>, file_label: String) {
        self.solver.state = SolveState::Running { file: file_label, started: Instant::now() };
        self.solver.view = SolveViewState::default();
        self.solver.reset_diagnosis();
        self.solver.solved_problem = Some(Arc::clone(&problem));

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
            KeyCode::Char('e') => self.start_diagnosis_both(),
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

        self.solver.state = SolveState::RunningBoth { file1: label1, file2: label2, result1: None, result2: None, started: Instant::now() };
        self.solver.view = SolveViewState::default();
        self.solver.reset_diagnosis();
        self.solver.solved_problem = None;

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
        // Land focus on the section's content so navigation keys act on it
        // immediately, rather than on the tab bar.
        self.focus_section_content();
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
        if self.search_popup.visible || self.palette.visible {
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
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_click(over_section_selector, over_name_list, over_detail, column, row);
            }
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

    fn handle_mouse_click(&mut self, over_section_selector: bool, over_name_list: bool, over_detail: bool, column: u16, row: u16) {
        if over_section_selector {
            self.focus = Focus::SectionSelector;
            // The tab bar is a single row: map the click column to a tab.
            if let Some(index) = self.layout.tab_bounds.iter().position(|&(start, end)| column >= start && column < end) {
                let new_section = Section::from_index(index);
                if self.active_section != new_section {
                    self.set_section(new_section);
                }
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
