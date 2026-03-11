//! Formatting helpers for the stack frame panel.
//!
//! Free functions that convert engine types to display strings and styles.

use std::collections::{HashMap, HashSet};

use hprof_engine::{EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber};
use ratatui::style::Style;

use crate::theme::THEME;

use super::types::ExpansionPhase;

/// Computes chunk ranges for a collection with
/// `total_count` entries.
///
/// Returns `(offset, limit)` pairs following the
/// 100/100/1000 chunking rules:
/// - `<= 100`: no sections (all eager)
/// - `101..=1000`: sections of 100
/// - `> 1000`: sections of 100 up to 1000, then
///   sections of 1000
pub fn compute_chunk_ranges(total_count: u64) -> Vec<(usize, usize)> {
    if total_count <= 100 {
        return vec![];
    }
    let total = total_count as usize;
    let mut ranges = Vec::new();
    // Sections of 100 from 100 up to min(1000, total)
    let boundary = total.min(1000);
    let mut offset = 100;
    while offset < boundary {
        let limit = (boundary - offset).min(100);
        ranges.push((offset, limit));
        offset += 100;
    }
    // Sections of 1000 from 1000 onward
    offset = 1000;
    while offset < total {
        let limit = (total - offset).min(1000);
        ranges.push((offset, limit));
        offset += 1000;
    }
    ranges
}

/// Collects all descendant object IDs reachable from `root_id` in depth-first
/// post-order. Cycles are broken via `visited`.
pub(crate) fn collect_descendants(
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

/// Formats a collapsed [`FieldValue::ObjectRef`] as `ClassName` or
/// `ClassName (N entries)` for collections.
pub(crate) fn format_object_ref_collapsed(class_name: &str, entry_count: Option<u64>) -> String {
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

/// Formats a [`FieldValue`] for display in field rows.
pub(crate) fn format_field_value_display(v: &FieldValue, phase: Option<&ExpansionPhase>) -> String {
    match v {
        FieldValue::Null => "null".to_string(),
        FieldValue::ObjectRef {
            class_name,
            entry_count,
            inline_value,
            ..
        } => {
            let base = match phase {
                Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => {
                    let display_name = if class_name.is_empty() {
                        "Object"
                    } else {
                        class_name
                    };
                    display_name
                        .rsplit('.')
                        .next()
                        .unwrap_or(display_name)
                        .to_string()
                }
                _ => format_object_ref_collapsed(class_name, *entry_count),
            };
            match inline_value {
                Some(v) => format!("{base} = {v}"),
                None => base,
            }
        }
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

/// Formats a `FieldValue` for inline display in collection entries.
pub(crate) fn format_entry_value_text(v: &FieldValue) -> String {
    match v {
        FieldValue::Null => "null".to_string(),
        FieldValue::ObjectRef {
            class_name,
            entry_count,
            inline_value,
            ..
        } => {
            let display_name = if class_name.is_empty() {
                "Object"
            } else {
                class_name
            };
            let short = display_name.rsplit('.').next().unwrap_or(display_name);
            let base = match entry_count {
                Some(n) => format!("{short} ({n} entries)"),
                None => short.to_string(),
            };
            match inline_value {
                Some(v) => format!("{base} = {v}"),
                None => base,
            }
        }
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

/// Returns the [`Style`] to apply to a rendered [`FieldValue`] row.
pub(crate) fn field_value_style(v: &FieldValue) -> Style {
    match v {
        FieldValue::Null => THEME.null_value,
        FieldValue::Bool(_)
        | FieldValue::Byte(_)
        | FieldValue::Short(_)
        | FieldValue::Int(_)
        | FieldValue::Long(_)
        | FieldValue::Float(_)
        | FieldValue::Double(_) => THEME.primitive_value,
        FieldValue::Char(_) => THEME.string_value,
        FieldValue::ObjectRef {
            inline_value: Some(_),
            ..
        } => THEME.string_value,
        FieldValue::ObjectRef { .. } => Style::new(),
    }
}

/// Formats one collection entry as a display line.
///
/// `value_phase` controls the expand toggle for `ObjectRef` values:
/// pass the current [`ExpansionPhase`] of the entry's value object
/// so that `+` / `-` is rendered correctly.
pub(crate) fn format_entry_line(
    entry: &EntryInfo,
    indent: &str,
    value_phase: Option<&ExpansionPhase>,
) -> String {
    let toggle = match value_phase {
        Some(ExpansionPhase::Expanded) | Some(ExpansionPhase::Loading) => "- ",
        Some(ExpansionPhase::Failed) => "! ",
        Some(ExpansionPhase::Collapsed) => "+ ",
        None => "  ",
    };
    let val = format_entry_value_text(&entry.value);
    if let Some(key) = &entry.key {
        let k = format_entry_value_text(key);
        format!("{indent}{toggle}[{}] {} => {}", entry.index, k, val)
    } else {
        format!("{indent}{toggle}[{}] {}", entry.index, val)
    }
}

pub(crate) fn format_frame_label(frame: &FrameInfo) -> String {
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
