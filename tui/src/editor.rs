//! A small modal (Vim-style) source editor over [`lp_lsp::Document`].
//!
//! Supported subset, fixed up front so it does not grow unbounded:
//!
//! - Normal: `h j k l` `w b` `0 ^ $` `gg G` `Ctrl-d/u/f/b`, `x` `dd` `yy` `p P`
//!   `o O` `i a I A`, `u` `Ctrl-r`, `/ n N`, `:`, and (LP only) `gd` `gr` `K`
//!   `]d [d`.
//! - Insert: text, Enter (keeps indentation), Backspace, Delete, arrows, Tab,
//!   `Ctrl-n` for completion (LP only), Esc.
//! - Command: `:w` `:q` `:q!` `:wq` `:x`, and (LP only) `:format`, `:rename NAME`,
//!   `:actions`.
//!
//! Not supported: counts, text objects, visual mode, registers, dot-repeat.
//!
//! The buffer is the language server's own document, so every feature runs on
//! exactly what is on screen. Columns are bytes (`Encoding::Utf8`); the cursor
//! only ever rests on character boundaries. Files that are not LP (MPS) get
//! plain editing: the grammar does not cover them.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::editor_mps::{Kind, MpsIndex};
use lp_lsp::config::FormatSettings;
use lp_lsp::semantic::SemanticResult;
use lp_lsp::{Config, Document, Encoding};
use ls_types::{CodeActionContext, CodeActionOrCommand, Diagnostic, DiagnosticSeverity, Position, TextEdit, Uri, WorkspaceEdit};

/// Quiet time after the last edit before the semantic pass runs.
const SEMANTIC_DEBOUNCE: Duration = Duration::from_millis(300);
/// Above this size the live syntax diagnostics are skipped (recomputed on save).
const LIVE_DIAGNOSTICS_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    /// `:` command line.
    Command,
    /// `/` search prompt.
    Search,
}

/// What the app should do after a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Close the editor and select this entry (by name) in the parsed view.
    SelectEntry(String),
}

/// A list or text popup over the source.
#[derive(Debug, Default)]
pub struct Popup {
    pub title: String,
    pub lines: Vec<String>,
    /// For list popups, the byte offset each line jumps to; empty for text.
    pub targets: Vec<usize>,
    /// For action popups, the edits each line applies.
    pub edits: Vec<Vec<(Range<usize>, String)>>,
    pub selected: usize,
}

/// One undoable change: `removed` was replaced by `inserted` at `at`.
#[derive(Debug, Clone)]
struct Edit {
    at: usize,
    removed: String,
    inserted: String,
    cursor_before: usize,
}

/// A saved snapshot of the document for atomic multi-edit undo.
type Group = Vec<Edit>;

#[allow(clippy::struct_excessive_bools)] // independent state flags, as in the server's Config
pub struct Editor {
    pub doc: Document,
    pub path: PathBuf,
    /// Language features available (LP files only).
    pub lsp: bool,
    /// An MPS file: name navigation and rename from a line-oriented index.
    pub mps: bool,
    mps_index: Option<(i32, MpsIndex)>,
    pub cursor: usize,
    /// Preferred display column for vertical motion.
    want_col: usize,
    /// First visible line.
    pub top: usize,
    pub mode: Mode,
    /// First key of a two-key command (`g`, `d`, `y`, `]`, `[`).
    pending: Option<char>,
    /// The `:` or `/` line being typed.
    pub prompt: String,
    pub search: Option<String>,
    register: String,
    register_linewise: bool,
    undo: Vec<Group>,
    redo: Vec<Group>,
    /// Edits of the current insert session, undone as one step.
    insert_group: Group,
    pub dirty: bool,
    /// Set after each successful save; the app takes it to reload the parsed model.
    pub saved: bool,
    /// Visible text rows, written by the renderer.
    pub height: usize,
    pub message: Option<String>,
    pub popup: Option<Popup>,
    pub diagnostics: Vec<Diagnostic>,
    config: Config,
    version: i32,
    /// When the buffer last changed and the semantic pass has not caught up.
    stale_since: Option<Instant>,
    semantic: Option<mpsc::Receiver<SemanticResult>>,
}

impl Editor {
    /// Open `text` (the file's contents) for editing. `lsp` enables language features.
    pub fn open(path: &Path, text: &str, lsp: bool) -> Self {
        let uri: Uri = "file:///source.lp".parse().expect("constant URI parses");
        let doc = Document::new(uri, text.to_owned(), 1, Encoding::Utf8);
        let mut editor = Self {
            doc,
            path: path.to_owned(),
            lsp,
            mps: !lsp && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("mps")),
            mps_index: None,
            cursor: 0,
            want_col: 0,
            top: 0,
            mode: Mode::Normal,
            pending: None,
            prompt: String::new(),
            search: None,
            register: String::new(),
            register_linewise: false,
            undo: Vec::new(),
            redo: Vec::new(),
            insert_group: Vec::new(),
            dirty: false,
            saved: false,
            height: 24,
            message: None,
            popup: None,
            diagnostics: Vec::new(),
            config: Config::default(),
            version: 1,
            stale_since: None,
            semantic: None,
        };
        if editor.lsp {
            editor.stale_since = Instant::now().checked_sub(SEMANTIC_DEBOUNCE);
        }
        editor.refresh_diagnostics();
        editor
    }

    // ----- text geometry -----

    fn text(&self) -> &str {
        &self.doc.text
    }

    /// Lines worth showing: a final newline does not start a visible line.
    pub fn line_count(&self) -> usize {
        let lines = self.doc.lines.line_count();
        if self.text().ends_with('\n') && lines > 1 { lines - 1 } else { lines }
    }

    pub fn cursor_line(&self) -> usize {
        self.doc.lines.line_of(self.cursor)
    }

    fn line_range(&self, line: usize) -> Range<usize> {
        self.doc.lines.line_range(self.text(), line)
    }

    /// Display column of byte `offset` within its line.
    pub fn column_of(&self, offset: usize) -> usize {
        let start = self.line_range(self.doc.lines.line_of(offset)).start;
        unicode_width::UnicodeWidthStr::width(&self.text()[start..offset])
    }

    pub fn cursor_column(&self) -> usize {
        self.column_of(self.cursor)
    }

    /// The byte offset on `line` nearest display column `column` (never past the last character in Normal mode).
    fn offset_at_column(&self, line: usize, column: usize) -> usize {
        let range = self.line_range(line);
        let mut width = 0;
        let mut last = range.start;
        for (index, ch) in self.text()[range.clone()].char_indices() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if width + w > column {
                return range.start + index;
            }
            width += w;
            last = range.start + index;
        }
        if self.mode == Mode::Insert { range.end } else { last }
    }

    fn next_char(&self, offset: usize) -> usize {
        self.text()[offset..].chars().next().map_or(offset, |c| offset + c.len_utf8())
    }

    fn prev_char(&self, offset: usize) -> usize {
        self.text()[..offset].chars().next_back().map_or(offset, |c| offset - c.len_utf8())
    }

    pub fn set_cursor(&mut self, offset: usize) {
        debug_assert!(offset <= self.text().len(), "cursor beyond the text");
        self.cursor = offset;
        self.want_col = self.cursor_column();
    }

    /// Move the cursor to `offset` and scroll it into view.
    pub fn goto_offset(&mut self, offset: usize, height: usize) {
        let mut offset = offset.min(self.text().len());
        while !self.text().is_char_boundary(offset) {
            offset -= 1;
        }
        self.set_cursor(offset);
        self.scroll_to_cursor(height);
    }

    /// Keep the cursor inside the window of `height` lines.
    pub fn scroll_to_cursor(&mut self, height: usize) {
        let line = self.cursor_line();
        if height == 0 {
            return;
        }
        if line < self.top {
            self.top = line;
        } else if line >= self.top + height {
            self.top = line + 1 - height;
        }
    }

    fn position(&self, offset: usize) -> Position {
        self.doc.position(offset)
    }

    // ----- edits -----

    /// Replace `range` with `text`, recording the change for undo.
    fn apply(&mut self, range: Range<usize>, text: &str) {
        let edit = Edit { at: range.start, removed: self.text()[range.clone()].to_owned(), inserted: text.to_owned(), cursor_before: self.cursor };
        self.version += 1;
        self.doc.replace(range, text, self.version);
        self.insert_group.push(edit);
        self.after_change();
    }

    fn after_change(&mut self) {
        self.dirty = true;
        self.redo.clear();
        self.stale_since = (self.lsp || self.mps).then(Instant::now);
        if self.lsp && self.text().len() <= LIVE_DIAGNOSTICS_LIMIT {
            self.refresh_diagnostics();
        }
    }

    /// Close the current group of edits as one undo step.
    fn commit_group(&mut self) {
        if !self.insert_group.is_empty() {
            self.undo.push(std::mem::take(&mut self.insert_group));
        }
    }

    fn undo(&mut self) {
        self.commit_group();
        let Some(group) = self.undo.pop() else {
            self.message = Some("Already at oldest change".to_owned());
            return;
        };
        for edit in group.iter().rev() {
            self.version += 1;
            self.doc.replace(edit.at..edit.at + edit.inserted.len(), &edit.removed, self.version);
        }
        let cursor = group.first().map_or(0, |edit| edit.cursor_before);
        self.redo.push(group);
        self.finish_history_step(cursor);
    }

    fn redo(&mut self) {
        let Some(group) = self.redo.pop() else {
            self.message = Some("Already at newest change".to_owned());
            return;
        };
        for edit in &group {
            self.version += 1;
            self.doc.replace(edit.at..edit.at + edit.removed.len(), &edit.inserted, self.version);
        }
        let cursor = group.last().map_or(0, |edit| edit.at + edit.inserted.len());
        self.undo.push(group);
        self.finish_history_step(cursor);
    }

    fn finish_history_step(&mut self, cursor: usize) {
        self.dirty = true;
        self.stale_since = (self.lsp || self.mps).then(Instant::now);
        self.refresh_diagnostics();
        self.set_cursor(cursor.min(self.text().len()));
        self.clamp_normal_cursor();
    }

    /// In Normal mode the cursor rests on a character, not past the line end.
    fn clamp_normal_cursor(&mut self) {
        let range = self.line_range(self.cursor_line());
        if self.mode != Mode::Insert && self.cursor >= range.end && range.end > range.start {
            self.cursor = self.prev_char(range.end);
        }
        self.want_col = self.cursor_column();
    }

    // ----- language features -----

    fn refresh_diagnostics(&mut self) {
        if self.lsp {
            self.diagnostics = lp_lsp::features::diagnostics::compute(&self.doc, &self.config);
        } else if self.mps {
            // The only diagnostic MPS gets: where the real parser stops, if it does.
            self.diagnostics = crate::editor_mps::parse_error(self.text())
                .map(|(offset, message)| Diagnostic {
                    range: self.doc.range(offset..offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message,
                    ..Diagnostic::default()
                })
                .into_iter()
                .collect();
        }
    }

    /// The name index for the current text, rebuilt after edits.
    fn mps_index(&mut self) -> &MpsIndex {
        if self.mps_index.as_ref().is_none_or(|(version, _)| *version != self.version) {
            self.mps_index = Some((self.version, MpsIndex::build(&self.doc.text)));
        }
        &self.mps_index.as_ref().expect("built above").1
    }

    /// Start or collect the debounced semantic pass. Call on every tick.
    pub fn tick(&mut self) {
        if let Some(receiver) = &self.semantic {
            match receiver.try_recv() {
                Ok(result) => {
                    self.semantic = None;
                    if result.version == self.doc.version {
                        self.doc.semantic_result = Some(Arc::new(result));
                        self.refresh_diagnostics();
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.semantic = None,
            }
        }
        let Some(since) = self.stale_since else { return };
        if self.mps {
            // The MPS check is a full parse, so it waits for a pause and skips huge files.
            if since.elapsed() >= SEMANTIC_DEBOUNCE {
                self.stale_since = None;
                if self.text().len() <= LIVE_DIAGNOSTICS_LIMIT {
                    self.refresh_diagnostics();
                }
            }
            return;
        }
        if self.semantic.is_some() || since.elapsed() < SEMANTIC_DEBOUNCE || self.text().len() > 20 * 1024 * 1024 {
            return;
        }
        self.stale_since = None;
        let (text, version) = (self.text().to_owned(), self.doc.version);
        let analysis = self.config.analysis.thresholds.clone();
        let (sender, receiver) = mpsc::channel();
        self.semantic = Some(receiver);
        std::thread::spawn(move || {
            // The editor may have closed meanwhile; a failed send is fine.
            drop(sender.send(lp_lsp::semantic::run(&text, version, &analysis)));
        });
    }

    /// Whether the semantic pass is pending or running (keeps the UI ticking).
    pub const fn is_busy(&self) -> bool {
        self.semantic.is_some() || self.stale_since.is_some()
    }

    fn need_lsp(&mut self) -> bool {
        if !self.lsp {
            let hint = if self.mps { "this feature is only available for LP files" } else { "language features are only available for LP files" };
            self.message = Some(hint.to_owned());
        }
        self.lsp
    }

    fn go_definition(&mut self, height: usize) {
        if self.mps {
            let cursor = self.cursor;
            let index = self.mps_index();
            let target = index.site_at(cursor).and_then(|site| index.definition(site)).map(|def| def.range.start);
            match target {
                Some(offset) => self.goto_offset(offset, height),
                None => self.message = Some("No definition found".to_owned()),
            }
            return;
        }
        if !self.need_lsp() {
            return;
        }
        match lp_lsp::features::navigation::definition(&self.doc, self.position(self.cursor)) {
            Some(location) => {
                let start = self.doc.byte_range(location.range).start;
                self.goto_offset(start, height);
            }
            None => self.message = Some("No definition found".to_owned()),
        }
    }

    fn list_references(&mut self) {
        if self.mps {
            let cursor = self.cursor;
            let index = self.mps_index();
            let ranges: Vec<(Range<usize>, &'static str)> =
                index.site_at(cursor).map(|site| index.occurrences(site).into_iter().map(|s| (s.range.clone(), s.section)).collect()).unwrap_or_default();
            if ranges.is_empty() {
                self.message = Some("No references found".to_owned());
                return;
            }
            let mut popup = Popup { title: format!("{} references", ranges.len()), ..Popup::default() };
            for (range, section) in ranges {
                let line = self.doc.lines.line_of(range.start);
                popup.lines.push(format!("{:>5}  {section:<8} {}", line + 1, self.text()[self.line_range(line)].trim()));
                popup.targets.push(range.start);
            }
            self.popup = Some(popup);
            return;
        }
        if !self.need_lsp() {
            return;
        }
        let locations = lp_lsp::features::navigation::references(&self.doc, self.position(self.cursor), true);
        if locations.is_empty() {
            self.message = Some("No references found".to_owned());
            return;
        }
        let mut popup = Popup { title: format!("{} references", locations.len()), ..Popup::default() };
        for location in locations {
            let range = self.doc.byte_range(location.range);
            let line = self.doc.lines.line_of(range.start);
            let content = self.text()[self.line_range(line)].trim().to_owned();
            popup.lines.push(format!("{:>5}  {content}", line + 1));
            popup.targets.push(range.start);
        }
        self.popup = Some(popup);
    }

    fn hover(&mut self) {
        if self.mps {
            let cursor = self.cursor;
            let index = self.mps_index();
            let text = index.site_at(cursor).map(|site| {
                let uses = index.occurrences(site);
                let defined = index.definition(site).map(|def| def.section);
                format!("{} `{}`\ndefined in {}\n{} occurrences", site.kind.label(), site.name, defined.unwrap_or("(no definition)"), uses.len())
            });
            match text {
                Some(text) => self.popup = Some(Popup { title: "Hover".to_owned(), lines: text.lines().map(str::to_owned).collect(), ..Popup::default() }),
                None => self.message = Some("Nothing to show here".to_owned()),
            }
            return;
        }
        if !self.need_lsp() {
            return;
        }
        match lp_lsp::features::hover::hover(&self.doc, self.position(self.cursor)) {
            Some(hover) => {
                let value = match hover.contents {
                    ls_types::HoverContents::Markup(markup) => markup.value,
                    ls_types::HoverContents::Scalar(scalar) => match scalar {
                        ls_types::MarkedString::String(text) => text,
                        ls_types::MarkedString::LanguageString(language) => language.value,
                    },
                    ls_types::HoverContents::Array(items) => items
                        .into_iter()
                        .map(|item| match item {
                            ls_types::MarkedString::String(text) => text,
                            ls_types::MarkedString::LanguageString(language) => language.value,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                };
                self.popup = Some(Popup { title: "Hover".to_owned(), lines: value.lines().map(str::to_owned).collect(), ..Popup::default() });
            }
            None => self.message = Some("Nothing to show here".to_owned()),
        }
    }

    /// `]d` / `[d`: the next or previous diagnostic.
    fn step_diagnostic(&mut self, forward: bool, height: usize) {
        let mut starts: Vec<usize> = self.diagnostics.iter().map(|d| self.doc.byte_range(d.range).start).collect();
        starts.sort_unstable();
        starts.dedup();
        let target = if forward { starts.iter().copied().find(|&s| s > self.cursor) } else { starts.iter().copied().rfind(|&s| s < self.cursor) };
        match target.or_else(|| if forward { starts.first().copied() } else { starts.last().copied() }) {
            Some(offset) => {
                self.goto_offset(offset, height);
                if let Some(diagnostic) = self.diagnostics.iter().find(|d| self.doc.byte_range(d.range).start == offset) {
                    self.message = Some(diagnostic.message.lines().next().unwrap_or_default().to_owned());
                }
            }
            None => self.message = Some("No diagnostics".to_owned()),
        }
    }

    fn open_completion(&mut self) {
        if !self.need_lsp() {
            return;
        }
        let list = lp_lsp::features::completion::complete(&self.doc, self.position(self.cursor));
        let mut popup = Popup { title: "Completion".to_owned(), ..Popup::default() };
        for item in list.items.into_iter().take(40) {
            let replacement = match &item.text_edit {
                Some(ls_types::CompletionTextEdit::Edit(edit)) => Some((self.doc.byte_range(edit.range), edit.new_text.clone())),
                _ => None,
            };
            let insert = item.insert_text.clone().unwrap_or_else(|| item.label.clone());
            let (range, text) = replacement.unwrap_or((self.cursor..self.cursor, insert));
            popup.lines.push(item.label);
            popup.edits.push(vec![(range, text)]);
        }
        if popup.lines.is_empty() {
            self.message = Some("No completions".to_owned());
        } else {
            self.popup = Some(popup);
        }
    }

    fn open_actions(&mut self) {
        if !self.need_lsp() {
            return;
        }
        let range = self.doc.range(self.cursor..self.next_char(self.cursor));
        let context = CodeActionContext { diagnostics: self.diagnostics.clone(), only: None, trigger_kind: None };
        let mut popup = Popup { title: "Code actions".to_owned(), ..Popup::default() };
        for action in lp_lsp::features::code_action::actions(&self.doc, range, &context, false) {
            if let CodeActionOrCommand::CodeAction(action) = action
                && let Some(edit) = action.edit
            {
                popup.lines.push(action.title);
                popup.edits.push(self.flatten_edit(&edit));
            }
        }
        if popup.lines.is_empty() {
            self.message = Some("No code actions here".to_owned());
        } else {
            self.popup = Some(popup);
        }
    }

    /// The byte-range edits of a single-document `WorkspaceEdit`.
    fn flatten_edit(&self, edit: &WorkspaceEdit) -> Vec<(Range<usize>, String)> {
        let mut out: Vec<(Range<usize>, String)> = edit
            .changes
            .iter()
            .flatten()
            .flat_map(|(_, edits)| edits.iter().map(|e: &TextEdit| (self.doc.byte_range(e.range), e.new_text.clone())))
            .collect();
        // Back to front, so earlier edits do not shift later ranges.
        out.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        out
    }

    fn apply_edits(&mut self, edits: &[(Range<usize>, String)]) {
        self.commit_group();
        for (range, text) in edits {
            self.apply(range.clone(), text);
        }
        self.commit_group();
        let clamp = self.cursor.min(self.text().len());
        self.set_cursor(clamp);
        self.clamp_normal_cursor();
    }

    fn rename(&mut self, new_name: &str) {
        if self.mps {
            self.rename_mps(new_name);
            return;
        }
        if !self.need_lsp() {
            return;
        }
        match lp_lsp::features::rename::rename(&self.doc, &[], self.position(self.cursor), new_name) {
            Ok(Some(edit)) => {
                let edits = self.flatten_edit(&edit);
                let count = edits.len();
                self.apply_edits(&edits);
                self.message = Some(format!("Renamed in {count} places"));
            }
            Ok(None) => self.message = Some("Name unchanged".to_owned()),
            Err(error) => self.message = Some(error),
        }
    }

    /// Rename the row or column under the cursor everywhere it occurs.
    fn rename_mps(&mut self, new_name: &str) {
        if new_name.is_empty() || new_name.contains(char::is_whitespace) {
            self.message = Some("A name cannot be empty or contain blanks".to_owned());
            return;
        }
        let cursor = self.cursor;
        let index = self.mps_index();
        let Some(site) = index.site_at(cursor).cloned() else {
            self.message = Some("Put the cursor on a row or column name".to_owned());
            return;
        };
        if site.name == new_name {
            self.message = Some("Name unchanged".to_owned());
            return;
        }
        if index.contains(site.kind, new_name) {
            self.message = Some(format!("A {} named `{new_name}` already exists", site.kind.label()));
            return;
        }
        let mut edits: Vec<(Range<usize>, String)> = index.occurrences(&site).into_iter().map(|s| (s.range.clone(), new_name.to_owned())).collect();
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        let count = edits.len();
        self.apply_edits(&edits);
        self.message = Some(format!("Renamed in {count} places"));
    }

    fn format(&mut self) {
        if !self.need_lsp() {
            return;
        }
        let settings: FormatSettings = self.config.format.clone();
        match lp_lsp::features::format::format_text(self.text(), &settings) {
            Some(formatted) if formatted != self.text() => {
                let len = self.text().len();
                self.apply_edits(&[(0..len, formatted)]);
                self.message = Some("Formatted".to_owned());
            }
            Some(_) => self.message = Some("Already formatted".to_owned()),
            None => self.message = Some("Cannot format: the file has syntax errors".to_owned()),
        }
    }

    /// Put the cursor on the definition of `name`: a labelled objective or
    /// constraint, else the first whole-word occurrence (variables).
    pub fn goto_name(&mut self, name: &str) {
        if self.mps {
            let index = self.mps_index();
            let found = [Kind::Row, Kind::Column].into_iter().find_map(|kind| {
                let site = index.occurrences_of(kind, name).into_iter().find(|s| matches!(s.section, "ROWS" | "COLUMNS"))?;
                Some(site.range.start)
            });
            if let Some(offset) = found {
                self.goto_offset(offset, self.height);
            }
            return;
        }
        let labelled = self.doc.index().entities.iter().find(|e| e.name.as_deref() == Some(name)).and_then(|e| e.name_range.clone());
        let found = labelled.map(|range| range.start).or_else(|| {
            let text = self.text();
            text.match_indices(name).map(|(at, _)| at).find(|&at| {
                let before = text[..at].chars().next_back();
                let after = text[at + name.len()..].chars().next();
                let word = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
                !before.is_some_and(word) && !after.is_some_and(word)
            })
        });
        match found {
            Some(offset) => self.goto_offset(offset, self.height),
            None => self.message = Some(format!("`{name}` not found in the source")),
        }
    }

    /// The name under the cursor, for jumping back to the parsed view.
    fn entry_under_cursor(&mut self) -> Option<String> {
        if self.mps {
            let cursor = self.cursor;
            return self.mps_index().site_at(cursor).map(|site| site.name.clone());
        }
        let index = self.doc.index();
        let offset = self.cursor;
        let from_entity = index.entities.iter().find(|e| e.range.contains(&offset)).and_then(|e| e.name.clone());
        from_entity.or_else(|| index.symbol_at(offset).map(|(range, _)| self.doc.slice(range).to_owned()))
    }

    // ----- saving -----

    /// Write the buffer to disk atomically (temporary file, then rename).
    pub fn save(&mut self) -> Result<(), String> {
        let temporary = self.path.with_extension("lp_diff.tmp");
        std::fs::write(&temporary, self.text()).map_err(|e| format!("cannot write {}: {e}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path).map_err(|e| format!("cannot replace {}: {e}", self.path.display()))?;
        self.dirty = false;
        self.saved = true;
        self.refresh_diagnostics();
        Ok(())
    }

    // ----- motions -----

    fn move_vertical(&mut self, delta: isize) {
        let line = self.cursor_line();
        let target = line.saturating_add_signed(delta).min(self.line_count().saturating_sub(1));
        self.cursor = self.offset_at_column(target, self.want_col);
    }

    fn word_forward(&mut self) {
        let text = self.text();
        let class = |c: char| if c.is_alphanumeric() || c == '_' { 1 } else if c.is_whitespace() { 0 } else { 2 };
        let mut offset = self.cursor;
        let start_class = text[offset..].chars().next().map_or(0, class);
        while let Some(c) = text[offset..].chars().next() {
            if class(c) != start_class || start_class == 0 {
                break;
            }
            offset += c.len_utf8();
        }
        while let Some(c) = text[offset..].chars().next() {
            if !c.is_whitespace() {
                break;
            }
            offset += c.len_utf8();
        }
        self.cursor = offset.min(text.len());
        self.clamp_normal_cursor();
    }

    fn word_backward(&mut self) {
        let text = self.text();
        let class = |c: char| if c.is_alphanumeric() || c == '_' { 1 } else if c.is_whitespace() { 0 } else { 2 };
        let mut offset = self.cursor;
        while let Some(c) = text[..offset].chars().next_back() {
            if !c.is_whitespace() {
                break;
            }
            offset -= c.len_utf8();
        }
        let target = text[..offset].chars().next_back().map_or(0, class);
        while let Some(c) = text[..offset].chars().next_back() {
            if class(c) != target {
                break;
            }
            offset -= c.len_utf8();
        }
        self.cursor = offset;
        self.want_col = self.cursor_column();
    }

    fn first_non_blank(&self, line: usize) -> usize {
        let range = self.line_range(line);
        let text = &self.text()[range.clone()];
        range.start + (text.len() - text.trim_start().len())
    }

    fn find_next(&mut self, forward: bool, height: usize) {
        let Some(needle) = self.search.clone() else {
            self.message = Some("No previous search".to_owned());
            return;
        };
        let text = self.text();
        let found = if forward {
            let from = self.next_char(self.cursor);
            text[from..].find(&needle).map(|i| from + i).or_else(|| text.find(&needle))
        } else {
            text[..self.cursor].rfind(&needle).or_else(|| text.rfind(&needle))
        };
        match found {
            Some(offset) => self.goto_offset(offset, height),
            None => self.message = Some(format!("Pattern not found: {needle}")),
        }
    }

    // ----- key handling -----

    /// Handle one key. `height` is the number of visible text lines.
    pub fn handle_key(&mut self, key: KeyEvent, height: usize) -> Outcome {
        self.message = None;
        let outcome = if self.popup.is_some() {
            self.popup_key(key, height)
        } else {
            match self.mode {
                Mode::Normal => self.normal_key(key, height),
                Mode::Insert => {
                    self.insert_key(key);
                    Outcome::Stay
                }
                Mode::Command | Mode::Search => self.prompt_key(key, height),
            }
        };
        self.scroll_to_cursor(height);
        outcome
    }

    fn popup_key(&mut self, key: KeyEvent, height: usize) -> Outcome {
        let Some(popup) = &mut self.popup else { return Outcome::Stay };
        let last = popup.lines.len().saturating_sub(1);
        let is_list = !popup.targets.is_empty() || !popup.edits.is_empty();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = None,
            KeyCode::Char('j') | KeyCode::Down if is_list => popup.selected = (popup.selected + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up if is_list => popup.selected = popup.selected.saturating_sub(1),
            KeyCode::Char('n') if is_list && key.modifiers.contains(KeyModifiers::CONTROL) => popup.selected = (popup.selected + 1).min(last),
            KeyCode::Char('p') if is_list && key.modifiers.contains(KeyModifiers::CONTROL) => popup.selected = popup.selected.saturating_sub(1),
            KeyCode::Enter if is_list => {
                let selected = popup.selected;
                let target = popup.targets.get(selected).copied();
                let edits = popup.edits.get(selected).cloned();
                self.popup = None;
                if let Some(offset) = target {
                    self.goto_offset(offset, height);
                }
                if let Some(edits) = edits {
                    // Completion keeps editing in Insert mode; actions return to Normal.
                    self.apply_edits(&edits);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn normal_key(&mut self, key: KeyEvent, height: usize) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let KeyCode::Char(c) = key.code else {
            return self.normal_special(key.code, height);
        };
        if let Some(first) = self.pending.take() {
            return self.chord(first, c, height);
        }
        let page = height.max(2);
        if ctrl {
            match c {
                'd' => self.scroll_lines(true, page / 2, height),
                'u' => self.scroll_lines(false, page / 2, height),
                'f' => self.scroll_lines(true, page, height),
                'b' => self.scroll_lines(false, page, height),
                'r' => self.redo(),
                _ => {}
            }
            return Outcome::Stay;
        }
        match c {
            'h' => {
                let start = self.line_range(self.cursor_line()).start;
                if self.cursor > start {
                    self.cursor = self.prev_char(self.cursor);
                }
                self.want_col = self.cursor_column();
            }
            'l' => {
                let range = self.line_range(self.cursor_line());
                let next = self.next_char(self.cursor);
                if next < range.end {
                    self.cursor = next;
                }
                self.want_col = self.cursor_column();
            }
            'j' => self.move_vertical(1),
            'k' => self.move_vertical(-1),
            'w' => self.word_forward(),
            'b' => self.word_backward(),
            '0' => self.set_cursor(self.line_range(self.cursor_line()).start),
            '^' => self.set_cursor(self.first_non_blank(self.cursor_line())),
            '$' => {
                let range = self.line_range(self.cursor_line());
                self.cursor = if range.end > range.start { self.prev_char(range.end) } else { range.start };
                self.want_col = usize::MAX;
            }
            'G' => {
                let last = self.line_count().saturating_sub(1);
                self.set_cursor(self.first_non_blank(last));
            }
            'g' | 'd' | 'y' | 'z' | '[' | ']' => self.pending = Some(c),
            'x' => self.delete_char(),
            'p' => self.paste(true),
            'P' => self.paste(false),
            'u' => self.undo(),
            'i' => self.enter_insert(),
            'a' => {
                let range = self.line_range(self.cursor_line());
                if self.cursor < range.end {
                    self.cursor = self.next_char(self.cursor);
                }
                self.enter_insert();
            }
            'I' => {
                self.cursor = self.first_non_blank(self.cursor_line());
                self.enter_insert();
            }
            'A' => {
                self.cursor = self.line_range(self.cursor_line()).end;
                self.enter_insert();
            }
            'o' | 'O' => self.open_line(c == 'o'),
            '/' | ':' => {
                self.mode = if c == '/' { Mode::Search } else { Mode::Command };
                self.prompt.clear();
            }
            'n' => self.find_next(true, height),
            'N' => self.find_next(false, height),
            'K' => self.hover(),
            _ => {}
        }
        Outcome::Stay
    }

    fn normal_special(&mut self, code: KeyCode, height: usize) -> Outcome {
        match code {
            KeyCode::Esc => {
                if self.pending.take().is_none() && !self.dirty {
                    return Outcome::Close;
                }
            }
            KeyCode::Enter => {
                if let Some(name) = self.entry_under_cursor() {
                    return Outcome::SelectEntry(name);
                }
            }
            KeyCode::Left => self.cursor = self.prev_char(self.cursor).max(self.line_range(self.cursor_line()).start),
            KeyCode::Right => {
                let end = self.line_range(self.cursor_line()).end;
                let next = self.next_char(self.cursor);
                if next < end {
                    self.cursor = next;
                }
            }
            KeyCode::Down => self.move_vertical(1),
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::PageDown => self.scroll_lines(true, height, height),
            KeyCode::PageUp => self.scroll_lines(false, height, height),
            KeyCode::Home => self.set_cursor(self.line_range(self.cursor_line()).start),
            KeyCode::End => {
                let range = self.line_range(self.cursor_line());
                self.cursor = if range.end > range.start { self.prev_char(range.end) } else { range.start };
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn scroll_lines(&mut self, down: bool, amount: usize, height: usize) {
        let last = self.line_count().saturating_sub(1);
        let line = self.cursor_line();
        let target = if down { (line + amount).min(last) } else { line.saturating_sub(amount) };
        self.cursor = self.offset_at_column(target, self.want_col);
        self.top = if down { (self.top + amount).min(last) } else { self.top.saturating_sub(amount) };
        self.scroll_to_cursor(height);
    }

    /// Second key of a two-key command.
    fn chord(&mut self, first: char, second: char, height: usize) -> Outcome {
        match (first, second) {
            ('g', 'g') => self.set_cursor(self.first_non_blank(0)),
            ('g', 'd') => self.go_definition(height),
            ('g', 'r') => self.list_references(),
            ('d', 'd') => self.delete_line(),
            ('y', 'y') => self.yank_line(),
            (']', 'd') => self.step_diagnostic(true, height),
            ('[', 'd') => self.step_diagnostic(false, height),
            _ => {}
        }
        Outcome::Stay
    }

    fn enter_insert(&mut self) {
        self.commit_group();
        self.mode = Mode::Insert;
    }

    fn leave_insert(&mut self) {
        self.mode = Mode::Normal;
        self.commit_group();
        let range = self.line_range(self.cursor_line());
        if self.cursor > range.start && self.cursor >= range.end {
            self.cursor = self.prev_char(self.cursor);
        }
        self.want_col = self.cursor_column();
    }

    fn open_line(&mut self, below: bool) {
        let line = self.cursor_line();
        let range = self.line_range(line);
        let indent: String = self.text()[range.clone()].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        self.enter_insert();
        if below {
            self.apply(range.end..range.end, &format!("\n{indent}"));
            self.cursor = range.end + 1 + indent.len();
        } else {
            self.apply(range.start..range.start, &format!("{indent}\n"));
            self.cursor = range.start + indent.len();
        }
        self.want_col = self.cursor_column();
    }

    fn delete_char(&mut self) {
        let range = self.line_range(self.cursor_line());
        if self.cursor >= range.end {
            return;
        }
        let end = self.next_char(self.cursor);
        self.register = self.text()[self.cursor..end].to_owned();
        self.register_linewise = false;
        self.commit_group();
        self.apply(self.cursor..end, "");
        self.commit_group();
        self.clamp_normal_cursor();
    }

    /// The line `line` with its newline, for linewise register operations.
    fn whole_line(&self, line: usize) -> Range<usize> {
        let start = self.doc.lines.line_start(line);
        let end = if line + 1 < self.line_count() { self.doc.lines.line_start(line + 1) } else { self.text().len() };
        start..end
    }

    fn yank_line(&mut self) {
        let range = self.whole_line(self.cursor_line());
        let mut text = self.text()[range].to_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = text;
        self.register_linewise = true;
        self.message = Some("1 line yanked".to_owned());
    }

    fn delete_line(&mut self) {
        self.yank_line();
        let line = self.cursor_line();
        let mut range = self.whole_line(line);
        // The last line has no newline of its own: take the one before it instead.
        if range.end == self.text().len() && range.start > 0 && !self.text()[range.clone()].ends_with('\n') {
            range.start -= 1;
        }
        self.commit_group();
        self.apply(range.clone(), "");
        self.commit_group();
        let line = line.min(self.line_count().saturating_sub(1));
        self.set_cursor(self.first_non_blank(line));
        self.clamp_normal_cursor();
    }

    fn paste(&mut self, after: bool) {
        if self.register.is_empty() {
            return;
        }
        let text = self.register.clone();
        self.commit_group();
        if self.register_linewise {
            let line = self.cursor_line();
            let at = if after { self.whole_line(line).end } else { self.doc.lines.line_start(line) };
            // Pasting after the last line, which may lack a newline.
            let prefix = if after && at == self.text().len() && !self.text().ends_with('\n') && !self.text().is_empty() { "\n" } else { "" };
            let body = if prefix.is_empty() { text } else { text.trim_end_matches('\n').to_owned() };
            self.apply(at..at, &format!("{prefix}{body}"));
            self.set_cursor(self.first_non_blank(self.doc.lines.line_of(at + prefix.len())));
        } else {
            let at = if after && self.cursor < self.line_range(self.cursor_line()).end { self.next_char(self.cursor) } else { self.cursor };
            self.apply(at..at, &text);
            self.cursor = at + text.len();
            self.cursor = self.prev_char(self.cursor).max(at);
        }
        self.commit_group();
        self.clamp_normal_cursor();
    }

    fn insert_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.leave_insert(),
            KeyCode::Char('n') if ctrl => self.open_completion(),
            KeyCode::Char(c) if !ctrl => self.insert_text(&c.to_string()),
            KeyCode::Tab => self.insert_text("  "),
            KeyCode::Enter => {
                let range = self.line_range(self.cursor_line());
                let indent: String = self.text()[range.start..self.cursor].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                self.insert_text(&format!("\n{indent}"));
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let start = self.prev_char(self.cursor);
                    self.apply(start..self.cursor, "");
                    self.set_cursor(start);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.text().len() {
                    let end = self.next_char(self.cursor);
                    self.apply(self.cursor..end, "");
                }
            }
            KeyCode::Left => self.set_cursor(self.prev_char(self.cursor)),
            KeyCode::Right => self.set_cursor(self.next_char(self.cursor)),
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::Down => self.move_vertical(1),
            _ => {}
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.apply(self.cursor..self.cursor, text);
        self.set_cursor(self.cursor + text.len());
    }

    /// Paste text arriving as one event goes straight into the buffer in Insert mode.
    pub fn paste_text(&mut self, text: &str) {
        match self.mode {
            Mode::Insert => self.insert_text(text),
            Mode::Command | Mode::Search => self.prompt.push_str(text.lines().next().unwrap_or_default()),
            Mode::Normal => {}
        }
    }

    fn prompt_key(&mut self, key: KeyEvent, height: usize) -> Outcome {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.prompt.clear();
            }
            KeyCode::Backspace => {
                if self.prompt.pop().is_none() {
                    self.mode = Mode::Normal;
                }
            }
            KeyCode::Char(c) => self.prompt.push(c),
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.prompt);
                let mode = std::mem::replace(&mut self.mode, Mode::Normal);
                if mode == Mode::Search {
                    if !line.is_empty() {
                        self.search = Some(line);
                    }
                    self.find_next(true, height);
                } else {
                    return self.run_command(line.trim());
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn run_command(&mut self, line: &str) -> Outcome {
        let (name, argument) = line.split_once(' ').map_or((line, ""), |(name, rest)| (name, rest.trim()));
        match name {
            "w" => drop(self.write_message()),
            "q" if self.dirty => self.message = Some("Unsaved changes (:w to save, :q! to discard)".to_owned()),
            "q" | "q!" => return Outcome::Close,
            "wq" | "x" => {
                if self.write_message() {
                    return Outcome::Close;
                }
            }
            "format" | "fmt" => self.format(),
            "actions" => self.open_actions(),
            "rename" if argument.is_empty() => self.message = Some("Usage: :rename NEW_NAME".to_owned()),
            "rename" => self.rename(argument),
            "" => {}
            other => self.message = Some(format!("Not an editor command: {other}")),
        }
        Outcome::Stay
    }

    /// Save, reporting the outcome in the message line. `true` on success.
    fn write_message(&mut self) -> bool {
        match self.save() {
            Ok(()) => {
                self.message = Some(format!("Wrote {}", self.path.display()));
                true
            }
            Err(error) => {
                self.message = Some(error);
                false
            }
        }
    }

    /// Diagnostic severity per line, for the gutter. Errors outrank warnings.
    pub fn line_severity(&self, line: usize) -> Option<DiagnosticSeverity> {
        self.diagnostics
            .iter()
            .filter(|d| d.range.start.line as usize <= line && line <= d.range.end.line as usize)
            .filter_map(|d| d.severity)
            .min_by_key(|severity| match *severity {
                DiagnosticSeverity::ERROR => 0,
                DiagnosticSeverity::WARNING => 1,
                _ => 2,
            })
    }

    /// The text of line `line` (without its terminator).
    pub fn line_text(&self, line: usize) -> &str {
        &self.text()[self.line_range(line)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LP: &str = "Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\nEnd\n";

    fn editor(text: &str) -> Editor {
        Editor::open(Path::new("/nonexistent/test.lp"), text, true)
    }

    fn keys(editor: &mut Editor, input: &str) -> Outcome {
        let mut last = Outcome::Stay;
        for c in input.chars() {
            let event = match c {
                '\n' => KeyEvent::from(KeyCode::Enter),
                '\u{1b}' => KeyEvent::from(KeyCode::Esc),
                c => KeyEvent::from(KeyCode::Char(c)),
            };
            last = editor.handle_key(event, 20);
        }
        last
    }

    #[test]
    fn motions_land_on_characters() {
        let mut e = editor(LP);
        keys(&mut e, "jl");
        assert_eq!((e.cursor_line(), e.cursor_column()), (1, 1));
        keys(&mut e, "$");
        assert_eq!(e.cursor, e.line_range(1).end - 1, "$ rests on the last character");
        keys(&mut e, "Ggg");
        assert_eq!(e.cursor, 0);
        keys(&mut e, "w");
        assert_eq!(e.cursor_line(), 1, "w moves past the word and its blanks");
    }

    #[test]
    fn insert_edits_are_one_undo_step() {
        let mut e = editor(LP);
        keys(&mut e, "jA z\u{1b}");
        assert!(e.text().contains("2 y z\n"), "{}", e.text());
        assert!(e.dirty);
        keys(&mut e, "u");
        assert_eq!(e.text(), LP, "the whole insert session is undone together");
        e.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL), 20);
        assert!(e.text().contains("2 y z\n"), "redo reapplies it");
    }

    #[test]
    fn line_operations_round_trip_through_the_register() {
        let mut e = editor(LP);
        keys(&mut e, "jddp");
        let lines: Vec<&str> = e.text().lines().collect();
        assert_eq!(lines[0], "Minimize");
        assert_eq!(lines[1], "Subject To", "dd removed the objective line");
        assert_eq!(lines[2], " obj: 3 x + 2 y", "p put it back below");
        keys(&mut e, "Gyyp");
        assert_eq!(e.text().matches("End").count(), 2, "yy then p duplicates the line");
    }

    #[test]
    fn open_line_keeps_indentation_and_x_deletes() {
        let mut e = editor(LP);
        keys(&mut e, "jo c3: x >= 0\u{1b}");
        assert!(e.text().contains(" obj: 3 x + 2 y\n  c3: x >= 0\n"), "{}", e.text());
        keys(&mut e, "0x");
        assert!(e.text().contains("\n c3: x >= 0\n"), "x removed one blank: {}", e.text());
    }

    #[test]
    fn search_wraps_and_repeats() {
        let mut e = editor(LP);
        keys(&mut e, "/y\n");
        assert_eq!(e.text()[e.cursor..].chars().next(), Some('y'));
        let first = e.cursor;
        keys(&mut e, "n");
        assert_ne!(e.cursor, first, "n finds the next match");
        keys(&mut e, "n");
        assert_eq!(e.cursor, first, "and wraps");
    }

    #[test]
    fn go_to_definition_and_references_use_the_language_features() {
        let mut e = editor("Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\nBounds\n x <= 10\nEnd\n");
        let at = e.text().find("c1: x + y").unwrap() + 4;
        e.set_cursor(at);
        keys(&mut e, "gd");
        assert_ne!(e.cursor, at, "gd on a variable use jumps to its first occurrence");
        keys(&mut e, "gr");
        let popup = e.popup.as_ref().expect("gr opens the references list");
        assert!(popup.lines.len() >= 3, "x appears in the objective, a constraint and a bound: {:?}", popup.lines);
        keys(&mut e, "j\n");
        assert!(e.popup.is_none(), "Enter jumps and closes the list");
    }

    #[test]
    fn hover_and_diagnostics_come_from_the_server_features() {
        let mut e = editor("Minimize\n obj: 3 x\nSubject To\n c1: x >= 1\nEnd\n");
        e.set_cursor(e.text().find("x >= 1").unwrap());
        keys(&mut e, "K");
        assert!(e.popup.as_ref().is_some_and(|popup| !popup.lines.is_empty()), "K shows a hover");
        keys(&mut e, "\u{1b}");
        let mut broken = editor("Minimize\n obj: 3 x +\nSubject To\n c1: x >= \nEnd\n");
        assert!(!broken.diagnostics.is_empty(), "syntax errors are reported");
        keys(&mut broken, "]d");
        assert!(broken.message.is_some(), "]d reports the diagnostic");
    }

    #[test]
    fn rename_and_format_commands_edit_the_buffer() {
        let mut e = editor("Minimize\n obj: 3 x + 2 y\nSubject To\n c1: x + y >= 1\nEnd\n");
        e.set_cursor(e.text().find("x + 2 y").unwrap());
        keys(&mut e, ":rename alpha\n");
        assert!(!e.text().contains(" x "), "{}", e.text());
        assert_eq!(e.text().matches("alpha").count(), 2, "every occurrence is renamed");
        keys(&mut e, "u");
        assert!(e.text().contains("3 x + 2 y"), "a rename is one undo step");
        keys(&mut e, ":format\n");
        assert!(e.message.is_some());
    }

    #[test]
    fn quit_refuses_with_unsaved_changes_and_save_is_atomic() {
        let dir = std::env::temp_dir().join(format!("lp_diff_editor_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("model.lp");
        std::fs::write(&path, LP).expect("fixture");
        let mut e = Editor::open(&path, LP, true);
        keys(&mut e, "ix\u{1b}:q\n");
        assert_eq!(keys(&mut e, ""), Outcome::Stay);
        assert!(e.message.is_none() || e.dirty, "still open and dirty");
        assert_eq!(keys(&mut e, ":wq\n"), Outcome::Close, "wq saves then closes");
        assert!(std::fs::read_to_string(&path).expect("saved").starts_with('x'));
        assert!(!path.with_extension("lp_diff.tmp").exists(), "the temporary file is renamed away");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn plain_files_have_no_language_features() {
        let mut e = Editor::open(Path::new("notes.txt"), "NAME x\nENDATA\n", false);
        keys(&mut e, "K");
        assert!(e.popup.is_none());
        assert!(e.message.as_deref().is_some_and(|m| m.contains("LP files")));
    }

    #[test]
    fn enter_asks_to_select_the_entry_under_the_cursor() {
        let mut e = editor(LP);
        e.set_cursor(e.text().find("c1").unwrap());
        assert_eq!(keys(&mut e, "\n"), Outcome::SelectEntry("c1".to_owned()));
    }

    const MPS: &str = "NAME          DEMO\nROWS\n N  COST\n L  LIM1\nCOLUMNS\n    X         COST         1.0   LIM1         1.0\nRHS\n    RHS       LIM1         4.0\nBOUNDS\n UP BND       X            4.0\nENDATA\n";

    #[test]
    fn mps_files_get_name_navigation_hover_and_rename() {
        let mut e = Editor::open(Path::new("demo.mps"), MPS, false);
        assert!(e.mps && !e.lsp);
        e.set_cursor(e.text().find("LIM1         4.0").unwrap());
        keys(&mut e, "gd");
        assert!(e.text()[..e.cursor].ends_with(" L  "), "gd on an RHS use lands on the ROWS definition");
        keys(&mut e, "gr");
        assert_eq!(e.popup.as_ref().map(|p| p.lines.len()), Some(3), "ROWS, COLUMNS and RHS");
        keys(&mut e, "\u{1b}K");
        assert!(e.popup.as_ref().is_some_and(|p| p.lines[0].starts_with("row `LIM1`")));
        keys(&mut e, "\u{1b}:rename CAP\n");
        assert_eq!(e.text().matches("CAP").count(), 3);
        assert!(!e.text().contains("LIM1"));
        keys(&mut e, "u");
        assert_eq!(e.text(), MPS, "a rename is one undo step");
        keys(&mut e, ":rename COST\n");
        assert!(e.message.as_deref().is_some_and(|m| m.contains("already exists")), "{:?}", e.message);
    }

    #[test]
    fn mps_parse_errors_become_a_diagnostic() {
        let broken = MPS.replace("4.0\nENDATA", "4.x\nENDATA");
        let mut e = Editor::open(Path::new("demo.mps"), &broken, false);
        assert_eq!(e.diagnostics.len(), 1, "{:?}", e.diagnostics);
        keys(&mut e, "]d");
        assert!(e.message.is_some());
        let clean = Editor::open(Path::new("demo.mps"), MPS, false);
        assert_eq!(clean.diagnostics, []);
    }
}
