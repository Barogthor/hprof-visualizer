//! Stack frame panel: frame list with inline local variable tree.
//!
//! [`StackState`] manages frame selection and expand/collapse of local vars.
//! [`StackView`] is a [`StatefulWidget`] rendering the current state.

use std::collections::{HashMap, HashSet};

use hprof_engine::{FieldInfo, FieldValue, FrameInfo, LineNumber, VariableInfo, VariableValue};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::theme;

/// Phase of an object expansion driven by `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionPhase {
    Collapsed,
    Loading,
    Expanded,
    Failed,
}

/// Phase of a lazy string value load driven by `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringPhase {
    Unloaded,
    Loading,
    Loaded,
    Failed,
}

/// Cursor position within the frame+var tree.
///
/// `field_path` encodes depth from the root `ObjectRef` var:
/// - `[]` (empty) — loading/error node for the root var
/// - `[2]` — field index 2 of the root object (depth 1)
/// - `[2, 1]` — field index 1 within field 2's expanded object (depth 2)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackCursor {
    NoFrames,
    OnFrame(usize),
    OnVar {
        frame_idx: usize,
        var_idx: usize,
    },
    /// Cursor on a specific field within an expanded object at any depth.
    OnObjectField {
        frame_idx: usize,
        var_idx: usize,
        /// Path of field indices from the root object to the current field.
        field_path: Vec<usize>,
    },
    /// Cursor on the loading/error pseudo-node for an expanding object.
    OnObjectLoadingNode {
        frame_idx: usize,
        var_idx: usize,
        /// Empty = root var's loading node. Non-empty = nested object's node.
        field_path: Vec<usize>,
    },
    /// Cursor on a cyclic reference marker (non-expandable leaf).
    OnCyclicNode {
        frame_idx: usize,
        var_idx: usize,
        field_path: Vec<usize>,
    },
}

/// State for the stack frame panel.
pub struct StackState {
    frames: Vec<FrameInfo>,
    /// Vars per frame_id — populated on demand by `App` calling the engine.
    vars: HashMap<u64, Vec<VariableInfo>>,
    expanded: HashSet<u64>,
    cursor: StackCursor,
    list_state: ListState,
    /// Per-object expansion phases (keyed by object_id).
    object_phases: HashMap<u64, ExpansionPhase>,
    /// Decoded fields for expanded objects.
    pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
    /// Error messages for failed expansions.
    object_errors: HashMap<u64, String>,
    /// Per-StringRef load phases (keyed by string object_id).
    string_phases: HashMap<u64, StringPhase>,
    /// Resolved string values (keyed by string object_id).
    string_values: HashMap<u64, String>,
    /// Error messages for failed string loads (keyed by string object_id).
    string_errors: HashMap<u64, String>,
}

/// Collects all descendant object IDs reachable from `root_id` in depth-first
/// post-order. Cycles are broken via `visited`.
fn collect_descendants(
    root_id: u64,
    fields: &HashMap<u64, Vec<FieldInfo>>,
    visited: &mut HashSet<u64>,
    out: &mut Vec<u64>,
) {
    if !visited.insert(root_id) {
        return;
    }
    if let Some(field_list) = fields.get(&root_id) {
        for f in field_list {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                collect_descendants(id, fields, visited, out);
            }
        }
    }
    out.push(root_id);
}

fn format_frame_label(frame: &FrameInfo) -> String {
    let line_label = match &frame.line {
        LineNumber::Line(n) => format!(":{}", n),
        LineNumber::NoInfo => String::new(),
        LineNumber::Unknown => " (?)".to_string(),
        LineNumber::Compiled => " (compiled)".to_string(),
        LineNumber::Native => " (native)".to_string(),
    };
    let location = if frame.source_file.is_empty() {
        line_label
    } else {
        format!(" [{}{}]", frame.source_file, line_label)
    };
    format!("{}.{}(){}", frame.class_name, frame.method_name, location)
}

impl StackState {
    /// Creates a new state for the given frames. Selects first frame.
    pub fn new(frames: Vec<FrameInfo>) -> Self {
        let cursor = if frames.is_empty() {
            StackCursor::NoFrames
        } else {
            StackCursor::OnFrame(0)
        };
        let mut list_state = ListState::default();
        if !frames.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            frames,
            vars: HashMap::new(),
            expanded: HashSet::new(),
            cursor,
            list_state,
            object_phases: HashMap::new(),
            object_fields: HashMap::new(),
            object_errors: HashMap::new(),
            string_phases: HashMap::new(),
            string_values: HashMap::new(),
            string_errors: HashMap::new(),
        }
    }

    /// Returns the frame_id currently selected, if any.
    pub fn selected_frame_id(&self) -> Option<u64> {
        match &self.cursor {
            StackCursor::NoFrames => None,
            StackCursor::OnFrame(fi) => self.frames.get(*fi).map(|f| f.frame_id),
            StackCursor::OnVar { frame_idx, .. }
            | StackCursor::OnObjectField { frame_idx, .. }
            | StackCursor::OnObjectLoadingNode { frame_idx, .. }
            | StackCursor::OnCyclicNode { frame_idx, .. } => {
                self.frames.get(*frame_idx).map(|f| f.frame_id)
            }
        }
    }

    /// Returns the current cursor.
    pub fn cursor(&self) -> &StackCursor {
        &self.cursor
    }

    /// Returns the object_id if the cursor is on an `ObjectRef` var.
    pub fn selected_object_id(&self) -> Option<u64> {
        if let StackCursor::OnVar { frame_idx, var_idx } = self.cursor {
            let frame = self.frames.get(frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(var_idx)?;
            if let VariableValue::ObjectRef { id, .. } = var.value {
                return Some(id);
            }
        }
        None
    }

    /// Returns the object_id if the cursor is on a loading/failed/empty pseudo-node.
    ///
    /// For root-level loading nodes (`field_path` empty) returns the root var's
    /// `ObjectRef` id. For nested loading nodes returns the nested object's id.
    pub fn selected_loading_object_id(&self) -> Option<u64> {
        if let StackCursor::OnObjectLoadingNode {
            frame_idx,
            var_idx,
            field_path,
        } = &self.cursor
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                return Some(self.resolve_object_at_path(root_id, field_path));
            }
        }
        None
    }

    /// Walks `field_path` from `root_id` and returns the object_id that owns
    /// the field at the last path element. An empty path returns `root_id`.
    fn resolve_object_at_path(&self, root_id: u64, field_path: &[usize]) -> u64 {
        let mut current = root_id;
        for &step in field_path {
            if let Some(fields) = self.object_fields.get(&current)
                && let Some(field) = fields.get(step)
                && let FieldValue::ObjectRef { id, .. } = field.value
            {
                current = id;
            } else {
                break;
            }
        }
        current
    }

    /// Returns the `ObjectRef` id of the field under the cursor, if the cursor
    /// is `OnObjectField` and that field holds a `FieldValue::ObjectRef`. Used
    /// by `App` to start or stop nested expansion; the caller is responsible
    /// for checking the expansion phase.
    pub fn selected_field_ref_id(&self) -> Option<u64> {
        if let StackCursor::OnObjectField {
            frame_idx,
            var_idx,
            field_path,
        } = &self.cursor
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                // Walk to parent object.
                let parent_path = &field_path[..field_path.len().saturating_sub(1)];
                let parent_id = self.resolve_object_at_path(root_id, parent_path);
                let field_idx = *field_path.last()?;
                let fields = self.object_fields.get(&parent_id)?;
                let field = fields.get(field_idx)?;
                if let FieldValue::ObjectRef { id, .. } = field.value {
                    return Some(id);
                }
            }
        }
        None
    }

    /// Returns the expansion phase for `object_id` (defaults to `Collapsed`).
    pub fn expansion_state(&self, object_id: u64) -> ExpansionPhase {
        self.object_phases
            .get(&object_id)
            .cloned()
            .unwrap_or(ExpansionPhase::Collapsed)
    }

    /// Marks an object as loading (called by App on expansion start).
    pub fn set_expansion_loading(&mut self, object_id: u64) {
        self.object_phases
            .insert(object_id, ExpansionPhase::Loading);
    }

    /// Marks an object expansion as complete with decoded fields.
    pub fn set_expansion_done(&mut self, object_id: u64, fields: Vec<FieldInfo>) {
        self.object_fields.insert(object_id, fields);
        self.object_phases
            .insert(object_id, ExpansionPhase::Expanded);
    }

    /// Marks an object expansion as failed with an error message.
    pub fn set_expansion_failed(&mut self, object_id: u64, error: String) {
        self.object_errors.insert(object_id, error);
        self.object_phases.insert(object_id, ExpansionPhase::Failed);
    }

    /// Cancels a loading expansion — reverts to `Collapsed`.
    pub fn cancel_expansion(&mut self, object_id: u64) {
        self.object_phases.remove(&object_id);
        self.object_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
    }

    /// Returns the current [`StringPhase`] for `id` (defaults to `Unloaded`).
    pub fn string_phase(&self, id: u64) -> StringPhase {
        self.string_phases
            .get(&id)
            .cloned()
            .unwrap_or(StringPhase::Unloaded)
    }

    /// Marks a string as loading.
    pub fn start_string_loading(&mut self, id: u64) {
        self.string_phases.insert(id, StringPhase::Loading);
    }

    /// Marks a string as loaded with its resolved value.
    pub fn set_string_loaded(&mut self, id: u64, value: String) {
        self.string_errors.remove(&id);
        self.string_phases.insert(id, StringPhase::Loaded);
        self.string_values.insert(id, value);
    }

    /// Marks a string as failed with an error message.
    pub fn set_string_failed(&mut self, id: u64, err: String) {
        self.string_phases.insert(id, StringPhase::Failed);
        self.string_errors.insert(id, err);
    }

    /// Returns the `StringRef` id if the cursor is on an `OnObjectField` with
    /// a `StringRef` field whose phase is `Unloaded` or `Failed`.
    pub fn selected_field_string_id(&self) -> Option<u64> {
        if let StackCursor::OnObjectField {
            frame_idx,
            var_idx,
            field_path,
        } = &self.cursor
        {
            let frame = self.frames.get(*frame_idx)?;
            let vars = self.vars.get(&frame.frame_id)?;
            let var = vars.get(*var_idx)?;
            if let VariableValue::ObjectRef { id: root_id, .. } = var.value {
                let parent_path = &field_path[..field_path.len().saturating_sub(1)];
                let parent_id = self.resolve_object_at_path(root_id, parent_path);
                let field_idx = *field_path.last()?;
                let fields = self.object_fields.get(&parent_id)?;
                let field = fields.get(field_idx)?;
                if let FieldValue::StringRef { id } = field.value {
                    let phase = self.string_phase(id);
                    if phase == StringPhase::Unloaded || phase == StringPhase::Failed {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    /// Collapses an expanded object.
    pub fn collapse_object(&mut self, object_id: u64) {
        self.object_phases.remove(&object_id);
        self.object_fields.remove(&object_id);
        self.object_errors.remove(&object_id);
    }

    /// Recursively collapses `object_id` and all nested expanded descendants.
    ///
    /// Also clears string state for any `StringRef` fields in collapsed objects.
    /// Uses a visited set to guard against cycles in corrupted heap metadata.
    /// After collapse, resyncs the cursor if it became orphaned.
    pub fn collapse_object_recursive(&mut self, object_id: u64) {
        let mut to_remove: Vec<u64> = Vec::new();
        let mut visited: HashSet<u64> = HashSet::new();
        collect_descendants(
            object_id,
            &self.object_fields,
            &mut visited,
            &mut to_remove,
        );
        // Clear string state for any StringRef fields in descendants.
        for &desc_id in &to_remove {
            if let Some(fields) = self.object_fields.get(&desc_id) {
                let string_ids: Vec<u64> = fields
                    .iter()
                    .filter_map(|f| {
                        if let FieldValue::StringRef { id } = f.value {
                            Some(id)
                        } else {
                            None
                        }
                    })
                    .collect();
                for sid in string_ids {
                    self.string_phases.remove(&sid);
                    self.string_values.remove(&sid);
                    self.string_errors.remove(&sid);
                }
            }
        }
        for id in to_remove {
            self.collapse_object(id);
        }
        self.resync_cursor_after_collapse();
    }

    /// If the current cursor is no longer in the flat list (orphaned
    /// after a collapse that propagated through a cyclic back-ref),
    /// fall back to the parent `OnVar` or `OnFrame`.
    fn resync_cursor_after_collapse(&mut self) {
        let flat = self.flat_items();
        if flat.contains(&self.cursor) {
            return;
        }
        // Try falling back to OnVar
        match &self.cursor {
            StackCursor::OnObjectField {
                frame_idx,
                var_idx,
                ..
            }
            | StackCursor::OnCyclicNode {
                frame_idx,
                var_idx,
                ..
            }
            | StackCursor::OnObjectLoadingNode {
                frame_idx,
                var_idx,
                ..
            } => {
                let fallback = StackCursor::OnVar {
                    frame_idx: *frame_idx,
                    var_idx: *var_idx,
                };
                if flat.contains(&fallback) {
                    self.cursor = fallback;
                } else {
                    self.cursor =
                        StackCursor::OnFrame(*frame_idx);
                }
            }
            _ => {}
        }
        self.sync_list_state();
    }

    /// Returns all `StringRef` object IDs reachable from `object_id` in the
    /// current expansion tree.
    ///
    /// Used by `App` to cancel in-flight string loads before collapsing a
    /// subtree, preventing stale completions from re-populating cleared state.
    pub fn string_ids_in_subtree(&self, object_id: u64) -> Vec<u64> {
        let mut descendants = Vec::new();
        let mut visited = HashSet::new();
        collect_descendants(
            object_id,
            &self.object_fields,
            &mut visited,
            &mut descendants,
        );
        let mut string_ids = Vec::new();
        for &desc_id in &descendants {
            if let Some(fields) = self.object_fields.get(&desc_id) {
                for f in fields {
                    if let FieldValue::StringRef { id } = f.value {
                        string_ids.push(id);
                    }
                }
            }
        }
        string_ids
    }

    /// Loads vars for `frame_id` into internal cache and toggles expand/collapse.
    ///
    /// When collapsing: if cursor is on a var of this frame, it is reset to the
    /// frame row so navigation remains consistent. All expanded objects that
    /// belong to vars of this frame are recursively collapsed.
    pub fn toggle_expand(&mut self, frame_id: u64, vars: Vec<VariableInfo>) {
        if self.expanded.contains(&frame_id) {
            self.expanded.remove(&frame_id);
            // Recursively collapse any expanded objects in this frame's vars.
            if let Some(cached_vars) = self.vars.get(&frame_id) {
                let object_ids: Vec<u64> = cached_vars
                    .iter()
                    .filter_map(|v| {
                        if let VariableValue::ObjectRef { id, .. } = v.value {
                            Some(id)
                        } else {
                            None
                        }
                    })
                    .collect();
                for oid in object_ids {
                    self.collapse_object_recursive(oid);
                }
            }
            // Reset cursor to the frame row when collapsing from a var position.
            if let StackCursor::OnVar { frame_idx, .. }
            | StackCursor::OnObjectField { frame_idx, .. }
            | StackCursor::OnObjectLoadingNode { frame_idx, .. }
            | StackCursor::OnCyclicNode { frame_idx, .. } = self.cursor
            {
                self.cursor = StackCursor::OnFrame(frame_idx);
            }
        } else {
            self.vars.insert(frame_id, vars);
            self.expanded.insert(frame_id);
        }
        self.sync_list_state();
    }

    /// Returns whether `frame_id` is currently expanded.
    pub fn is_expanded(&self, frame_id: u64) -> bool {
        self.expanded.contains(&frame_id)
    }

    /// Moves the cursor one step down.
    pub fn move_down(&mut self) {
        let flat = self.flat_items();
        if let Some(current) = flat.iter().position(|c| c == &self.cursor)
            && current + 1 < flat.len()
        {
            let next = current + 1;
            self.cursor = flat[next].clone();
            self.list_state.select(Some(next));
        }
    }

    /// Moves the cursor one step up.
    pub fn move_up(&mut self) {
        let flat = self.flat_items();
        if let Some(current) = flat.iter().position(|c| c == &self.cursor)
            && let Some(prev) = current.checked_sub(1)
        {
            self.cursor = flat[prev].clone();
            self.list_state.select(Some(prev));
        }
    }

    /// Returns the flattened cursor index (position in the rendered list).
    fn flat_index(&self) -> Option<usize> {
        let flat = self.flat_items();
        flat.iter().position(|c| c == &self.cursor)
    }

    /// Flattened ordered list of cursors matching the rendered list items.
    fn flat_items(&self) -> Vec<StackCursor> {
        let mut out = Vec::new();
        for (fi, frame) in self.frames.iter().enumerate() {
            out.push(StackCursor::OnFrame(fi));
            if self.expanded.contains(&frame.frame_id) {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                if vars.is_empty() {
                    out.push(StackCursor::OnVar {
                        frame_idx: fi,
                        var_idx: 0,
                    });
                } else {
                    for (vi, var) in vars.iter().enumerate() {
                        out.push(StackCursor::OnVar {
                            frame_idx: fi,
                            var_idx: vi,
                        });
                        if let VariableValue::ObjectRef { id: object_id, .. } = var.value {
                            let mut visited = HashSet::new();
                            self.emit_object_children(
                                fi,
                                vi,
                                object_id,
                                vec![],
                                &mut visited,
                                &mut out,
                            );
                        }
                    }
                }
            }
        }
        out
    }

    /// Emits cursor nodes for the children of `object_id` at `parent_path`.
    ///
    /// Guards against runaway recursion: stops at depth 16.
    /// `visited` tracks the ancestor chain for cycle detection.
    fn emit_object_children(
        &self,
        fi: usize,
        vi: usize,
        object_id: u64,
        parent_path: Vec<usize>,
        visited: &mut HashSet<u64>,
        out: &mut Vec<StackCursor>,
    ) {
        if parent_path.len() >= 16 {
            return;
        }
        match self.expansion_state(object_id) {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                out.push(StackCursor::OnObjectLoadingNode {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: parent_path,
                });
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let fields = self.object_fields.get(&object_id);
                let field_count = fields.map(|f| f.len()).unwrap_or(0);
                if field_count == 0 {
                    out.push(StackCursor::OnObjectLoadingNode {
                        frame_idx: fi,
                        var_idx: vi,
                        field_path: parent_path.clone(),
                    });
                } else {
                    let field_list = fields.unwrap();
                    for (idx, field) in field_list.iter().enumerate() {
                        let mut path = parent_path.clone();
                        path.push(idx);
                        if let FieldValue::ObjectRef { id, .. } = field.value
                            && visited.contains(&id)
                        {
                            out.push(StackCursor::OnCyclicNode {
                                frame_idx: fi,
                                var_idx: vi,
                                field_path: path,
                            });
                            continue;
                        }
                        out.push(StackCursor::OnObjectField {
                            frame_idx: fi,
                            var_idx: vi,
                            field_path: path.clone(),
                        });
                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.emit_object_children(fi, vi, id, path, visited, out);
                        }
                    }
                }
                visited.remove(&object_id);
            }
            ExpansionPhase::Failed => {
                out.push(StackCursor::OnObjectLoadingNode {
                    frame_idx: fi,
                    var_idx: vi,
                    field_path: parent_path,
                });
            }
        }
    }

    fn sync_list_state(&mut self) {
        let idx = self.flat_index();
        self.list_state.select(idx);
    }

    /// Formats a collapsed [`FieldValue::ObjectRef`] as `ClassName [>]`
    /// or `ClassName (N entries) [>]` for collections.
    fn format_object_ref_collapsed(class_name: &str, entry_count: Option<u64>) -> String {
        let display_name = if class_name.is_empty() {
            "Object"
        } else {
            class_name
        };
        let short = display_name.rsplit('.').next().unwrap_or(display_name);
        match entry_count {
            Some(n) => format!("{short} ({n} entries)"),
            None => short.to_string(),
        }
    }

    /// Truncates a string for display: max 80 chars, appended `..` if longer.
    fn truncate_string_display(s: &str) -> String {
        const MAX_STRING_DISPLAY: usize = 80;
        if s.chars().count() <= MAX_STRING_DISPLAY {
            s.to_string()
        } else {
            let end = s
                .char_indices()
                .nth(MAX_STRING_DISPLAY)
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            format!("{}..", &s[..end])
        }
    }

    /// Formats a [`FieldValue`] for display in field rows (depth ≥ 1).
    ///
    /// For `StringRef`, the `string_phase` and optional `string_value` control
    /// the display.
    fn format_field_value(
        v: &FieldValue,
        phase: Option<&ExpansionPhase>,
        string_phase: Option<(&StringPhase, Option<&str>)>,
    ) -> String {
        match v {
            FieldValue::Null => "null".to_string(),
            FieldValue::StringRef { .. } => match string_phase {
                Some((StringPhase::Loaded, Some(val))) => {
                    format!("String = \"{}\"", Self::truncate_string_display(val))
                }
                Some((StringPhase::Failed, _)) => "String = <unresolved>".to_string(),
                Some((StringPhase::Loading, _)) => "String = \"~\"".to_string(),
                _ => "String = \"...\"".to_string(),
            },
            FieldValue::ObjectRef {
                class_name,
                entry_count,
                ..
            } => match phase {
                Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => {
                    let display_name = if class_name.is_empty() {
                        "Object"
                    } else {
                        class_name
                    };
                    let short = display_name.rsplit('.').next().unwrap_or(display_name);
                    short.to_string()
                }
                _ => Self::format_object_ref_collapsed(class_name, *entry_count),
            },
            FieldValue::Bool(b) => b.to_string(),
            FieldValue::Char(c) => format!("'{c}'"),
            FieldValue::Byte(n) => n.to_string(),
            FieldValue::Short(n) => n.to_string(),
            FieldValue::Int(n) => n.to_string(),
            FieldValue::Long(n) => n.to_string(),
            FieldValue::Float(f) => format!("{f}"),
            FieldValue::Double(d) => format!("{d}"),
        }
    }

    /// Builds the list items for rendering.
    pub fn build_items(&self) -> Vec<ListItem<'static>> {
        let mut items = Vec::new();
        for (fi, frame) in self.frames.iter().enumerate() {
            let label = format_frame_label(frame);
            let is_expanded = self.expanded.contains(&frame.frame_id);
            let toggle = if !frame.has_variables {
                "  "
            } else if is_expanded {
                "- "
            } else {
                "+ "
            };
            let text = format!("{toggle}{label}");
            let is_selected = matches!(&self.cursor, StackCursor::OnFrame(i) if *i == fi)
                || matches!(&self.cursor,
                    StackCursor::OnVar { frame_idx, .. }
                    | StackCursor::OnObjectField { frame_idx, .. }
                    | StackCursor::OnObjectLoadingNode { frame_idx, .. }
                    | StackCursor::OnCyclicNode { frame_idx, .. }
                    if *frame_idx == fi);
            let style = if is_selected {
                theme::SELECTED
            } else {
                ratatui::style::Style::default()
            };
            items.push(ListItem::new(Line::from(Span::styled(text, style))));

            if self.expanded.contains(&frame.frame_id) {
                let empty = vec![];
                let vars = self.vars.get(&frame.frame_id).unwrap_or(&empty);
                if vars.is_empty() {
                    let var_style = if matches!(&self.cursor,
                        StackCursor::OnVar { frame_idx, .. } if *frame_idx == fi)
                    {
                        theme::SELECTED
                    } else {
                        theme::SEARCH_HINT
                    };
                    items.push(ListItem::new(Line::from(Span::styled(
                        "  (no locals)",
                        var_style,
                    ))));
                } else {
                    for (vi, var) in vars.iter().enumerate() {
                        let phase = if let VariableValue::ObjectRef { id, .. } = var.value {
                            self.expansion_state(id)
                        } else {
                            ExpansionPhase::Collapsed
                        };

                        let (toggle, val_str) = match (&var.value, &phase) {
                            (VariableValue::Null, _) => ("  ", "null".to_string()),
                            (
                                VariableValue::ObjectRef { class_name, .. },
                                ExpansionPhase::Collapsed,
                            ) => ("+ ", format!("local variable: {}", class_name)),
                            (VariableValue::ObjectRef { class_name, .. }, _) => {
                                ("- ", format!("local variable: {}", class_name))
                            }
                        };
                        let var_text = format!("  {toggle}[{}] {val_str}", var.index,);
                        let var_selected = matches!(&self.cursor,
                            StackCursor::OnVar { frame_idx: ffi, var_idx: vvi }
                            if *ffi == fi && *vvi == vi);
                        let var_style = if var_selected {
                            theme::SELECTED
                        } else {
                            ratatui::style::Style::default()
                        };
                        items.push(ListItem::new(Line::from(Span::styled(var_text, var_style))));

                        if let VariableValue::ObjectRef { id: object_id, .. } = var.value {
                            let mut visited = HashSet::new();
                            self.build_object_items(
                                fi,
                                vi,
                                object_id,
                                &[],
                                &mut visited,
                                &mut items,
                            );
                        }
                    }
                }
            }
        }
        if items.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "(no frames)",
                theme::SEARCH_HINT,
            ))));
        }
        items
    }

    /// Recursively appends list items for `object_id` at `parent_path`.
    ///
    /// Indentation = `2 + 2 * (parent_path.len() + 1)` spaces for field rows.
    /// `visited` tracks the ancestor chain for cycle detection.
    fn build_object_items(
        &self,
        fi: usize,
        vi: usize,
        object_id: u64,
        parent_path: &[usize],
        visited: &mut HashSet<u64>,
        items: &mut Vec<ListItem<'static>>,
    ) {
        // Guard against runaway recursion.
        if parent_path.len() >= 16 {
            return;
        }
        // Depth: root fields at depth 1 → 4 spaces, depth 2 → 6 spaces, etc.
        let indent = " ".repeat(2 + 2 * (parent_path.len() + 1));
        let phase = self.expansion_state(object_id);
        match phase {
            ExpansionPhase::Collapsed => {}
            ExpansionPhase::Loading => {
                let cur_path: Vec<usize> = parent_path.to_vec();
                let selected = matches!(&self.cursor,
                    StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                    if *ffi == fi && *vvi == vi && *field_path == cur_path);
                let s = if selected {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}~ Loading..."),
                    s,
                ))));
            }
            ExpansionPhase::Expanded => {
                visited.insert(object_id);
                let empty: Vec<FieldInfo> = vec![];
                let field_list = self
                    .object_fields
                    .get(&object_id)
                    .map(|f| f.as_slice())
                    .unwrap_or(empty.as_slice());
                if field_list.is_empty() {
                    let cur_path: Vec<usize> = parent_path.to_vec();
                    let selected = matches!(&self.cursor,
                        StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                        if *ffi == fi && *vvi == vi && *field_path == cur_path);
                    let s = if selected {
                        theme::SELECTED
                    } else {
                        theme::SEARCH_HINT
                    };
                    items.push(ListItem::new(Line::from(Span::styled(
                        format!("{indent}(no fields)"),
                        s,
                    ))));
                } else {
                    for (fidx, field) in field_list.iter().enumerate() {
                        let mut child_path = parent_path.to_vec();
                        child_path.push(fidx);

                        // Cycle detection for ObjectRef fields
                        if let FieldValue::ObjectRef { id, class_name, .. } = &field.value
                            && visited.contains(id)
                        {
                            let label = if *id == object_id {
                                "self-ref"
                            } else {
                                "cyclic"
                            };
                            let short = class_name.rsplit('.').next().unwrap_or(class_name);
                            let marker = format!("\u{21BB} {} @ 0x{:X} [{}]", short, id, label,);
                            let text = format!("{indent}  {}: {}", field.name, marker,);
                            let sel = matches!(
                                &self.cursor,
                                StackCursor::OnCyclicNode {
                                    frame_idx: ffi,
                                    var_idx: vvi,
                                    field_path,
                                }
                                if *ffi == fi
                                    && *vvi == vi
                                    && *field_path == child_path
                            );
                            let s = if sel {
                                theme::SELECTED
                            } else {
                                theme::SEARCH_HINT
                            };
                            items.push(ListItem::new(Line::from(Span::styled(text, s))));
                            continue;
                        }

                        let selected = matches!(&self.cursor,
                            StackCursor::OnObjectField { frame_idx: ffi, var_idx: vvi, field_path }
                            if *ffi == fi && *vvi == vi && *field_path == child_path);
                        let child_phase = if let FieldValue::ObjectRef { id, .. } = field.value {
                            Some(self.expansion_state(id))
                        } else {
                            None
                        };
                        let string_phase_info = if let FieldValue::StringRef { id } = field.value {
                            let phase = self.string_phase(id);
                            let value = self.string_values.get(&id).map(|s| s.as_str());
                            Some((phase, value))
                        } else {
                            None
                        };
                        let s = if selected {
                            if matches!(string_phase_info, Some((StringPhase::Failed, _))) {
                                theme::STATUS_WARNING
                            } else {
                                theme::SELECTED
                            }
                        } else if matches!(string_phase_info, Some((StringPhase::Failed, _))) {
                            theme::STATUS_WARNING
                        } else {
                            ratatui::style::Style::default()
                        };
                        let val = Self::format_field_value(
                            &field.value,
                            child_phase.as_ref(),
                            string_phase_info.as_ref().map(|(p, v)| (p, *v)),
                        );
                        let toggle = match &child_phase {
                            Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
                            Some(ExpansionPhase::Collapsed) | Some(ExpansionPhase::Failed) => "+ ",
                            None => "  ",
                        };
                        let text = format!("{indent}{toggle}{}: {}", field.name, val,);
                        items.push(ListItem::new(Line::from(Span::styled(text, s))));

                        if let FieldValue::ObjectRef { id, .. } = field.value {
                            self.build_object_items(fi, vi, id, &child_path, visited, items);
                        }
                    }
                }
                visited.remove(&object_id);
            }
            ExpansionPhase::Failed => {
                let cur_path: Vec<usize> = parent_path.to_vec();
                let msg = self
                    .object_errors
                    .get(&object_id)
                    .cloned()
                    .unwrap_or_else(|| "Failed to resolve object".to_string());
                let selected = matches!(&self.cursor,
                    StackCursor::OnObjectLoadingNode { frame_idx: ffi, var_idx: vvi, field_path }
                    if *ffi == fi && *vvi == vi && *field_path == cur_path);
                let s = if selected {
                    theme::SELECTED
                } else {
                    theme::SEARCH_HINT
                };
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}! {msg}"),
                    s,
                ))));
            }
        }
    }
}

/// Stateful widget for the stack frame panel.
pub struct StackView {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
}

impl StatefulWidget for StackView {
    type State = StackState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let border_style = if self.focused {
            theme::BORDER_FOCUSED
        } else {
            theme::BORDER_UNFOCUSED
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title("Stack Frames  [Enter] expand  [Esc] back");
        let inner = block.inner(area);
        block.render(area, buf);

        let items = state.build_items();
        let list = List::new(items)
            .highlight_style(ratatui::style::Style::default().add_modifier(Modifier::BOLD));
        StatefulWidget::render(list, inner, buf, &mut state.list_state);
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{FrameInfo, LineNumber, VariableInfo, VariableValue};

    use super::*;

    fn make_frame(frame_id: u64) -> FrameInfo {
        FrameInfo {
            frame_id,
            method_name: format!("method{}", frame_id),
            class_name: format!("Class{}", frame_id),
            source_file: format!("Class{}.java", frame_id),
            line: LineNumber::Line(1),
            has_variables: false,
        }
    }

    fn make_var(index: usize, object_id: u64) -> VariableInfo {
        VariableInfo {
            index,
            value: if object_id == 0 {
                VariableValue::Null
            } else {
                VariableValue::ObjectRef {
                    id: object_id,
                    class_name: "Object".to_string(),
                }
            },
        }
    }

    #[test]
    fn new_with_three_frames_selects_frame_0() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let state = StackState::new(frames);
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn move_down_on_three_frames_with_no_expanded_moves_to_frame_1() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let mut state = StackState::new(frames);
        state.move_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn move_up_at_frame_0_does_nothing() {
        let frames = vec![make_frame(1), make_frame(2), make_frame(3)];
        let mut state = StackState::new(frames);
        state.move_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn toggle_expand_with_vars_then_move_down_moves_to_var_0() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 1), make_var(1, 2)];
        state.toggle_expand(10, vars);
        // cursor is still OnFrame(0), move_down should go to OnVar{frame_idx:0, var_idx:0}
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
    }

    #[test]
    fn move_down_past_last_var_of_expanded_frame_moves_to_next_frame() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 1)];
        state.toggle_expand(10, vars);
        // flat: [Frame(0), Var{0,0}, Frame(1)]
        state.move_down(); // → Var{0,0}
        state.move_down(); // → Frame(1)
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn toggle_expand_on_already_expanded_frame_collapses_it() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        state.toggle_expand(10, vec![make_var(0, 1)]);
        assert!(state.is_expanded(10));
        state.toggle_expand(10, vec![]);
        assert!(!state.is_expanded(10));
    }

    #[test]
    fn toggle_expand_collapse_from_var_cursor_resets_to_frame_and_navigation_works() {
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        state.toggle_expand(10, vec![make_var(0, 1)]);
        state.move_down(); // → OnVar{frame_idx:0, var_idx:0}
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
        // Collapse while cursor is on a var
        state.toggle_expand(10, vec![]);
        // Cursor must reset to the frame row
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
        // Navigation must work: can move to the next frame
        state.move_down();
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
        // And back
        state.move_up();
        assert_eq!(state.cursor, StackCursor::OnFrame(0));
    }

    #[test]
    fn selected_frame_id_returns_correct_frame_id() {
        let frames = vec![make_frame(42), make_frame(99)];
        let state = StackState::new(frames);
        assert_eq!(state.selected_frame_id(), Some(42));
    }

    #[test]
    fn format_frame_label_keeps_line_metadata_when_source_file_missing() {
        let frame = FrameInfo {
            frame_id: 1,
            method_name: "run".to_string(),
            class_name: "Thread".to_string(),
            source_file: String::new(),
            line: LineNumber::Native,
            has_variables: false,
        };
        assert_eq!(format_frame_label(&frame), "Thread.run() (native)");
    }

    #[test]
    fn format_frame_label_with_source_file_and_line_number() {
        let frame = FrameInfo {
            frame_id: 1,
            method_name: "run".to_string(),
            class_name: "Thread".to_string(),
            source_file: "Thread.java".to_string(),
            line: LineNumber::Line(42),
            has_variables: false,
        };
        assert_eq!(format_frame_label(&frame), "Thread.run() [Thread.java:42]");
    }

    #[test]
    fn new_with_empty_frames_returns_none_for_selected_frame_id() {
        let state = StackState::new(vec![]);
        assert_eq!(state.selected_frame_id(), None);
    }

    // --- Task 10: Object expansion phase tests ---

    fn make_var_object_ref(index: usize, object_id: u64) -> VariableInfo {
        VariableInfo {
            index,
            value: VariableValue::ObjectRef {
                id: object_id,
                class_name: "Object".to_string(),
            },
        }
    }

    #[test]
    fn set_expansion_loading_changes_phase_to_loading() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_loading(42);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Loading);
    }

    #[test]
    fn set_expansion_done_changes_phase_to_expanded() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_done(42, vec![]);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Expanded);
    }

    #[test]
    fn set_expansion_failed_changes_phase_to_failed() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_failed(42, "err".to_string());
        assert_eq!(state.expansion_state(42), ExpansionPhase::Failed);
    }

    #[test]
    fn cancel_expansion_on_loading_reverts_to_collapsed() {
        let mut state = StackState::new(vec![make_frame(1)]);
        state.set_expansion_loading(42);
        state.cancel_expansion(42);
        assert_eq!(state.expansion_state(42), ExpansionPhase::Collapsed);
    }

    #[test]
    fn flat_items_loading_object_includes_loading_node() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_loading(99);
        let flat = state.flat_items();
        assert!(flat.contains(&StackCursor::OnObjectLoadingNode {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![],
        }));
    }

    #[test]
    fn flat_items_expanded_with_two_fields_includes_two_object_field_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "a".to_string(),
                value: FieldValue::Int(1),
            },
            FieldInfo {
                name: "b".to_string(),
                value: FieldValue::Int(2),
            },
        ];
        state.set_expansion_done(99, fields);
        let flat = state.flat_items();
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![1],
        }));
    }

    #[test]
    fn move_down_from_on_var_expanded_moves_to_first_object_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(99, fields);
        // cursor is OnFrame(0), move down → OnVar{0,0}, move down → OnObjectField{0,0,[0]}
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0
            }
        );
        state.move_down();
        assert_eq!(
            state.cursor,
            StackCursor::OnObjectField {
                frame_idx: 0,
                var_idx: 0,
                field_path: vec![0],
            }
        );
    }

    #[test]
    fn move_down_past_last_object_field_moves_to_next_frame() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(99, fields);
        // flat: [Frame(0), Var{0,0}, Field{0,0,0}, Frame(1)]
        state.move_down(); // Frame → Var
        state.move_down(); // Var → Field
        state.move_down(); // Field → Frame(1)
        assert_eq!(state.cursor, StackCursor::OnFrame(1));
    }

    #[test]
    fn selected_loading_object_id_on_loading_node_returns_object_id() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 42)];
        state.toggle_expand(10, vars);
        state.set_expansion_loading(42);
        // move to the loading node
        state.move_down(); // → OnVar{0,0}
        state.move_down(); // → OnObjectLoadingNode{0,0,field_path:[]}
        assert_eq!(state.selected_loading_object_id(), Some(42));
    }

    // --- Task 4.5 / 5.5: depth-2 navigation and indentation tests ---

    #[test]
    fn flat_items_depth2_expansion_emits_correct_cursor_sequence() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // Root object 100 has one ObjectRef field pointing to object 200.
        let fields_100 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        // Object 200 has one Int field.
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(200, fields_200);

        let flat = state.flat_items();
        // Expected: Frame(0), Var{0,0}, Field{0,0,[0]}, Field{0,0,[0,0]}
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
    }

    #[test]
    fn selected_field_ref_id_returns_object_ref_id_for_nested_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Bar".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields);
        // Navigate to the field at path [0]
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        assert_eq!(state.selected_field_ref_id(), Some(200));
    }

    #[test]
    fn selected_field_ref_id_returns_none_for_non_object_ref_field() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(100, fields);
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };
        assert_eq!(state.selected_field_ref_id(), None);
    }

    // --- Task 7.4: recursive collapse tests ---

    #[test]
    fn collapse_object_recursive_removes_nested_expanded_child() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        // Expand root 100 → child 200
        let fields_100 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(1),
        }];
        state.set_expansion_done(200, fields_200);

        assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Expanded);

        state.collapse_object_recursive(100);

        assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
    }

    #[test]
    fn string_ids_in_subtree_collects_string_ref_ids_from_descendants() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        // Root 100 has a StringRef field 42 and an ObjectRef child 200.
        let fields_100 = vec![
            FieldInfo {
                name: "label".to_string(),
                value: FieldValue::StringRef { id: 42 },
            },
            FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 200,
                    class_name: "Foo".to_string(),
                    entry_count: None,
                },
            },
        ];
        state.set_expansion_done(100, fields_100);
        // Child 200 has a StringRef field 99.
        let fields_200 = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 99 },
        }];
        state.set_expansion_done(200, fields_200);

        let mut ids = state.string_ids_in_subtree(100);
        ids.sort();
        assert_eq!(ids, vec![42, 99]);
    }

    #[test]
    fn collapse_object_recursive_cycle_guard_does_not_infinite_loop() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        // Artificial cycle: 100 → 200 → 100 (corrupted heap)
        let fields_100 = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "A".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_100);
        let fields_200 = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "B".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(200, fields_200);
        // Must terminate without stack overflow
        state.collapse_object_recursive(100);
        assert_eq!(state.expansion_state(100), ExpansionPhase::Collapsed);
        assert_eq!(state.expansion_state(200), ExpansionPhase::Collapsed);
    }

    // --- Task 5: StringPhase and string state tests ---

    #[test]
    fn string_phase_returns_unloaded_for_unknown_id() {
        let state = StackState::new(vec![]);
        assert_eq!(state.string_phase(42), StringPhase::Unloaded);
    }

    #[test]
    fn start_string_loading_sets_loading_phase() {
        let mut state = StackState::new(vec![]);
        state.start_string_loading(42);
        assert_eq!(state.string_phase(42), StringPhase::Loading);
    }

    #[test]
    fn set_string_loaded_sets_loaded_phase_and_value() {
        let mut state = StackState::new(vec![]);
        state.set_string_loaded(42, "hello".to_string());
        assert_eq!(state.string_phase(42), StringPhase::Loaded);
        assert_eq!(
            state.string_values.get(&42).map(|s| s.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn set_string_failed_sets_failed_phase_and_error() {
        let mut state = StackState::new(vec![]);
        state.set_string_failed(42, "unresolved".to_string());
        assert_eq!(state.string_phase(42), StringPhase::Failed);
    }

    #[test]
    fn build_items_string_ref_unloaded_shows_placeholder() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 200 },
        }];
        state.set_expansion_done(99, fields);
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(
            text.contains("\"...\""),
            "unloaded string must show placeholder, got: {text:?}"
        );
    }

    #[test]
    fn build_items_string_ref_loaded_shows_value() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 200 },
        }];
        state.set_expansion_done(99, fields);
        state.set_string_loaded(200, "hello".to_string());
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(
            text.contains("\"hello\""),
            "loaded string must show value, got: {text:?}"
        );
    }

    #[test]
    fn build_items_string_ref_loaded_long_value_shows_truncated() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 200 },
        }];
        state.set_expansion_done(99, fields);
        let long_str = "a".repeat(101);
        state.set_string_loaded(200, long_str);
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(
            text.contains(".."),
            "long string must be truncated with .., got: {text:?}"
        );
    }

    #[test]
    fn build_items_string_ref_failed_shows_unresolved() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 200 },
        }];
        state.set_expansion_done(99, fields);
        state.set_string_failed(200, "unresolved".to_string());
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(
            text.contains("<unresolved>"),
            "failed string must show <unresolved>, got: {text:?}"
        );
    }

    #[test]
    fn collapse_object_recursive_clears_string_state_for_string_ref_fields() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let fields = vec![FieldInfo {
            name: "name".to_string(),
            value: FieldValue::StringRef { id: 200 },
        }];
        state.set_expansion_done(99, fields);
        state.set_string_loaded(200, "hello".to_string());
        assert_eq!(state.string_phase(200), StringPhase::Loaded);
        state.collapse_object_recursive(99);
        assert_eq!(state.string_phase(200), StringPhase::Unloaded);
    }

    // --- Task 8.2: frame collapse clears nested expansion ---

    #[test]
    fn toggle_expand_collapse_frame_clears_nested_object_phases() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // Expand object 100 (nested)
        let fields_100 = vec![FieldInfo {
            name: "x".to_string(),
            value: FieldValue::Int(1),
        }];
        state.set_expansion_done(100, fields_100);
        assert_eq!(state.expansion_state(100), ExpansionPhase::Expanded);
        // Collapse the frame
        state.toggle_expand(10, vec![]);
        // object_phases must be cleaned up
        assert!(state.object_phases.is_empty());
    }

    // --- Task 5.5: build_items indentation test ---

    fn item_text(item: ListItem<'static>) -> String {
        use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
        let area = Rect::new(0, 0, 120, 1);
        let mut buf = Buffer::empty(area);
        Widget::render(List::new(vec![item]), area, &mut buf);
        buf.content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn build_items_depth1_field_has_correct_indent() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "count".to_string(),
            value: FieldValue::Int(5),
        }];
        state.set_expansion_done(99, fields);
        let items = state.build_items();
        // items[0] = frame, items[1] = var, items[2] = field
        assert_eq!(items.len(), 3);
        let text = item_text(items[2].clone());
        // 4-space indent + 2-char toggle prefix ("  " for primitives)
        assert!(
            text.starts_with("      ") && !text.starts_with("        "),
            "depth-1 field must have 4+2 indent, got: {text:?}"
        );
    }

    #[test]
    fn build_items_depth2_field_has_correct_indent() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        let fields_99 = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "Foo".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(99, fields_99);
        let fields_200 = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(7),
        }];
        state.set_expansion_done(200, fields_200);
        let items = state.build_items();
        // items[0]=frame, [1]=var, [2]=depth-1 field, [3]=depth-2 field
        assert_eq!(items.len(), 4);
        let depth1 = item_text(items[2].clone());
        // 4-space indent + 2-char toggle ("- " for expanded ObjectRef)
        assert!(
            depth1.starts_with("    - "),
            "depth-1 ObjectRef field must have toggle prefix, got: {depth1:?}"
        );
        let depth2 = item_text(items[3].clone());
        // 6-space indent + 2-char toggle ("  " for primitive)
        assert!(
            depth2.starts_with("        ") && !depth2.starts_with("          "),
            "depth-2 field must have 6+2 indent, got: {depth2:?}"
        );
    }

    #[test]
    fn build_items_failed_expansion_shows_error_message_with_correct_indent() {
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 99)];
        state.toggle_expand(10, vars);
        state.set_expansion_failed(99, "Failed to resolve object".to_string());
        let items = state.build_items();
        // items[0]=frame, [1]=var, [2]=error node at depth 1 (4 spaces)
        assert_eq!(items.len(), 3);
        let text = item_text(items[2].clone());
        assert!(
            text.starts_with("    "),
            "error node must have 4-space indent, got: {text:?}"
        );
        assert!(
            text.contains("! Failed to resolve object"),
            "error node must contain error message, got: {text:?}"
        );
    }

    // --- Cyclic reference detection tests ---

    #[test]
    fn flat_items_self_ref_emits_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "self".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "Node".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 1);
        let deep_fields = flat
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    StackCursor::OnObjectField {
                        field_path, ..
                    } if field_path.len() > 1
                )
            })
            .count();
        assert_eq!(deep_fields, 0, "no recursive fields beyond depth 1");
    }

    #[test]
    fn flat_items_multi_self_ref_emits_two_cyclic_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "left".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            },
            FieldInfo {
                name: "right".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            },
        ];
        state.set_expansion_done(100, fields);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 2);
    }

    #[test]
    fn build_items_self_ref_renders_self_ref_marker() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![FieldInfo {
            name: "me".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "java.lang.Thread".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields);
        let items = state.build_items();
        let text = item_text(items[2].clone());
        assert!(text.contains("\u{21BB}"), "must contain ↻, got: {text:?}");
        assert!(
            text.contains("[self-ref]"),
            "must contain [self-ref], got: {text:?}"
        );
        assert!(
            text.contains("Thread"),
            "must show short class name, got: {text:?}"
        );
        assert!(
            !text.contains("java.lang.Thread"),
            "must NOT show FQCN, got: {text:?}"
        );
    }

    #[test]
    fn flat_items_indirect_cycle_emits_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) → B(200)
        let fields_a = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        // B(200) → A(100) (back-reference)
        let fields_b = vec![FieldInfo {
            name: "parent".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "A".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 1, "B's back-ref to A should be cyclic");
        // Should not recurse 16 levels deep
        let max_depth = flat
            .iter()
            .filter_map(|c| match c {
                StackCursor::OnObjectField { field_path, .. } => Some(field_path.len()),
                StackCursor::OnCyclicNode { field_path, .. } => Some(field_path.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(max_depth <= 3, "no deep recursion, max depth: {max_depth}");
    }

    #[test]
    fn build_items_indirect_cycle_renders_cyclic_marker() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields_a = vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        let fields_b = vec![FieldInfo {
            name: "parent".to_string(),
            value: FieldValue::ObjectRef {
                id: 100,
                class_name: "A".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let items = state.build_items();
        let all_text: Vec<String> = items.into_iter().map(item_text).collect();
        let cyclic_line = all_text.iter().find(|t| t.contains("[cyclic]"));
        assert!(
            cyclic_line.is_some(),
            "must have [cyclic] marker, items: {all_text:?}"
        );
        let line = cyclic_line.unwrap();
        assert!(line.contains("\u{21BB}"), "must contain ↻, got: {line:?}");
        assert!(
            !line.contains("[self-ref]"),
            "indirect cycle must NOT show [self-ref]"
        );
    }

    #[test]
    fn move_down_up_across_cyclic_node() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        let fields = vec![
            FieldInfo {
                name: "a".to_string(),
                value: FieldValue::Int(1),
            },
            FieldInfo {
                name: "b".to_string(),
                value: FieldValue::ObjectRef {
                    id: 100,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            },
            FieldInfo {
                name: "c".to_string(),
                value: FieldValue::Int(3),
            },
        ];
        state.set_expansion_done(100, fields);
        // flat: Frame(0), Var{0,0}, Field[0](Int),
        //       CyclicNode[1](self-ref), Field[2](Int)
        state.move_down(); // Frame → Var
        state.move_down(); // Var → Field[0]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![0]
        ));
        state.move_down(); // Field[0] → CyclicNode[1]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnCyclicNode { field_path, .. }
            if *field_path == vec![1]
        ));
        state.move_down(); // CyclicNode[1] → Field[2]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![2]
        ));
        // Now go back up
        state.move_up(); // Field[2] → CyclicNode[1]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnCyclicNode { field_path, .. }
            if *field_path == vec![1]
        ));
        state.move_up(); // CyclicNode[1] → Field[0]
        assert!(matches!(
            &state.cursor,
            StackCursor::OnObjectField { field_path, .. }
            if *field_path == vec![0]
        ));
    }

    #[test]
    fn flat_items_acyclic_tree_no_cyclic_nodes() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) → B(200) → C(300), no cycles
        let fields_a = vec![FieldInfo {
            name: "b".to_string(),
            value: FieldValue::ObjectRef {
                id: 200,
                class_name: "B".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(100, fields_a);
        let fields_b = vec![FieldInfo {
            name: "c".to_string(),
            value: FieldValue::ObjectRef {
                id: 300,
                class_name: "C".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(200, fields_b);
        let fields_c = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(300, fields_c);
        let flat = state.flat_items();
        let cyclic_count = flat
            .iter()
            .filter(|c| matches!(c, StackCursor::OnCyclicNode { .. }))
            .count();
        assert_eq!(cyclic_count, 0, "acyclic tree must have zero cyclic nodes");
        // Should have fields at depths 1, 2, 3
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0, 0],
        }));
    }

    #[test]
    fn flat_items_diamond_shared_object_no_false_positive() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 100)];
        state.toggle_expand(10, vars);
        // A(100) has two fields both pointing to C(300)
        let fields_a = vec![
            FieldInfo {
                name: "left".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "C".to_string(),
                    entry_count: None,
                },
            },
            FieldInfo {
                name: "right".to_string(),
                value: FieldValue::ObjectRef {
                    id: 300,
                    class_name: "C".to_string(),
                    entry_count: None,
                },
            },
        ];
        state.set_expansion_done(100, fields_a);
        let fields_c = vec![FieldInfo {
            name: "val".to_string(),
            value: FieldValue::Int(42),
        }];
        state.set_expansion_done(300, fields_c);
        let flat = state.flat_items();
        // C is shared but NOT an ancestor — no cyclic nodes
        let cyclic_count = flat
            .iter()
            .filter(|c| {
                matches!(c, StackCursor::OnCyclicNode { .. })
            })
            .count();
        assert_eq!(
            cyclic_count, 0,
            "diamond/shared object must not be a false positive"
        );
        // C's field should appear under both left and right
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0, 0],
        }));
        assert!(flat.contains(&StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![1, 0],
        }));
    }

    #[test]
    fn collapse_cyclic_child_resyncs_cursor_to_var() {
        use hprof_engine::{FieldInfo, FieldValue};
        let frames = vec![make_frame(10), make_frame(20)];
        let mut state = StackState::new(frames);
        let vars = vec![make_var_object_ref(0, 1000)];
        state.toggle_expand(10, vars);
        // Thread(1000) → parkBlocker field → Coroutine(2000)
        let thread_fields = vec![FieldInfo {
            name: "parkBlocker".to_string(),
            value: FieldValue::ObjectRef {
                id: 2000,
                class_name: "Coroutine".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(1000, thread_fields);
        // Coroutine(2000) → blockedThread → Thread(1000) (cycle)
        let coroutine_fields = vec![FieldInfo {
            name: "blockedThread".to_string(),
            value: FieldValue::ObjectRef {
                id: 1000,
                class_name: "Thread".to_string(),
                entry_count: None,
            },
        }];
        state.set_expansion_done(2000, coroutine_fields);

        // Navigate to parkBlocker field (path [0])
        state.cursor = StackCursor::OnObjectField {
            frame_idx: 0,
            var_idx: 0,
            field_path: vec![0],
        };

        // Collapse the nested Coroutine object.
        // collect_descendants(2000) follows back-ref to 1000,
        // collapsing BOTH objects. Cursor becomes orphaned.
        state.collapse_object_recursive(2000);

        // Cursor must have been resynced — not stuck
        let flat = state.flat_items();
        assert!(
            flat.contains(&state.cursor),
            "cursor must be in flat_items after collapse, got: {:?}",
            state.cursor,
        );
        // Should have fallen back to OnVar
        assert!(
            matches!(
                &state.cursor,
                StackCursor::OnVar {
                    frame_idx: 0,
                    var_idx: 0,
                }
            ),
            "cursor should fall back to OnVar, got: {:?}",
            state.cursor,
        );

        // Navigation must work again
        state.move_down();
        assert_ne!(
            state.cursor,
            StackCursor::OnVar {
                frame_idx: 0,
                var_idx: 0,
            },
            "move_down must move away from OnVar"
        );
    }
}
