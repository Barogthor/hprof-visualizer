//! Tests for the favorites panel: rendering, scroll state, row metadata, and toggles.

use hprof_engine::{CollectionPage, EntryInfo, FieldInfo, FieldValue, VariableInfo, VariableValue};
use ratatui::{Terminal, backend::TestBackend};
use std::collections::{HashMap, HashSet};

use super::*;
use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};
use crate::views::stack_view::{FrameId, NavigationPath, NavigationPathBuilder, ThreadId, VarIdx};

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
        hidden_fields: HashSet::new(),
        show_hidden: false,
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
        hidden_fields: HashSet::new(),
        show_hidden: false,
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
        hidden_fields: HashSet::new(),
        show_hidden: false,
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
        let path = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path);
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

        assert!(
            text.contains("+ "),
            "expected + marker for collapsed var[0], got: {text:?}"
        );
        assert!(
            !text.contains("child"),
            "collapsed var[0] must hide child field, got: {text:?}"
        );
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
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

        let mut local_collapsed: HashSet<NavigationPath> = HashSet::new();
        let collapse_path = NavigationPathBuilder::new(FrameId(0), VarIdx(0))
            .field(crate::views::stack_view::FieldIdx(0))
            .build();
        local_collapsed.insert(collapse_path);

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
            hidden_fields: HashSet::new(),
            show_hidden: false,
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

        assert!(
            text.contains("+ "),
            "collapsed collection field must show +, got: {text:?}"
        );
        assert!(
            !text.contains("? "),
            "collapsed collection must show + not ?, got: {text:?}"
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
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
            text.contains("[static fields]"),
            "expected static section header, got: {text:?}"
        );
        // Static fields are collapsed by default in snapshots
        // (AC #7) — SOME_STATIC should NOT be visible.
        assert!(
            !text.contains("SOME_STATIC"),
            "static field must be hidden in snapshot: {text:?}"
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
            vec![HashMap::new(), HashMap::new()],
            vec![vec![], vec![]],
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
            vec![HashMap::new(), HashMap::new()],
            vec![vec![], vec![]],
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
        state.update_row_metadata(
            vec![3],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![vec![]],
        );
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
            vec![HashMap::new(), HashMap::new()],
            vec![vec![], vec![]],
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
        state.update_row_metadata(
            vec![5],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![vec![]],
        );
        state.sub_row = 4;

        state.update_row_metadata(
            vec![2],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![vec![]],
        );
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
        let (row_count, _kind_map, _sentinel_map, _field_row_map, _) = collect_row_metadata(&item);

        let PinnedSnapshot::Frame {
            variables,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected frame snapshot");
        };
        let object_phases =
            object_phases_for_item(object_fields, &HashMap::new(), collection_chunks);
        let rendered = render_variable_tree(
            TreeRoot::Frame {
                vars: variables,
                frame_id: 100,
            },
            object_fields,
            &HashMap::new(),
            collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
            None,
            None,
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _kind_map, _sentinel_map, _field_row_map, _) = collect_row_metadata(&item);

        let object_phases =
            object_phases_for_item(&object_fields, &HashMap::new(), &collection_chunks);
        let rendered = render_variable_tree(
            TreeRoot::Frame {
                vars: match &item.snapshot {
                    PinnedSnapshot::Frame { variables, .. } => variables,
                    _ => unreachable!(),
                },
                frame_id: 100,
            },
            &object_fields,
            &HashMap::new(),
            &collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
            None,
            None,
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        };

        let (row_count, kind_map, _sentinel_map, _field_row_map, _) = collect_row_metadata(&item);

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
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _kind_map, _sentinel_map, _field_row_map, _) = collect_row_metadata(&item);
        let PinnedSnapshot::Subtree {
            root_id,
            object_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected subtree snapshot");
        };
        let object_phases =
            object_phases_for_item(object_fields, &HashMap::new(), collection_chunks);
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
                show_hidden: false,
            },
            None,
            None,
            None,
            None,
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _, _, _, _) = collect_row_metadata(&item);

        // 1 header + A-row + b-field-row + cyclic-A-row + 1 separator = 5
        assert_eq!(row_count, 5);
    }

    #[test]
    fn collect_row_metadata_primitive_and_unexpanded_ref_row_count() {
        let primitive = make_primitive_item();
        let (primitive_rows, _, _, _, _) = collect_row_metadata(&primitive);
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
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };
        let (unexpanded_rows, _, _, _, _) = collect_row_metadata(&unexpanded);
        assert_eq!(unexpanded_rows, 3);
    }
}

mod toggle_tests {
    use super::*;

    #[test]
    fn favorites_item_toggle_expand_removes_from_local_collapsed() {
        let mut item = make_frame_with_nested_objects();
        let path = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path.clone());

        item.local_collapsed.remove(&path);

        assert!(!item.local_collapsed.contains(&path));
    }

    #[test]
    fn favorites_item_toggle_collapse_adds_to_local_collapsed() {
        let mut item = make_frame_with_nested_objects();
        let path = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();

        item.local_collapsed.insert(path.clone());

        assert!(item.local_collapsed.contains(&path));
    }
}

mod hide_field_tests {
    use super::*;
    use crate::views::tree_render::{RenderOptions, TreeRoot, render_variable_tree};

    fn make_two_var_frame() -> PinnedItem {
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![
                    VariableInfo {
                        index: 0,
                        value: VariableValue::Null,
                    },
                    VariableInfo {
                        index: 1,
                        value: VariableValue::Null,
                    },
                ],
                object_fields: HashMap::new(),
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: crate::views::stack_view::ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: crate::views::stack_view::NavigationPathBuilder::frame_only(
                    crate::views::stack_view::FrameId(1),
                ),
            },
        }
    }

    fn make_subtree_with_objectref_child() -> PinnedItem {
        // root object 1 has one ObjectRef field (field_idx=0) pointing to child 2.
        // child 2 has two primitive fields.
        let mut object_fields = HashMap::new();
        object_fields.insert(
            1u64,
            vec![FieldInfo {
                name: "child".to_string(),
                value: FieldValue::ObjectRef {
                    id: 2,
                    class_name: "Inner".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        object_fields.insert(
            2u64,
            vec![
                FieldInfo {
                    name: "a".to_string(),
                    value: FieldValue::Int(1),
                },
                FieldInfo {
                    name: "b".to_string(),
                    value: FieldValue::Int(2),
                },
            ],
        );
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 1,
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: crate::views::stack_view::ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: crate::views::stack_view::NavigationPathBuilder::new(
                    crate::views::stack_view::FrameId(1),
                    crate::views::stack_view::VarIdx(0),
                )
                .build(),
            },
        }
    }

    // 6.9
    #[test]
    fn collect_row_metadata_field_row_map_populated() {
        let item = make_two_var_frame();
        let (_row_count, _kind_map, _sentinel_map, field_row_map, _) = collect_row_metadata(&item);

        assert_eq!(field_row_map.get(&1), Some(&(HideKey::Var(0), false)));
        assert_eq!(field_row_map.get(&2), Some(&(HideKey::Var(1), false)));
        assert_eq!(field_row_map.len(), 2);
    }

    // 6.10
    #[test]
    fn collect_row_metadata_hidden_var_row_shows_is_hidden_true() {
        let mut item = make_two_var_frame();
        item.hidden_fields.insert(HideKey::Var(0));

        // show_hidden=false (default): hidden var produces no row — absent from map.
        // var[1] shifts to sub_row 1.
        let (_row_count, _kind_map, _sentinel_map, field_row_map, _) = collect_row_metadata(&item);
        assert_eq!(field_row_map.get(&1), Some(&(HideKey::Var(1), false)));
        assert_eq!(field_row_map.get(&2), None);
        assert_eq!(field_row_map.len(), 1);

        // show_hidden=true: hidden var appears as placeholder at sub_row 1 with is_hidden=true.
        item.show_hidden = true;
        let (_row_count2, _kind_map2, _sentinel_map2, field_row_map2, _) =
            collect_row_metadata(&item);
        assert_eq!(field_row_map2.get(&1), Some(&(HideKey::Var(0), true)));
        assert_eq!(field_row_map2.get(&2), Some(&(HideKey::Var(1), false)));
    }

    // 6.11
    #[test]
    fn collect_row_metadata_hidden_objectref_row_count_decreases() {
        let item = make_subtree_with_objectref_child();

        let PinnedSnapshot::Subtree {
            root_id,
            object_fields,
            object_static_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected subtree");
        };

        // Baseline: nothing hidden.
        let (row_count_base, _, _, _, _) = collect_row_metadata(&item);
        let object_phases_base =
            object_phases_for_item(object_fields, object_static_fields, collection_chunks);
        let rendered_base = render_variable_tree(
            TreeRoot::Subtree { root_id: *root_id },
            object_fields,
            object_static_fields,
            collection_chunks,
            &object_phases_base,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
            None,
            None,
        );
        // header=1, ObjectRef row=1, 2 child primitives=2, separator=1 → row_count=5
        assert_eq!(row_count_base, 5);
        assert_eq!(row_count_base, rendered_base.len() + 2);

        // Hidden case: hide field_idx=0 of root object 1.
        let mut item_hidden = make_subtree_with_objectref_child();
        item_hidden.hidden_fields.insert(HideKey::Field {
            parent_id: 1,
            field_idx: 0,
        });
        let (row_count_hidden, _, _, _, _) = collect_row_metadata(&item_hidden);

        let PinnedSnapshot::Subtree {
            root_id: root_id2,
            object_fields: of2,
            object_static_fields: osf2,
            collection_chunks: cc2,
            ..
        } = &item_hidden.snapshot
        else {
            panic!("expected subtree");
        };
        let object_phases_hidden = object_phases_for_item(of2, osf2, cc2);
        // show_hidden=false: field + children completely absent → row_count=2
        let hide_set = item_hidden.hidden_fields.clone();
        let rendered_hidden = render_variable_tree(
            TreeRoot::Subtree { root_id: *root_id2 },
            of2,
            osf2,
            cc2,
            &object_phases_hidden,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            Some(&hide_set),
            None,
            None,
            None,
        );
        assert_eq!(
            row_count_hidden, 2,
            "show_hidden=false: header + separator only"
        );
        assert_eq!(row_count_hidden, rendered_hidden.len() + 2);

        // show_hidden=true: placeholder row → row_count=3
        let mut item_revealed = make_subtree_with_objectref_child();
        item_revealed.hidden_fields.insert(HideKey::Field {
            parent_id: 1,
            field_idx: 0,
        });
        item_revealed.show_hidden = true;
        let (row_count_revealed, _, _, _, _) = collect_row_metadata(&item_revealed);

        let PinnedSnapshot::Subtree {
            root_id: root_id3,
            object_fields: of3,
            object_static_fields: osf3,
            collection_chunks: cc3,
            ..
        } = &item_revealed.snapshot
        else {
            panic!("expected subtree");
        };
        let object_phases_revealed = object_phases_for_item(of3, osf3, cc3);
        let rendered_revealed = render_variable_tree(
            TreeRoot::Subtree { root_id: *root_id3 },
            of3,
            osf3,
            cc3,
            &object_phases_revealed,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: true,
            },
            Some(&item_revealed.hidden_fields),
            None,
            None,
            None,
        );
        assert_eq!(
            row_count_revealed, 3,
            "show_hidden=true: header + placeholder + separator"
        );
        assert_eq!(row_count_revealed, rendered_revealed.len() + 2);
    }

    // 6.12
    #[test]
    fn favorites_panel_state_field_key_at_cursor_correct() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        let mut field_row_map: FieldRowMap = HashMap::new();
        field_row_map.insert(1, (HideKey::Var(0), false));
        state.update_row_metadata(
            vec![3],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![field_row_map],
            vec![vec![]],
        );
        state.selected_item = 0;
        state.sub_row = 1;

        assert_eq!(state.field_key_at_cursor(), Some((HideKey::Var(0), false)));
    }

    // 6.13
    #[test]
    fn favorites_panel_state_field_key_at_cursor_none_for_header() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(1);
        let mut field_row_map: FieldRowMap = HashMap::new();
        field_row_map.insert(1, (HideKey::Var(0), false));
        state.update_row_metadata(
            vec![3],
            vec![HashMap::new()],
            vec![HashMap::new()],
            vec![field_row_map],
            vec![vec![]],
        );
        state.selected_item = 0;
        state.sub_row = 0;

        assert_eq!(state.field_key_at_cursor(), None);
    }

    // 6.17
    #[test]
    fn collect_row_metadata_truncated_offset_shifts_field_row_map() {
        let mut item = make_two_var_frame();
        if let PinnedSnapshot::Frame {
            ref mut truncated, ..
        } = item.snapshot
        {
            *truncated = true;
        }
        let (_row_count, _kind_map, _sentinel_map, field_row_map, _) = collect_row_metadata(&item);

        // row 0 = header, row 1 = truncated-warning, row 2 = var[0], row 3 = var[1]
        assert_eq!(field_row_map.get(&2), Some(&(HideKey::Var(0), false)));
        assert_eq!(field_row_map.get(&3), Some(&(HideKey::Var(1), false)));
        assert_eq!(field_row_map.get(&1), None);
    }
}

// ── Story 12.2: path-based collapse state ──────────────────────

mod path_based_collapse_tests {
    use super::*;
    use crate::views::stack_view::FieldIdx;
    use crate::views::tree_render::{RenderOptions, TreeRoot, render_variable_tree};

    /// Frame snapshot with object 0x1234 reachable at two paths:
    /// var[0] → 0x1234  and  var[1] → 0x1234.
    fn make_shared_object_frame() -> PinnedItem {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            0x1234u64,
            vec![FieldInfo {
                name: "x".to_string(),
                value: FieldValue::Int(1),
            }],
        );
        PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![
                    VariableInfo {
                        index: 0,
                        value: VariableValue::ObjectRef {
                            id: 0x1234,
                            class_name: "Obj".to_string(),
                            entry_count: None,
                        },
                    },
                    VariableInfo {
                        index: 1,
                        value: VariableValue::ObjectRef {
                            id: 0x1234,
                            class_name: "Obj".to_string(),
                            entry_count: None,
                        },
                    },
                ],
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        }
    }

    fn render_item(item: &PinnedItem) -> Vec<String> {
        let PinnedSnapshot::Frame {
            variables,
            object_fields,
            object_static_fields,
            collection_chunks,
            ..
        } = &item.snapshot
        else {
            panic!("expected frame snapshot");
        };
        let object_phases =
            object_phases_for_item(object_fields, object_static_fields, collection_chunks);
        let items = render_variable_tree(
            TreeRoot::Frame {
                vars: variables,
                frame_id: 0,
            },
            object_fields,
            object_static_fields,
            collection_chunks,
            &object_phases,
            &HashMap::new(),
            RenderOptions {
                show_object_ids: false,
                snapshot_mode: true,
                show_hidden: false,
            },
            None,
            None,
            Some(&item.local_collapsed),
            None,
        );
        items.iter().map(|li| format!("{li:?}")).collect()
    }

    // 5.1: collapse path A, path B stays expanded
    #[test]
    fn collapse_one_path_other_stays_expanded() {
        let mut item = make_shared_object_frame();
        let path_a = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path_a);

        let lines = render_item(&item);
        // var[0] should show + (collapsed)
        let var0 = &lines[0];
        assert!(
            var0.contains("+ "),
            "var[0] should be collapsed (+), got: {var0}"
        );
        // var[1] should show - (expanded) with child "x: 1"
        let var1 = &lines[1];
        assert!(
            var1.contains("- "),
            "var[1] should be expanded (-), got: {var1}"
        );
        // child field should exist for var[1]
        assert!(
            lines.iter().any(|l| l.contains("x: 1")),
            "expected x: 1 in output, got: {lines:?}"
        );
    }

    // 5.2: enum self-ref collapse in snapshot — independent
    #[test]
    fn enum_self_ref_collapse_independent() {
        let mut object_fields: HashMap<u64, Vec<FieldInfo>> = HashMap::new();
        object_fields.insert(
            0xAA,
            vec![FieldInfo {
                name: "val".to_string(),
                value: FieldValue::Int(42),
            }],
        );
        let mut object_static_fields: HashMap<u64, Vec<FieldInfo>> = HashMap::new();
        object_static_fields.insert(
            0xAA,
            vec![FieldInfo {
                name: "INSTANCE".to_string(),
                value: FieldValue::ObjectRef {
                    id: 0xAA,
                    class_name: "MyEnum".to_string(),
                    entry_count: None,
                    inline_value: None,
                },
            }],
        );
        let mut item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "Foo.bar()".to_string(),
            snapshot: PinnedSnapshot::Frame {
                variables: vec![VariableInfo {
                    index: 0,
                    value: VariableValue::ObjectRef {
                        id: 0xAA,
                        class_name: "MyEnum".to_string(),
                        entry_count: None,
                    },
                }],
                object_fields,
                object_static_fields,
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed: HashSet::new(),
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::frame_only(FrameId(1)),
            },
        };

        // Collapse the static field occurrence
        let static_path = NavigationPathBuilder::new(FrameId(0), VarIdx(0))
            .static_field(crate::views::stack_view::StaticFieldIdx(0))
            .build();
        item.local_collapsed.insert(static_path);

        let lines = render_item(&item);
        // var[0] itself should be expanded (-)
        let var0 = &lines[0];
        assert!(
            var0.contains("- "),
            "parent should remain expanded, got: {var0}"
        );
        // val: 42 should still be visible
        assert!(
            lines.iter().any(|l| l.contains("val: 42")),
            "expected val: 42 visible, got: {lines:?}"
        );
    }

    // 5.3: toggle round-trip
    #[test]
    fn toggle_round_trip_collapse_then_expand() {
        let mut item = make_shared_object_frame();
        let path_a = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();

        // Initially expanded
        let lines_before = render_item(&item);
        assert!(lines_before[0].contains("- "), "initially expanded");

        // Collapse
        item.local_collapsed.insert(path_a.clone());
        let lines_collapsed = render_item(&item);
        assert!(lines_collapsed[0].contains("+ "), "after collapse");

        // Re-expand
        item.local_collapsed.remove(&path_a);
        let lines_after = render_item(&item);
        assert!(lines_after[0].contains("- "), "after re-expand");
        assert_eq!(lines_before.len(), lines_after.len(), "row count restored");
    }

    // 5.4: sequential toggles — collapse A then B → both independent
    #[test]
    fn sequential_collapse_both_independent() {
        let mut item = make_shared_object_frame();
        let path_a = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        let path_b = NavigationPathBuilder::new(FrameId(0), VarIdx(1)).build();

        item.local_collapsed.insert(path_a);
        item.local_collapsed.insert(path_b);

        let lines = render_item(&item);
        // Both should show +
        assert!(lines[0].contains("+ "), "var[0] collapsed: {lines:?}");
        assert!(lines[1].contains("+ "), "var[1] collapsed: {lines:?}");
        // No child rows
        assert!(
            !lines.iter().any(|l| l.contains("x: 1")),
            "no children visible when both collapsed"
        );
    }

    // 5.5: collapse state preserved across re-render
    #[test]
    fn collapse_state_preserved_across_rerender() {
        let mut item = make_shared_object_frame();
        let path_a = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path_a);

        let lines1 = render_item(&item);
        let lines2 = render_item(&item);
        assert_eq!(lines1, lines2, "re-render produces same output");
    }

    // 5.6: DIFFERENTIAL — two occurrences of same object, collapse one
    #[test]
    fn differential_same_object_different_visual_output() {
        let mut item = make_shared_object_frame();
        let path_var0 = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path_var0);

        let lines = render_item(&item);
        // var[0] should show + (collapsed, no children)
        assert!(
            lines[0].contains("+ "),
            "var[0] collapsed: got {}",
            lines[0]
        );
        // var[1] should show - (expanded, with x: 1 child)
        assert!(lines[1].contains("- "), "var[1] expanded: got {}", lines[1]);
        // The two var lines must differ
        assert_ne!(
            lines[0], lines[1],
            "collapsed vs expanded must produce different output"
        );
    }

    // 5.7: path_map indexation — cursor row maps to correct path
    #[test]
    fn path_map_correct_indexation() {
        let item = make_shared_object_frame();
        let (_row_count, _kind_map, _sentinel_map, _field_row_map, path_map) =
            collect_row_metadata(&item);

        // path_map[0] = header → None
        assert_eq!(path_map[0], None, "header has no path");
        // path_map[1] = var[0] row → Frame(0)/Var(0)
        let expected_path0 = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        assert_eq!(
            path_map[1].as_ref(),
            Some(&expected_path0),
            "sub_row 1 maps to var[0] path"
        );
        // Find var[1] path
        let expected_path1 = NavigationPathBuilder::new(FrameId(0), VarIdx(1)).build();
        let var1_idx = path_map
            .iter()
            .position(|p| p.as_ref() == Some(&expected_path1));
        assert!(var1_idx.is_some(), "path_map should contain var[1] path");
    }

    // 5.7 continued: with hidden fields
    #[test]
    fn path_map_correct_with_hidden_fields() {
        let mut item = make_shared_object_frame();
        item.hidden_fields.insert(HideKey::Var(0));
        item.show_hidden = true;

        let (_, _, _, _, path_map) = collect_row_metadata(&item);

        // path_map[0] = header → None
        assert_eq!(path_map[0], None, "header has no path");
        // path_map[1] = hidden placeholder → None
        assert_eq!(path_map[1], None, "hidden placeholder has no path");
        // path_map[2] = var[1] → Frame(0)/Var(1)
        let expected_path1 = NavigationPathBuilder::new(FrameId(0), VarIdx(1)).build();
        assert_eq!(
            path_map[2].as_ref(),
            Some(&expected_path1),
            "after hidden placeholder, var[1] path correct"
        );
    }

    // 5.8: subtree snapshot — collapse a field via path
    #[test]
    fn subtree_snapshot_path_collapse() {
        let mut object_fields = HashMap::new();
        object_fields.insert(
            0x50u64,
            vec![
                FieldInfo {
                    name: "a".to_string(),
                    value: FieldValue::Int(1),
                },
                FieldInfo {
                    name: "b".to_string(),
                    value: FieldValue::ObjectRef {
                        id: 0x51,
                        class_name: "Inner".to_string(),
                        entry_count: None,
                        inline_value: None,
                    },
                },
            ],
        );
        object_fields.insert(
            0x51u64,
            vec![FieldInfo {
                name: "c".to_string(),
                value: FieldValue::Int(2),
            }],
        );

        // Collapse field b (index 1) at path FrameId(0x50)/VarIdx(0)/Field(1)
        let collapse_path = NavigationPathBuilder::new(FrameId(0x50), VarIdx(0))
            .field(FieldIdx(1))
            .build();

        let mut local_collapsed = HashSet::new();
        local_collapsed.insert(collapse_path);

        let item = PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 0x50,
                object_fields,
                object_static_fields: HashMap::new(),
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            local_collapsed,
            hidden_fields: HashSet::new(),
            show_hidden: false,
            key: PinKey {
                thread_id: ThreadId(1),
                thread_name: "main".to_string(),
                nav_path: NavigationPathBuilder::new(FrameId(1), VarIdx(0)).build(),
            },
        };

        let (row_count, _, _, _, _) = collect_row_metadata(&item);

        // Without collapse: header + a + b(expanded) + c + separator = 5
        // With b collapsed: header + a + b(collapsed) + separator = 4
        assert_eq!(row_count, 4, "field b collapsed hides its children");
    }

    // 5.9: all existing tests pass (regression) — covered by running
    // `cargo test` which includes all 407+ existing tests.

    // Additional: debug_assert_eq parity for collapsed snapshot
    #[test]
    fn metadata_render_row_count_match_with_collapse() {
        let mut item = make_shared_object_frame();
        let path_a = NavigationPathBuilder::new(FrameId(0), VarIdx(0)).build();
        item.local_collapsed.insert(path_a);

        // collect_row_metadata uses MetadataCollector (path-based)
        // and debug_assert_eq checks render_variable_tree parity.
        // If they disagree, this test panics (debug mode).
        let (row_count, _, _, _, _) = collect_row_metadata(&item);

        // Collapsed var[0] + expanded var[1] with child x: 1
        // header=1, var0(+)=1, var1(-)=1, x:1=1, separator=1 → 5
        assert_eq!(row_count, 5);
    }
}
