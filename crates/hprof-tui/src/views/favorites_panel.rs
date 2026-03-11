//! Favorites panel widget: renders pinned snapshots side-by-side.
//!
//! [`FavoritesPanel`] is a [`StatefulWidget`] that renders each [`PinnedItem`]
//! with a header and its frozen variable tree via [`render_variable_tree`].

use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, StatefulWidget, Widget},
};

use crate::{
    favorites::{PinnedItem, PinnedSnapshot},
    theme::THEME,
    views::{
        cursor::CursorState,
        stack_view::ExpansionPhase,
        tree_render::{render_variable_tree, TreeRoot},
    },
};

/// Render-time data for the favorites panel (non-mutable).
pub struct FavoritesPanel<'a> {
    /// Whether this panel has keyboard focus.
    pub focused: bool,
    /// Pinned items to display (borrowed from `App`).
    pub pinned: &'a [PinnedItem],
}

/// Mutable scroll state for the favorites panel.
pub struct FavoritesPanelState {
    nav: CursorState<usize>,
    items_len: usize,
}

impl Default for FavoritesPanelState {
    fn default() -> Self {
        Self {
            nav: CursorState::new(0),
            items_len: 0,
        }
    }
}

impl FavoritesPanelState {
    /// Returns selected item index in `pinned`.
    pub fn selected_index(&self) -> usize {
        *self.nav.cursor()
    }

    /// Updates known item count for selection sync.
    pub fn set_items_len(&mut self, len: usize) {
        self.items_len = len;
        let items: Vec<usize> = (0..len).collect();
        self.nav.sync(&items);
    }

    /// Sets selected item index, or fully deselects when `None`.
    pub fn set_selected_index(&mut self, idx: Option<usize>) {
        if let Some(i) = idx {
            let items: Vec<usize> = (0..self.items_len).collect();
            self.nav.set_cursor_and_sync(i, &items);
        } else {
            self.nav.list_state_mut().select(None);
        }
    }

    pub fn list_state_mut(&mut self) -> &mut ListState {
        self.nav.list_state_mut()
    }
}

impl StatefulWidget for FavoritesPanel<'_> {
    type State = FavoritesPanelState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.set_items_len(self.pinned.len());
        let border_style = if self.focused {
            THEME.border_focused
        } else {
            THEME.border_unfocused
        };
        let title = format!("Favorites [{}]", self.pinned.len());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border_style)
            .title(title)
            .title_bottom(Line::from(Span::raw("[f] unpin")));
        let inner = block.inner(area);
        block.render(area, buf);

        let mut items: Vec<ListItem<'static>> = Vec::new();

        for item in self.pinned {
            // Header line.
            let header_text = match &item.snapshot {
                PinnedSnapshot::Frame { .. } => {
                    format!("[F] {} · {}", item.thread_name, item.frame_label)
                }
                _ => format!(
                    "[V] {} · {} › {}",
                    item.thread_name, item.frame_label, item.item_label
                ),
            };
            items.push(ListItem::new(Line::from(Span::styled(
                header_text,
                THEME.status_bar_bg,
            ))));

            // Snapshot content.
            match &item.snapshot {
                PinnedSnapshot::Frame {
                    variables,
                    object_fields,
                    collection_chunks,
                    truncated,
                } => {
                    if *truncated {
                        items.push(ListItem::new(Line::from(Span::styled(
                            "  [!] snapshot partiel — trop d'objets",
                            THEME.error_indicator,
                        ))));
                    }
                    let object_phases: HashMap<u64, ExpansionPhase> = object_fields
                        .keys()
                        .map(|&id| (id, ExpansionPhase::Expanded))
                        .collect();
                    let tree = render_variable_tree(
                        TreeRoot::Frame { vars: variables },
                        object_fields,
                        collection_chunks,
                        &object_phases,
                        &HashMap::new(),
                    );
                    items.extend(tree);
                }
                PinnedSnapshot::Subtree {
                    root_id,
                    object_fields,
                    collection_chunks,
                    truncated,
                } => {
                    if *truncated {
                        items.push(ListItem::new(Line::from(Span::styled(
                            "  [!] snapshot partiel — trop d'objets",
                            THEME.error_indicator,
                        ))));
                    }
                    let object_phases: HashMap<u64, ExpansionPhase> = object_fields
                        .keys()
                        .map(|&id| (id, ExpansionPhase::Expanded))
                        .collect();
                    let tree = render_variable_tree(
                        TreeRoot::Subtree { root_id: *root_id },
                        object_fields,
                        collection_chunks,
                        &object_phases,
                        &HashMap::new(),
                    );
                    items.extend(tree);
                }
                PinnedSnapshot::UnexpandedRef {
                    class_name,
                    object_id,
                } => {
                    let short = class_name.rsplit('.').next().unwrap_or(class_name);
                    items.push(ListItem::new(Line::from(Span::raw(format!(
                        "  {short} @ 0x{object_id:X} [not expanded]"
                    )))));
                }
                PinnedSnapshot::Primitive { value_label } => {
                    items.push(ListItem::new(Line::from(Span::raw(format!(
                        "  {value_label}"
                    )))));
                }
            }

            // Blank separator between items.
            items.push(ListItem::new(Line::from("")));
        }

        if items.is_empty() {
            items.push(ListItem::new(Line::from(Span::styled(
                "(no favorites)",
                THEME.null_value,
            ))));
        }

        let list = List::new(items).highlight_style(THEME.selection_bg);
        StatefulWidget::render(list, inner, buf, state.list_state_mut());
    }
}

#[cfg(test)]
mod tests {
    use hprof_engine::{FieldInfo, FieldValue, VariableInfo, VariableValue};
    use ratatui::{backend::TestBackend, Terminal};
    use std::collections::HashMap;

    use super::*;
    use crate::favorites::{PinKey, PinnedItem, PinnedSnapshot};

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
                collection_chunks: HashMap::new(),
                truncated: false,
            },
            key: PinKey::Frame {
                frame_id: 1,
                thread_name: "main".to_string(),
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
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        }
    }

    #[test]
    fn panel_shows_frame_header_with_f_prefix() {
        let items = vec![make_frame_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
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
                pinned: &items,
            },
            80,
            10,
        );
        assert!(text.contains("42"), "expected value 42, got: {text:?}");
    }

    #[test]
    fn panel_shows_frame_variables() {
        let items = vec![make_frame_item()];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                pinned: &items,
            },
            80,
            10,
        );
        // Frame snapshot with a null var should show [0] null
        assert!(text.contains("[0]"), "expected var index, got: {text:?}");
        assert!(text.contains("null"), "expected null, got: {text:?}");
    }

    #[test]
    fn panel_shows_collapsed_chunk_placeholder_for_unloaded_chunks() {
        use crate::views::stack_view::{ChunkState, CollectionChunks};
        let mut collection_chunks = HashMap::new();
        collection_chunks.insert(
            5u64,
            CollectionChunks {
                total_count: 200,
                eager_page: None,
                chunk_pages: {
                    let mut m = HashMap::new();
                    m.insert(100usize, ChunkState::Collapsed);
                    m
                },
            },
        );
        let mut object_fields = HashMap::new();
        object_fields.insert(
            10u64,
            vec![FieldInfo {
                name: "items".to_string(),
                value: FieldValue::ObjectRef {
                    id: 5,
                    class_name: "ArrayList".to_string(),
                    entry_count: Some(200),
                    inline_value: None,
                },
            }],
        );
        let items = vec![PinnedItem {
            thread_name: "main".to_string(),
            frame_label: "Foo.bar()".to_string(),
            item_label: "var[0]".to_string(),
            snapshot: PinnedSnapshot::Subtree {
                root_id: 10,
                object_fields,
                collection_chunks,
                truncated: false,
            },
            key: PinKey::Var {
                frame_id: 1,
                thread_name: "main".to_string(),
                var_idx: 0,
            },
        }];
        let text = render_panel(
            FavoritesPanel {
                focused: false,
                pinned: &items,
            },
            80,
            15,
        );
        assert!(
            text.contains("+"),
            "expected + placeholder for collapsed chunk, got: {text:?}"
        );
    }

    #[test]
    fn favorites_state_cursor_moves_down() {
        let mut state = FavoritesPanelState::default();
        state.set_items_len(3);
        state.nav.move_down(&[0usize, 1, 2]);
        assert_eq!(state.selected_index(), 1);
    }
}
