use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::state::{DiffFilter, Focus, Section};

impl App {
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C is an unconditional quit regardless of any other mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        if self.show_search_popup {
            self.handle_search_popup_key(key);
            return;
        }

        if self.show_help {
            // Any key dismisses the help pop-up.
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

    /// Handle a key event while the search pop-up is visible.
    fn handle_search_popup_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.show_search_popup = false;
            }
            KeyCode::Enter => {
                self.confirm_search_selection();
            }
            KeyCode::Backspace => {
                self.search_popup_query.pop();
                self.recompute_search_popup();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.search_popup_results.is_empty() {
                    self.search_popup_selected = (self.search_popup_selected + 1).min(self.search_popup_results.len() - 1);
                    self.search_popup_scroll = 0;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.search_popup_selected = self.search_popup_selected.saturating_sub(1);
                self.search_popup_scroll = 0;
            }
            KeyCode::Char(c) => {
                self.search_popup_query.push(c);
                self.recompute_search_popup();
            }
            _ => {}
        }
    }

    /// Handle a key event in normal (non-search) mode.
    fn handle_normal_key(&mut self, key: KeyEvent) {
        // Page-scroll keybindings (Ctrl+D/U = half-page, Ctrl+F/B = full-page).
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            let visible = match self.focus {
                Focus::NameList => self.name_list_height.saturating_sub(2) as usize, // subtract borders
                Focus::Detail => self.detail_height.saturating_sub(2) as usize,
                Focus::SectionSelector => 0,
            };
            match key.code {
                KeyCode::Char('d') => {
                    self.page_down(visible / 2);
                    return;
                }
                KeyCode::Char('u') => {
                    self.page_up(visible / 2);
                    return;
                }
                KeyCode::Char('f') => {
                    self.page_down(visible);
                    return;
                }
                KeyCode::Char('b') => {
                    self.page_up(visible);
                    return;
                }
                _ => {} // fall through to normal key handling
            }
        }

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

            // Navigation (vi-style and arrow keys).
            KeyCode::Char('j') | KeyCode::Down => self.navigate_down(),
            KeyCode::Char('k') | KeyCode::Up => self.navigate_up(),

            // n/N: search match navigation when a query is committed, otherwise up/down.
            KeyCode::Char('n') => {
                if self.search_query.is_empty() {
                    self.navigate_down();
                } else {
                    self.search_next();
                }
            }
            KeyCode::Char('N') => {
                if self.search_query.is_empty() {
                    self.navigate_up();
                } else {
                    self.search_prev();
                }
            }
            KeyCode::Char('g') | KeyCode::Home => self.jump_to_top(),
            KeyCode::Char('G') | KeyCode::End => self.jump_to_bottom(),

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

            // Toggle help pop-up.
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
            }

            // Yank (clipboard).
            KeyCode::Char('y') => self.yank_name(),
            KeyCode::Char('Y') => self.yank_detail(),

            // Open the search pop-up.
            KeyCode::Char('/') => {
                self.show_search_popup = true;
                self.search_popup_query.clear();
                self.search_popup_results.clear();
                self.search_popup_selected = 0;
                self.search_popup_scroll = 0;
                self.recompute_search_popup();
            }

            _ => {}
        }
    }

    /// Cycle focus: `SectionSelector` → `NameList` → Detail → `SectionSelector`.
    /// Skips `NameList` when the current section has no selectable entries.
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
                debug_assert!(len > 0, "name_list_len must be positive after early return");
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
                let len = self.name_list_len();
                if len == 0 {
                    return;
                }
                debug_assert!(len > 0, "name_list_len must be positive after early return");
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
                // Intentionally set to u16::MAX: ratatui's Paragraph::scroll clamps
                // to the actual content height, so this safely scrolls to the end
                // without needing to know the content height in advance.
                self.detail_scroll = u16::MAX;
            }
        }
    }

    /// Enter drops focus deeper: `SectionSelector` → `NameList` → Detail.
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

    pub(crate) fn set_section(&mut self, section: Section) {
        self.active_section = section;
        self.section_selector_state.select(Some(section.index()));
        self.invalidate_cache();
        self.ensure_active_section_cache();
        self.reset_name_list_selection();
        self.detail_scroll = 0;
        self.focus = Focus::SectionSelector;
    }

    pub(crate) fn set_filter(&mut self, filter: DiffFilter) {
        if self.filter != filter {
            self.filter = filter;
            self.invalidate_cache();
            self.ensure_active_section_cache();
            self.reset_name_list_selection();
        }
    }

    /// Handle a mouse event: scroll wheels and left-click panel selection.
    pub fn handle_mouse(&mut self, event: MouseEvent) {
        // Ignore mouse events while search pop-up is visible.
        if self.show_search_popup {
            return;
        }

        if self.show_help {
            // Any mouse interaction dismisses the help overlay.
            if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.show_help = false;
            }
            return;
        }

        let col = event.column;
        let row = event.row;

        // Determine which panel the mouse is over.
        let over_section_selector = self.section_selector_rect.contains((col, row).into());
        let over_name_list = self.name_list_rect.contains((col, row).into());
        let over_detail = self.detail_rect.contains((col, row).into());

        match event.kind {
            MouseEventKind::ScrollDown => {
                if over_section_selector {
                    self.navigate_down();
                } else if over_name_list {
                    // Temporarily set focus to NameList for navigation, then restore.
                    let prev_focus = self.focus;
                    self.focus = Focus::NameList;
                    if self.has_name_list() && self.active_name_list_state_mut().selected().is_none() {
                        self.active_name_list_state_mut().select(Some(0));
                    }
                    self.navigate_down();
                    self.focus = prev_focus;
                } else if over_detail {
                    self.detail_scroll = self.detail_scroll.saturating_add(3);
                }
            }
            MouseEventKind::ScrollUp => {
                if over_section_selector {
                    self.navigate_up();
                } else if over_name_list {
                    let prev_focus = self.focus;
                    self.focus = Focus::NameList;
                    self.navigate_up();
                    self.focus = prev_focus;
                } else if over_detail {
                    self.detail_scroll = self.detail_scroll.saturating_sub(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if over_section_selector {
                    self.focus = Focus::SectionSelector;
                    // Calculate which section was clicked (account for border).
                    let rel_y = row.saturating_sub(self.section_selector_rect.y + 1);
                    let idx = (rel_y as usize).min(Section::ALL.len() - 1);
                    let new_section = Section::from_index(idx);
                    if self.active_section != new_section {
                        self.set_section(new_section);
                    }
                } else if over_name_list {
                    self.focus = Focus::NameList;
                    let len = self.name_list_len();
                    if len > 0 {
                        // Calculate item index from click position: subtract border (1) from area.
                        let rel_y = row.saturating_sub(self.name_list_rect.y + 1) as usize;
                        let scroll_offset = self.active_name_list_state_mut().offset();
                        let clicked_idx = rel_y + scroll_offset;
                        if clicked_idx < len {
                            self.active_name_list_state_mut().select(Some(clicked_idx));
                            self.detail_scroll = 0;
                        }
                    }
                } else if over_detail {
                    self.focus = Focus::Detail;
                }
            }
            _ => {}
        }
    }
}
