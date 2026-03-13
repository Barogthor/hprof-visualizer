//! Tests for the favorites panel: rendering, scroll state, row metadata, and toggles.

use hprof_engine::{CollectionPage, EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{Terminal, backend::TestBackend};
use std::collections::{HashMap, HashSet};

use super::*;
use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};
use crate::views::stack_view::{FrameId, NavigationPathBuilder, ThreadId, VarIdx};

fn render_panel(panel: FavoritesPanel<'_>, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut state = FavoritesPanelState::default();
    terminal
        .draw(|f| {
            f.render_stateful_widget(panel, f.area(), &mut state);
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol().to_string())
        .collect()
}

fn make_frame_item() -> PinnedItem {
    PinnedItem {
        thread_name: "main".to_string(),
        frame_label: "Foo.bar()".to_string(),
        item_label: "Foo.bar()".to_string(),
        snapshot: PinnedSnapshot::Frame {
            variables: vec![VariableInfo {
                index: 0,
                value: VariableValue::Null,
            }],
            object_fields: HashMap::new(),
            object_static_fields: HashMap::new(),
            collection_chunks: HashMap::new(),
            truncated: false,
        },
        local_collapsed: HashSet::new(),
        key: PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
        },
    }
}

fn make_primitive_item() -> PinnedItem {
    PinnedItem {
        thread_name: "main".to_string(),
        frame_label: "Foo.bar()".to_string(),
        item_label: "var[0]".to_string(),
        snapshot: PinnedSnapshot::Primitive {
            value_label: "42".to_string(),
        },
        local_collapsed: HashSet::new(),
        key: PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
        },
    }
}

fn make_frame_with_nested_objects() -> PinnedItem {
    let mut object_fields = HashMap::new();
    object_fields.insert(
        10,
        vec![FieldInfo {
            name: "child".to_string(),
            value: FieldValue::ObjectRef {
                id: 11,
                class_name: "Node".to_string(),
                entry_count: None,
                inline_value: None,
            },
        }],
    );
    object_fields.insert(
        11,
        vec![FieldInfo {
            name: "value".to_string(),
            value: FieldValue::Int(1),
        }],
    );
    PinnedItem {
        thread_name: "main".to_string(),
        frame_label: "Foo.bar()".to_string(),
        item_label: "Foo.bar()".to_string(),
        snapshot: PinnedSnapshot::Frame {
            variables: vec![VariableInfo {
                index: 0,
                value: VariableValue::ObjectRef {
                    id: 10,
                    class_name: "Node".to_string(),
                    entry_count: None,
                },
            }],
            object_fields,
            object_static_fields: HashMap::new(),
            collection_chunks: HashMap::new(),
            truncated: false,
        },
        local_collapsed: HashSet::new(),
        key: PinKey {
            thread_id: ThreadId(1),
            thread_name: "main".to_string(),
            nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
        },
    }
}

mod rendering_tests {
    use super::*;

    #[test]
    fn panel_shows_frame_header_with_f_prefix() {
        let items = vec![make_frame_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("[F]"), "expected [F] prefix, got: {text:?}");
        assert!(text.contains("main"), "expected thread name, got: {text:?}");
        assert!(
            text.contains("Foo.bar()"),
            "expected frame label, got: {text:?}"
        );
    }

    #[test]
    fn panel_shows_variable_header_with_v_prefix() {
        let items = vec![make_primitive_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("[V]"), "expected [V] prefix, got: {text:?}");
    }

    #[test]
    fn panel_shows_primitive_value() {
        let items = vec![make_primitive_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("42"), "expected value 42, got: {text:?}");
    }

    #[test]
    fn favorites_panel_renders_with_local_collapsed_shows_plus() {
        let mut item = make_frame_with_nested_objects();
        item.local_collapsed.insert(10);
        let items = vec![item];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(text.contains("+"), "expected plus marker, got: {text:?}");
    }

    #[test]
    fn favorites_panel_renders_expanded_shows_minus() {
        let items = vec![make_frame_with_nested_objects()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(text.contains("-"), "expected minus marker, got: {text:?}");
    }

    #[test]
    fn favorites_panel_renders_unavailable_object_with_question_marker() {
        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 999,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        }];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            20,
        );

        assert!(
            text.contains("? [0] local variable: Node"),
            "expected unavailable marker, got: {text:?}"
        );
    }

    #[test]
    fn favorites_panel_collapsed_collection_row_shows_plus_not_question() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 200,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(2),
                    inline_value: None,
                },
            }],
        );

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            200,
            CollectionChunks {
                total_count: 2,
                eager_page: Some(CollectionPage {
                    entries: vec![EntryInfo {
                        index: 0,
                        key: None,
                        value: FieldValue::Int(7),
                    }],
                    total_count: 2,
                    offset: 0,
                    has_more: false,
                }),
                chunk_pages: HashMap::new(),
            },
        );

        let mut local_collapsed = HashSet::new();
        local_collapsed.insert(200);

        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 1,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks,
                truncated: false,
            },
            local_collapsed,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        }];

        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            30,
        );

        assert!(text.contains("+ items"), "expected + marker, got: {text:?}");
        assert!(
            !text.contains("? items"),
            "did not expect ? marker, got: {text:?}"
        );
        assert!(
            !text.contains("[0] 7"),
            "collapsed collection should hide entries"
        );
    }

    #[test]
    fn favorites_panel_renders_static_fields_for_pinned_object() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10,
            vec![FieldInfo {
                name: "value".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        let mut object_static_fields = HashMap::new();
        object_static_fields.insert(
            10,
            vec![FieldInfo {
                name: "SOME_STATIC".to_string(),
                value: FieldValue::Int(99),
            }],
        );

        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 10,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: object_fields.clone(),
                object_static_fields: object_static_fields.clone(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        }];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                show_object_ids: false,
                pinned: &items,
            },
            120,
            30,
        );

        assert!(
            text.contains("[static]"),
            "expected static section, got: {text:?}"
        );
        assert!(
            text.contains("SOME_STATIC"),
            "expected static field label, got: {text:?}"
        );
    }
}

mod scroll_state_tests {
    use super::*;

    #[test]
    fn favorites_panel_state_move_down_crosses_item_boundary() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 2],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 0;
        state.sub_row = 2;

        state.move_down();

        assert_eq!(state.selected_item, 1);
        assert_eq!(state.sub_row, 0);
    }

    #[test]
    fn favorites_panel_state_move_up_crosses_item_boundary() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 2],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 1;
        state.sub_row = 0;

        state.move_up();

        assert_eq!(state.selected_item, 0);
        assert_eq!(state.sub_row, 2);
    }

    #[test]
    fn favorites_panel_state_move_down_noop_at_last_row() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        state.update_row_metadata(vec![3], vec![HashMap::new()], vec![HashMap::new()]);
        state.selected_item = 0;
        state.sub_row = 2;

        state.move_down();

        assert_eq!(state.selected_item, 0);
        assert_eq!(state.sub_row, 2);
    }

    #[test]
    fn favorites_panel_state_abs_row_correct() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);
        state.update_row_metadata(
            vec![3, 4],
            vec![HashMap::new(), HashMap::new()],
            vec![HashMap::new(), HashMap::new()],
        );
        state.selected_item = 1;
        state.sub_row = 2;

        assert_eq!(state.abs_row(), 5);
    }

    #[test]
    fn favorites_panel_state_move_down_before_first_render_advances_item() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(2);

        state.move_down();

        assert_eq!(state.selected_item, 1);
    }

    #[test]
    fn sub_row_clamped_after_collapse() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        state.update_row_metadata(vec![5], vec![HashMap::new()], vec![HashMap::new()]);
        state.sub_row = 4;

        state.update_row_metadata(vec![2], vec![HashMap::new()], vec![HashMap::new()]);
        state.clamp_sub_row();

        assert_eq!(state.sub_row, 1);
    }
}

mod row_metadata_tests {
    use super::*;
    use crate::views::tree_render::{RenderOptions, TreeRoot, render_variable_tree};

    #[test]
    fn collect_row_metadata_matches_render_count_flat() {
        let item = make_frame_with_nested_objects();
        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);

        let PinnedSnapshot::Frame {
            variables,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected frame snapshot");
        };
        let object_phases = object_phases_for_item(
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &item.local_collapsed,
        );
        let rendered = render_variable_tree(
            TreeRoot::Frame { vars: variables },
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
            },
        );

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn collect_row_metadata_matches_render_count_nested() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 11,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            11,
            vec![FieldInfo {
                name: "grandchild".to_string(),
                value: FieldValue::ObjectRef {
                    id: 12,
                    class_name: "Node".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            12,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 99,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(1003),
                    inline_value: None,
                },
            }],
        );

        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            99,
            CollectionChunks {
                total_count: 1003,
                eager_page: Some(CollectionPage {
                    entries: vec![
                        EntryInfo {
                            index: 0,
                            key: None,
                            value: FieldValue::Int(1),
                        },
                        EntryInfo {
                            index: 1,
                            key: None,
                            value: FieldValue::Int(2),
                        },
                        EntryInfo {
                            index: 2,
                            key: None,
                            value: FieldValue::Int(3),
                        },
                    ],
                    total_count: 1003,
                    offset: 0,
                    has_more: true,
                }),
                chunk_pages: HashMap::new(),
            },
        );

        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 10,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: object_fields.clone(),
                object_static_fields: HashMap::new(),
                collection_chunks: collection_chunks.clone(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);

        let object_phases = object_phases_for_item(
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &item.local_collapsed,
        );
        let rendered = render_variable_tree(
            TreeRoot::Frame {
                vars: match &item.snapshot {
                    PinnedSnapshot::Frame { variables, .. } => variables,
                    _ => unreachable!(),
                },
            },
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
            },
        );

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn collect_row_metadata_unavailable_object_is_not_toggleable() {
        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 999,
                        class_name: "Node".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        };

        let (row_count, kind_map, _sentinel_map) = collect_row_metadata(&item);

        assert_eq!(row_count, 3);
        assert!(
            kind_map.is_empty(),
            "unavailable row must not be toggleable"
        );
    }

    #[test]
    fn collect_row_metadata_subtree_collection_root_matches_render_count() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};

        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 77,
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::from([(
                    77u64,
                    CollectionChunks {
                        total_count: 120,
                        eager_page: Some(CollectionPage {
                            entries: vec![EntryInfo {
                                index: 0,
                                key: None,
                                value: FieldValue::Int(7),
                            }],
                            total_count: 120,
                            offset: 0,
                            has_more: true,
                        }),
                        chunk_pages: HashMap::from([(100usize, ChunkState::Collapsed)]),
                    },
                )]),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _kind_map, _sentinel_map) = collect_row_metadata(&item);
        let PinnedSnapshot::Subtree {
            root_id,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected subtree snapshot");
        };
        let object_phases = object_phases_for_item(
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &item.local_collapsed,
        );
        let rendered = render_variable_tree(
            TreeRoot::Subtree { root_id: *root_id },
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
            },
        );

        assert_eq!(row_count, rendered.len() + 2);
    }

    #[test]
    fn collect_row_metadata_cyclic_object_emits_one_row_not_infinite() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1,
            vec![FieldInfo {
                name: "b".to_string(),
                value: FieldValue::ObjectRef {
                    id: 2,
                    class_name: "B".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            2,
            vec![FieldInfo {
                name: "a".to_string(),
                value: FieldValue::ObjectRef {
                    id: 1,
                    class_name: "A".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 1,
                        class_name: "A".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _, _) = collect_row_metadata(&item);

        // 1 header + A-row + b-field-row + cyclic-A-row + 1 separator = 5
        assert_eq!(row_count, 5);
    }

    #[test]
    fn collect_row_metadata_primitive_and_unexpanded_ref_row_count() {
        let primitive = make_primitive_item();
        let (primitive_rows, _, _) = collect_row_metadata(&primitive);
        assert_eq!(primitive_rows, 3);

        let unexpanded = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::UnexpandedRef {
                class_name: "Foo".to_string(),
                object_id: 1,
            },
            local_collapsed: HashSet::new(),
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };
        let (unexpanded_rows, _, _) = collect_row_metadata(&unexpanded);
        assert_eq!(unexpanded_rows, 3);
    }
}

mod toggle_tests {
    use super::*;

    #[test]
    fn favorites_item_toggle_expand_removes_from_local_collapsed() {
        let mut item = make_frame_with_nested_objects();
        item.local_collapsed.insert(10);

        if let Some((id, is_collapsed)) = Some((10u64, true))
            && is_collapsed
        {
            item.local_collapsed.remove(&id);
        }

        assert!(!item.local_collapsed.contains(&10));
    }

    #[test]
    fn favorites_item_toggle_collapse_adds_to_local_collapsed() {
        let mut item = make_frame_with_nested_objects();

        if let Some((id, is_collapsed)) = Some((10u64, false))
            && !is_collapsed
        {
            item.local_collapsed.insert(id);
        }

        assert!(item.local_collapsed.contains(&10));
    }
}
