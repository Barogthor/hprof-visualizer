//! Post-indexation coherence validation.
//!
//! Runs after the first pass completes, checking for
//! data anomalies (duplicate serials, hierarchy cycles)
//! and collecting warnings rather than failing.

use rustc_hash::FxHashSet;

use crate::indexer::IndexResult;

/// Maximum validation warnings before summarising.
const MAX_VALIDATION_WARNINGS: usize = 50;

/// Pushes a validation warning, respecting the cap.
fn push_validation_warning(warnings: &mut Vec<String>, suppressed: &mut usize, msg: String) {
    let validation_count = warnings
        .iter()
        .filter(|w| w.starts_with("validation:"))
        .count();
    if validation_count < MAX_VALIDATION_WARNINGS {
        warnings.push(msg);
    } else {
        *suppressed += 1;
    }
}

/// Validates the populated index for data coherence.
///
/// Appends warnings to `result.warnings` for any
/// anomalies found (capped at
/// [`MAX_VALIDATION_WARNINGS`]). Does not modify index
/// data.
pub(crate) fn validate_index(result: &mut IndexResult) {
    let mut suppressed = 0usize;
    check_class_object_id_consistency(result, &mut suppressed);
    check_super_class_cycles(result, &mut suppressed);
    if suppressed > 0 {
        result.warnings.push(format!(
            "validation: ... {suppressed} additional \
             validation warning(s) suppressed (only \
             first {MAX_VALIDATION_WARNINGS} shown)"
        ));
    }
}

fn check_class_object_id_consistency(result: &mut IndexResult, suppressed: &mut usize) {
    for class_def in result.index.classes.values() {
        let id = class_def.class_object_id;
        if !result.index.class_dumps.contains_key(&id)
            && result.index.class_names_by_id.contains_key(&id)
        {
            push_validation_warning(
                &mut result.warnings,
                suppressed,
                format!(
                    "validation: LOAD_CLASS \
                     class_object_id 0x{id:x} has no \
                     matching CLASS_DUMP sub-record"
                ),
            );
        }
    }
}

fn check_super_class_cycles(result: &mut IndexResult, suppressed: &mut usize) {
    let class_dumps = &result.index.class_dumps;
    let mut visited_global = FxHashSet::default();
    let mut in_cycle = FxHashSet::default();

    for &start_id in class_dumps.keys() {
        if visited_global.contains(&start_id) {
            continue;
        }
        let mut path = Vec::new();
        let mut path_set = FxHashSet::default();
        let mut current = start_id;

        loop {
            if visited_global.contains(&current) {
                break;
            }
            if path_set.contains(&current) {
                let cycle_start = path.iter().position(|&id| id == current).unwrap();
                for &cid in &path[cycle_start..] {
                    in_cycle.insert(cid);
                }
                break;
            }
            path.push(current);
            path_set.insert(current);

            match class_dumps.get(&current) {
                Some(info) if info.super_class_id != 0 => {
                    current = info.super_class_id;
                }
                _ => break,
            }
        }

        for &id in &path {
            visited_global.insert(id);
        }
    }

    if !in_cycle.is_empty() {
        let mut ids: Vec<_> = in_cycle.into_iter().collect();
        ids.sort_unstable();
        let formatted: Vec<_> = ids.iter().map(|id| format!("0x{id:x}")).collect();
        push_validation_warning(
            &mut result.warnings,
            suppressed,
            format!(
                "validation: super_class_id cycle \
                 detected among class IDs: [{}]",
                formatted.join(", ")
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::precise::PreciseIndex;
    use crate::types::{ClassDef, ClassDumpInfo};

    fn empty_result() -> IndexResult {
        IndexResult {
            index: PreciseIndex::new(),
            warnings: Vec::new(),
            records_attempted: 0,
            records_indexed: 0,
            segment_filters: Vec::new(),
            heap_record_ranges: Vec::new(),
            #[cfg(feature = "test-utils")]
            diagnostics: crate::indexer::DiagnosticInfo::default(),
        }
    }

    #[test]
    fn no_warnings_on_consistent_data() {
        let mut result = empty_result();
        result.index.classes.insert(
            1,
            ClassDef {
                class_serial: 1,
                class_object_id: 0xA,
                stack_trace_serial: 0,
                class_name_string_id: 0,
            },
        );
        result
            .index
            .class_names_by_id
            .insert(0xA, "java.lang.Object".to_string());
        result.index.class_dumps.insert(
            0xA,
            ClassDumpInfo {
                class_object_id: 0xA,
                super_class_id: 0,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![],
            },
        );
        validate_index(&mut result);
        assert!(
            result.warnings.is_empty(),
            "consistent data should produce no warnings"
        );
    }

    #[test]
    fn warns_on_class_object_id_not_in_class_dumps() {
        let mut result = empty_result();
        result.index.classes.insert(
            1,
            ClassDef {
                class_serial: 1,
                class_object_id: 0xA,
                stack_trace_serial: 0,
                class_name_string_id: 0,
            },
        );
        result
            .index
            .class_names_by_id
            .insert(0xA, "com.example.Foo".to_string());
        validate_index(&mut result);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| { w.contains("LOAD_CLASS") && w.contains("0xa") }),
            "should warn about missing CLASS_DUMP: {:?}",
            result.warnings
        );
    }

    #[test]
    fn warns_on_super_class_cycle() {
        let mut result = empty_result();
        result.index.class_dumps.insert(
            0xA,
            ClassDumpInfo {
                class_object_id: 0xA,
                super_class_id: 0xB,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![],
            },
        );
        result.index.class_dumps.insert(
            0xB,
            ClassDumpInfo {
                class_object_id: 0xB,
                super_class_id: 0xA,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![],
            },
        );
        validate_index(&mut result);
        assert!(
            result.warnings.iter().any(|w| w.contains("cycle")),
            "should warn about cycle: {:?}",
            result.warnings
        );
    }

    #[test]
    fn no_cycle_warning_for_valid_hierarchy() {
        let mut result = empty_result();
        result.index.class_dumps.insert(
            0xA,
            ClassDumpInfo {
                class_object_id: 0xA,
                super_class_id: 0xB,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![],
            },
        );
        result.index.class_dumps.insert(
            0xB,
            ClassDumpInfo {
                class_object_id: 0xB,
                super_class_id: 0,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![],
            },
        );
        validate_index(&mut result);
        assert!(
            !result.warnings.iter().any(|w| w.contains("cycle")),
            "valid hierarchy must not produce cycle warnings"
        );
    }
}
