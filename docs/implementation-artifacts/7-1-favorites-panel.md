# Story 7.1: Favorites Panel

Status: ready-for-dev

## Story

As a user,
I want to pin any value (stack frame, variable, or object field) and compare them side-by-side
in a favorites panel,
so that I can correlate specific data points across threads without losing my place in the
navigation.

## Acceptance Criteria

1. Given the cursor is on a **frame header** in the StackFrames panel,
   when the user presses `f`,
   then the whole frame is pinned: snapshot captures the frame label, thread name, all variables,
   and their currently expanded object/collection trees.

2. Given the cursor is on a **variable** (`OnVar`) in the StackFrames panel,
   when the user presses `f`,
   then that variable is pinned: snapshot captures the thread name, frame label as context,
   variable label, and if the variable is an expanded `ObjectRef` — its expanded subtree.

3. Given the cursor is on an **object field** (`OnObjectField`) in the StackFrames panel,
   when the user presses `f`,
   then that field is pinned: snapshot captures thread name, frame label, a breadcrumb path
   label, and if the field is an expanded `ObjectRef` — its expanded subtree.

   **Out of scope 7.1** : les cursors `OnCollectionEntry` et `OnCollectionEntryObjField`
   ne déclenchent pas de pin — `f` est silencieusement ignoré sur ces positions
   (future story).

4. Given at least one item is pinned,
   when the TUI renders,
   then the favorites panel appears automatically on the right side of the layout
   (if terminal width ≥ `MIN_WIDTH_FAVORITES_PANEL`).

5. Given no items are pinned,
   when the TUI renders,
   then the favorites panel is hidden — full width used by the main panels.

6. Given the cursor is on a position that is already pinned (même thread, même frame, même
   chemin dans l'arbre),
   when the user presses `f`,
   then the item is unpinned (toggle).

7. Given the favorites panel is visible,
   when the user presses `F` (shift+f),
   then keyboard focus moves to the favorites panel.

8. Given the favorites panel has focus,
   when the user presses `F` or `Esc`,
   then focus returns to the previous main panel.

9. Given the favorites panel has focus,
   when the user presses `f` on a selected item,
   then that item is unpinned.

10. Given a pinned **frame** is rendered in the favorites panel,
    when the panel is drawn,
    then the frame label is shown as header and all its variables and expanded subtrees
    are shown inline (auto-expanded).

11. Given a pinned **variable or field** is rendered in the favorites panel,
    when the panel is drawn,
    then `"ThreadName · frame_label › item_label"` is shown as header, followed by its
    expanded subtree (if it was an ObjectRef) or its value inline (if primitive/null).

12. Given a variable's expanded collection had only N entries loaded at pin time,
    when rendered in the favorites panel,
    then those N entries are shown and unloaded chunks appear as `+ [offset..end]`
    placeholders — the snapshot is frozen, no in-panel chunk loading in this story.

13. Given the terminal width is < `MIN_WIDTH_FAVORITES_PANEL` (120 cols),
    when the TUI renders,
    then the favorites panel is hidden and `[★ N]` appears in the status bar;
    pressing `F` shows `"Terminal trop étroit (< 120 cols)"` in the status bar.

## Tasks / Subtasks

- [ ] Task 1: Add new InputEvent variants (AC: #1–#3, #6, #7, #9)
  - [ ] 1.1 Add `ToggleFavorite` and `FocusFavorites` to `InputEvent` in `input.rs`
  - [ ] 1.2 In `from_key()`, map `'f'` → `ToggleFavorite` and `'F'` (shift) → `FocusFavorites`
        **before** the catch-all `SearchChar` arm (see Dev Notes)
  - [ ] 1.3 Write unit tests for both new key mappings

- [ ] Task 2: Define PinnedItem types (AC: #1–#3, #10–#11)
  - [ ] 2.1 Create `crates/hprof-tui/src/favorites.rs`
  - [ ] 2.2 Define `PinKey` — unique identifier for toggle detection:
        ```rust
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum PinKey {
            Frame   { frame_id: u64, thread_name: String },
            Var     { frame_id: u64, thread_name: String, var_idx: usize },
            Field   { frame_id: u64, thread_name: String, var_idx: usize,
                      field_path: Vec<usize> },
        }
        ```
  - [ ] 2.3 Define `PinnedSnapshot` — what was captured:
        ```rust
        pub enum PinnedSnapshot {
            /// Whole frame: all variables + expanded trees.
            Frame {
                variables: Vec<VariableInfo>,
                object_fields: HashMap<u64, Vec<FieldInfo>>,
                collection_chunks: HashMap<u64, CollectionChunks>,
                truncated: bool,
            },
            /// Single ObjectRef variable/field + its expanded subtree.
            Subtree {
                root_id: u64,
                object_fields: HashMap<u64, Vec<FieldInfo>>,
                collection_chunks: HashMap<u64, CollectionChunks>,
                truncated: bool,
            },
            /// ObjectRef that was NOT expanded at pin time — shown as ref label only.
            UnexpandedRef { class_name: String, object_id: u64 },
            /// Primitive value or null.
            Primitive { value_label: String },
        }
        ```
        Note : `UnexpandedRef` distingue une ref non-expandée d'un vrai primitif —
        utile pour de futures stories qui ajouteraient l'expansion in-panel.
  - [ ] 2.4 Define `PinnedItem` — `frame_id` est supprimé (redondant avec `key`) :
        ```rust
        pub struct PinnedItem {
            pub thread_name: String,
            pub frame_label: String,
            /// Display label: "var[2]", "retryCount", "field[2].subField[1]"
            /// (voir Dev Notes pour la règle de formatage)
            pub item_label: String,
            pub snapshot: PinnedSnapshot,
            pub key: PinKey,
        }
        ```
        Pour obtenir `frame_id` depuis un `PinnedItem`, lire `item.key` (tous les variants
        contiennent `frame_id`).
  - [ ] 2.5 Write unit tests for `PinKey` equality (same frame_id + different thread_name
        → inégaux; même key → égaux)

- [ ] Task 3: Implement snapshot construction (AC: #1–#3, #12)
  - [ ] 3.1 Dans `stack_view.rs`, exposer les accesseurs nécessaires sur `StackState`
        (vérifier si certains existent déjà avant de les créer) :
        - `pub(crate) fn cursor(&self) -> &StackCursor`
        - `pub(crate) fn frames(&self) -> &[FrameInfo]`
        - `pub(crate) fn vars(&self) -> &HashMap<u64, Vec<VariableInfo>>`
        - `pub(crate) fn object_fields(&self) -> &HashMap<u64, Vec<FieldInfo>>`
        - `pub(crate) fn collection_chunks_map(&self) -> &HashMap<u64, CollectionChunks>`
        - `collect_descendants` → `pub(crate)`
  - [ ] 3.2 Implement `fn subtree_snapshot(root_id: u64, state: &StackState,
        reachable: &mut HashSet<u64>) -> (HashMap<u64, Vec<FieldInfo>>, HashMap<u64, CollectionChunks>, bool)`:
        - Walk via `collect_descendants` en passant `reachable` partagé — la limite
          `SNAPSHOT_OBJECT_LIMIT = 500` s'applique **globalement** sur tout le frame,
          pas par appel (appeler avec le même `reachable` pour toutes les vars du frame)
        - Clone reachable `object_fields` + `collection_chunks` (freeze Loading → Collapsed)
        - Retourner `truncated = true` si `reachable.len() >= SNAPSHOT_OBJECT_LIMIT`
  - [ ] 3.3 Implement `fn snapshot_from_cursor(cursor: &StackCursor, state: &StackState,
        thread_name: &str) -> Option<PinnedItem>`:
        - **Résolution `frame_idx → frame_id`** (obligatoire pour tous les variants) :
          `let frame = &state.frames()[frame_idx]; let frame_id = frame.frame_id;`
          Le champ s'appelle `frame_id` sur `FrameInfo`, pas `id`.
          Ne jamais utiliser `frame_idx` comme `frame_id` — ce sont des types différents.
        - `OnFrame(frame_idx)` → `frame_id = state.frames()[frame_idx].frame_id`;
          `PinnedSnapshot::Frame` (all variables + subtree_snapshot per root var);
          `item_label = frame_label.clone()`
        - `OnVar { frame_idx, var_idx }` :
          - Si `VariableValue::ObjectRef { id, .. }` ET `state.object_fields().contains_key(&id)`
            (expandé) → `PinnedSnapshot::Subtree`
          - Si `VariableValue::ObjectRef { id, class_name }` ET non-expandé
            → `PinnedSnapshot::UnexpandedRef { class_name, object_id: id }`
          - Sinon (primitif ou null) → `PinnedSnapshot::Primitive { value_label }`
        - `OnObjectField { frame_idx, var_idx, field_path }` → résoudre le field via field_path
          (voir Dev Notes), construire `PinKey::Field { frame_id, thread_name: thread_name.into(),
          var_idx, field_path }` (les 4 champs obligatoires), puis `Subtree`, `UnexpandedRef`
          ou `Primitive` selon le `FieldValue` du leaf
        - `OnCollectionEntry`, `OnCollectionEntryObjField` → `None` (hors scope 7.1)
        - Autres cursors (`NoFrames`, `OnObjectLoadingNode`, `OnCyclicNode`, `OnChunkSection`)
          → `None` (pas de pin)
  - [ ] 3.4 Write unit tests:
        - `OnFrame` → Frame snapshot complet
        - `OnVar` ObjectRef expandé → `Subtree`
        - `OnVar` ObjectRef non-expandé → `UnexpandedRef`
        - `OnVar` primitif → `Primitive`
        - `OnObjectField` → correct field résolu, `var_idx` dans `PinKey::Field`
        - Limite 500 objects → `truncated = true`
        - `OnCyclicNode` → `None`
        - `OnObjectLoadingNode` → `None`

- [ ] Task 4: Extend App state (AC: #4–#9)
  - [ ] 4.1 Add `Focus::Favorites` variant to `Focus` enum in `app.rs`
  - [ ] 4.2 Add `pinned: Vec<PinnedItem>` to `App` struct
  - [ ] 4.3 Add `favorites_list_state: ListState`
  - [ ] 4.4 Add `prev_focus: Focus` (default: `Focus::ThreadList`)
  - [ ] 4.5 Add `const MIN_WIDTH_FAVORITES_PANEL: u16 = 120;`
  - [ ] 4.6 Add `pinned_cursor: usize` à `App` — index dans `pinned` (pas index de ligne
        visuelle); naviguer Up/Down incrémente/décrémente cet index; `ListState` est
        utilisé uniquement pour le scroll ratatui; synchroniser via
        `list_state.select(if pinned.is_empty() { None } else { Some(pinned_cursor) })`
  - [ ] 4.7 Implement `toggle_pin(&mut self, item: PinnedItem)`:
        - Chercher par `item.key` (PartialEq sur PinKey)
        - Si présent → retirer + clamp `pinned_cursor`:
          `pinned_cursor = pinned_cursor.min(pinned.len().saturating_sub(1))`
        - Si absent → push

- [ ] Task 5: Handle input routing (AC: #1–#3, #6–#9)
  - [ ] 5.1 In `handle_stack_frames_input()`:
        - `ToggleFavorite` → `handle_stack_frames_input` n'est appelé que quand
          `self.stack_state` est `Some` — utiliser
          `if let Some(state) = &self.stack_state { ... }` ou documenter l'invariant;
          obtenir `thread_name` (voir Dev Notes) ;
          appeler `snapshot_from_cursor(state.cursor(), state, &thread_name)`;
          si `Some(item)` → `self.toggle_pin(item)`; si `None` → ignorer silencieusement
        - `FocusFavorites` → si panel visible (`!pinned.is_empty() && width >= MIN_WIDTH…`)
          alors `self.prev_focus = self.focus; self.focus = Focus::Favorites`,
          sinon afficher message status bar
        - **Préserver le comportement existant** : `Escape` dans ce handler doit toujours
          effectuer son action actuelle (retour ThreadList) — ne pas le supprimer ni
          le conditionner lors de l'ajout des nouveaux arms
  - [ ] 5.2 In `handle_thread_list_input()` quand search IS active:
        utiliser le **même pattern que `SearchChar(c)` existant** — construire la nouvelle
        chaîne de filtre en ajoutant `'f'` ou `'F'` et appeler `apply_filter`.
        Ne pas créer de méthode `push_search_char` — ce serait dupliquer la logique
        déjà présente dans le handler `SearchChar`.
  - [ ] 5.3 In `handle_thread_list_input()` quand search is NOT active:
        `FocusFavorites` → même transition focus que 5.1 (pas de pin depuis thread list)
  - [ ] 5.4 Add `Focus::Favorites` branch in `handle_input()`:
        - `Up`/`Down` → navigate : `pinned_cursor = pinned_cursor.saturating_sub(1)` / `(pinned_cursor + 1).min(pinned.len().saturating_sub(1))`
        - `ToggleFavorite` → **guard obligatoire** : `if !self.pinned.is_empty() { ... }`;
          unpin par `self.pinned[self.pinned_cursor].key.clone()` puis clamp `pinned_cursor`
        - `FocusFavorites` | `Escape` → `self.focus = self.prev_focus`

- [ ] Task 6: Extract shared tree rendering (AC: #10–#12)
  - [ ] 6.1 Extract la logique de rendu variable-tree de `StackView` en une fonction
        `pub(crate) fn render_variable_tree(...) -> Vec<ListItem>` :
        - Si `stack_view.rs` reste sous 500 lignes après extraction → la mettre dans `stack_view.rs`
        - Si `stack_view.rs` dépasse 500 lignes → créer `views/tree_render.rs`
  - [ ] 6.2 Checklist d'exhaustivité — tous les variants de `StackCursor` couverts :
        - [ ] `OnFrame`
        - [ ] `OnVar`
        - [ ] `OnObjectField`
        - [ ] `OnObjectLoadingNode`
        - [ ] `OnCyclicNode` ← critique
        - [ ] `OnChunkSection`
        - [ ] `OnCollectionEntry`
        - [ ] `OnCollectionEntryObjField`
        - [ ] `NoFrames`
  - [ ] 6.3 Adapter la signature pour supporter le rendu depuis un `root_id` (subtree)
        en plus du rendu depuis `Vec<VariableInfo>` (frame complet) :
        ```rust
        pub(crate) enum TreeRoot<'a> {
            Frame { vars: &'a [VariableInfo] },
            Subtree { root_id: u64 },
        }
        pub(crate) fn render_variable_tree(
            root: TreeRoot<'_>,
            object_fields: &HashMap<u64, Vec<FieldInfo>>,
            collection_chunks: &HashMap<u64, CollectionChunks>,
            object_phases: &HashMap<u64, ExpansionPhase>,
        ) -> Vec<ListItem<'static>>
        ```
  - [ ] 6.4 `StackView` délègue à cette fonction via `TreeRoot::Frame` (refactor pur)
  - [ ] 6.5 Ajouter un test avec un objet cyclique → `OnCyclicNode` s'affiche, pas de crash
  - [ ] 6.6 Tous les tests `StackView` existants passent après extraction

- [ ] Task 7: Create FavoritesPanel widget (AC: #4, #5, #10–#12)
  - [ ] 7.1 Create `crates/hprof-tui/src/views/favorites_panel.rs`
  - [ ] 7.2 Définir le widget et son state selon le pattern ratatui correct
        (data dans le widget, seul le scroll mutable dans State) :
        ```rust
        /// Data passée au moment du render — non-mutable.
        pub struct FavoritesPanel<'a> {
            pub focused: bool,
            pub pinned: &'a [PinnedItem],
            pub pinned_cursor: usize,
        }

        /// Seul l'état de scroll ratatui est mutable entre renders.
        pub struct FavoritesPanelState {
            pub list_state: ListState,
        }

        impl StatefulWidget for FavoritesPanel<'_> {
            type State = FavoritesPanelState;
            fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) { ... }
        }
        ```
        `App` possède `favorites_panel_state: FavoritesPanelState` et construit
        `FavoritesPanel { focused, pinned: &self.pinned, pinned_cursor: self.pinned_cursor }`
        à chaque appel de `draw()`.
  - [ ] 7.3 Pour chaque `PinnedItem` :
        - Header : `"[F] ThreadName · frame_label"` (Frame)
          ou `"[V] ThreadName · frame_label › item_label"` (Var/Field)
        - Déléguer à `render_variable_tree` via `TreeRoot::Frame { vars }` ou
          `TreeRoot::Subtree { root_id }` selon le snapshot
        - Pour les snapshots (`Frame` ou `Subtree`), synthétiser `object_phases` au moment
          du rendu : `{ id => ExpansionPhase::Expanded }` pour tous les `id` présents dans
          `snapshot.object_fields`. Les `ObjectRef` dont l'id est **absent** de cette map
          sont traités comme `Collapsed` par le renderer (même comportement que dans
          `StackView` pour un objet non encore expandé). Allouer cette `HashMap` une seule
          fois par item pinné, pas par ligne rendue (voir Known limitation F13).
        - Si `truncated = true` → afficher `"[!] snapshot partiel"` sous le header
        - Séparateur vide entre items
  - [ ] 7.4 Border: `BORDER_FOCUSED`/`BORDER_UNFOCUSED`; titre `"Favorites [N]"`;
        légende 1 ligne `"[f] unpin"`
  - [ ] 7.5 Export from `views/mod.rs`
  - [ ] 7.6 Write render tests: item count, types d'affichage Frame vs Variable,
        collapsed chunk placeholder visible

- [ ] Task 8: Conditional layout split (AC: #4, #5, #13)
  - [ ] 8.1 In `App::draw()`, panel visible si `!pinned.is_empty() && area.width >= MIN_WIDTH_FAVORITES_PANEL`
  - [ ] 8.2 Si caché : layout 2-pane existant (30% thread list | 70% stack)
  - [ ] 8.3 Si visible : layout 3-pane :
        ```
        Constraint::Percentage(30) | Constraint::Min(0) | Constraint::Min(40)
        ```
        (thread list 30%, stack prend le reste, favorites au moins 40 cols)
  - [ ] 8.4 Si `!pinned.is_empty() && area.width < MIN_WIDTH_FAVORITES_PANEL` :
        afficher `[★ N]` dans la status bar
  - [ ] 8.5 Si `F` pressé et panel non visible : afficher
        `"Terminal trop étroit (< 120 cols)"` dans la status bar

- [ ] Task 9: Validate and finalize
  - [ ] 9.1 `cargo test --all-targets` — all tests pass
  - [ ] 9.2 `cargo clippy --all-targets -- -D warnings` — zero warnings
  - [ ] 9.3 `cargo fmt -- --check` — clean
  - [ ] 9.4 Manual test avec `assets/heapdump-visualvm.hprof` :
        - Pin un frame header → toutes les variables visibles
        - Pin une variable ObjectRef expandée → subtree visible
        - Pin une variable primitive → valeur inline
        - Pin un field deep dans un objet → breadcrumb + subtree
        - Expand une collection partiellement, pin → loaded entries + collapsed chunks
        - Unpin via `f`; vérifier que le layout se referme quand vide
        - `F`/`Esc` focus switch; `f` en mode search appende au filtre

## Dev Notes

### PinKey : identifiant unique pour le toggle

Deux items sont le même pin si et seulement si leur `PinKey` est égal. `PinKey` encode
la position structurelle dans l'arbre, pas l'index visuel :

```rust
// Même frame_id dans deux threads différents → PinKey différents
PinKey::Frame { frame_id: 42, thread_name: "Thread-1".into() }
PinKey::Frame { frame_id: 42, thread_name: "Thread-2".into() }  // ≠
```

### Source du thread_name dans snapshot_from_cursor

`snapshot_from_cursor` prend `thread_name: &str`. Dans `App`, le nom du thread actif
provient de `self.thread_list` : lors de la sélection d'un thread (quand `stack_state`
devient `Some`), le `ThreadInfo` correspondant est accessible via
`self.thread_list.selected_thread()` (ou équivalent). Implémenter `active_thread_name()`
comme suit :

```rust
fn active_thread_name(&self) -> String {
    self.thread_list
        .selected_thread()          // retourne Option<&ThreadInfo>
        .map(|t| t.name.clone())
        .unwrap_or_default()
}
```

Si `ThreadListState` n'expose pas `selected_thread()`, ajouter ce getter.
`handle_stack_frames_input` est uniquement appelé quand `stack_state` est `Some` —
l'`unwrap_or_default` est un fallback défensif, pas un cas attendu.

### item_label : règle de formatage

| Cursor | item_label |
|--------|-----------|
| `OnFrame` | `frame_label` (identique au header, `item_label` inutilisé) |
| `OnVar { var_idx }` | `"var[{var_idx}]"` |
| `OnObjectField { var_idx, field_path }` | `"var[{var_idx}].{nom_field[0]}.{nom_field[1]}…"` — résoudre les noms de champ depuis `object_fields` si disponibles, sinon `"field[i]"` |

Exemple : `OnObjectField { var_idx: 0, field_path: [2, 1] }` → `"var[0].cache.size"`
si les noms sont résolus, ou `"var[0].field[2].field[1]"` si non.

### snapshot_from_cursor : résolution du field_path

Pour `OnObjectField { frame_idx, var_idx, field_path }`, le field ciblé se trouve en
suivant `field_path` dans `object_fields`. Utiliser les accesseurs `pub(crate)` de Task 3.1 :

```rust
// Résoudre le FieldInfo au bout du field_path :
let frame = &state.frames()[frame_idx];
let frame_id = frame.frame_id;             // champ = frame_id, PAS .id
let var = state.vars()
    .get(&frame_id)?                       // keyed by frame_id
    .get(var_idx)?;
let root_id = match var.value {
    VariableValue::ObjectRef { id, .. } => id,
    _ => return None,
};
let mut current_id = root_id;
for &field_idx in &field_path[..field_path.len() - 1] {
    let fields = state.object_fields().get(&current_id)?;
    match fields[field_idx].value {
        FieldValue::ObjectRef { id, .. } => current_id = id,
        _ => return None,
    }
}
let leaf_field = &state.object_fields().get(&current_id)?[*field_path.last()?];
// leaf_field est le FieldInfo pinnée
```

### Limite snapshot : 500 object_ids max

```rust
const SNAPSHOT_OBJECT_LIMIT: usize = 500;
```

Si `reachable.len() >= SNAPSHOT_OBJECT_LIMIT`, stopper la traversée, `truncated = true`.
Le panel affiche `"[!] snapshot partiel — trop d'objets"` sous le header du pin concerné.

### Known limitation : PinKey positionnel, pas sémantique

`PinKey::Var { var_idx }` et `PinKey::Field { var_idx, field_path }` utilisent des indices
positionnels. Si la liste de variables d'un frame change entre deux renders (cas rare mais
possible sur recharge async), `var_idx = 2` peut désigner une variable différente. Le
toggle-detection silently unpinne alors le mauvais item. Cette limitation est connue et
acceptée pour 7.1 — les variables d'un frame sont stables dans la session courante.

### Known limitation : snapshot statique

Les chunks non chargés au moment du pin restent `Collapsed` — non chargeables depuis
le favorites panel (in-panel loading = future story). Si l'utilisateur veut voir une
collection complète dans favorites, il doit la charger dans StackFrames puis re-pinner.

### Known limitation : allocation `object_phases` par render

La synthèse `HashMap<u64, ExpansionPhase>` pour le rendu du snapshot est allouée une fois
par item pinné par frame de render. Avec la limite de 500 objets et quelques pins, c'est
acceptable. Si les benchmarks montrent un impact (Epic 8 sensibilité perf), la prochaine
optimisation est de cacher cette map dans `PinnedItem` à la construction du snapshot.

### Unpin par PinKey, pas par index

```rust
// CORRECT — utiliser pinned_cursor (index dans pinned, pas ligne visuelle) :
if !self.pinned.is_empty() {                              // guard obligatoire
    let key = self.pinned[self.pinned_cursor].key.clone();
    self.pinned.retain(|i| i.key != key);
    self.pinned_cursor = self.pinned_cursor
        .min(self.pinned.len().saturating_sub(1));
    let sel = if self.pinned.is_empty() { None } else { Some(self.pinned_cursor) };
    self.favorites_list_state.select(sel);
}

// FAUX — ne jamais utiliser selected_index (ancien nom) ni l'index ratatui brut :
// self.pinned.remove(selected_index);  ← index peut dériver
```

### Input conflict: 'f' vs SearchChar

```rust
// Dans from_key() — AVANT le catch-all :
(KeyCode::Char('f'), KeyModifiers::NONE) => Some(InputEvent::ToggleFavorite),
(KeyCode::Char('F'), KeyModifiers::SHIFT) => Some(InputEvent::FocusFavorites),
```

En mode search dans ThreadList :
```rust
InputEvent::ToggleFavorite => { self.thread_list.push_search_char('f'); Action::Continue }
InputEvent::FocusFavorites => { self.thread_list.push_search_char('F'); Action::Continue }
```

### Focus state machine

```
ThreadList ←──Enter──→ StackFrames
     │                      │
     └──── F key ──────────┘
                  │
                  ▼
              Favorites
                  │
              Esc / F key
                  │
                  ▼
             prev_focus (ThreadList ou StackFrames)
```

`F` depuis `ThreadList` ET depuis `StackFrames` → `Focus::Favorites` (Task 5.1 et 5.3).
`Esc` ou `F` depuis `Favorites` → restaure `prev_focus`.

### Dependency on Story 7.2 (Theme System — parallel)

- **7.2 merged first** : utiliser `app.theme.<field>` dans `FavoritesPanel` et `render_variable_tree`
- **7.1 starts first** : utiliser `crate::theme::THEME` constants — aucun `Color::*` inline.
  Story 7.2 migrera `favorites_panel.rs` dans son propre scope.

### Project Structure Notes

New files:
- `crates/hprof-tui/src/favorites.rs` — `PinKey`, `PinnedSnapshot`, `PinnedItem`,
  `subtree_snapshot`, `snapshot_from_cursor`
- `crates/hprof-tui/src/views/favorites_panel.rs` — `FavoritesPanel` widget
- `crates/hprof-tui/src/views/tree_render.rs` — si `stack_view.rs` > 500 lignes

Modified files:
- `crates/hprof-tui/src/views/stack_view.rs` — extraire `render_variable_tree` + `TreeRoot`;
  `collect_descendants` → `pub(crate)`
- `crates/hprof-tui/src/lib.rs` — add `mod favorites;`
- `crates/hprof-tui/src/views/mod.rs` — add `pub mod favorites_panel;`
- `crates/hprof-tui/src/app.rs` — `Focus`, `App` struct, `handle_input`, `draw`,
  `MIN_WIDTH_FAVORITES_PANEL`
- `crates/hprof-tui/src/input.rs` — `ToggleFavorite`/`FocusFavorites` + `from_key`

### References

- Epic 7 story definition: [Source: docs/planning-artifacts/epics.md#Story-7.1-Favorites-Panel]
- `StackState`, `StackCursor`, `CollectionChunks`, `collect_descendants`:
  [Source: crates/hprof-tui/src/views/stack_view.rs]
- `VariableInfo`, `VariableValue`, `FieldInfo`, `FieldValue`:
  [Source: crates/hprof-engine/src/engine.rs]
- `SearchableList` StatefulWidget pattern: [Source: crates/hprof-tui/src/views/thread_list.rs]
- `Focus` enum + `App` struct: [Source: crates/hprof-tui/src/app.rs:54–92]
- `InputEvent` + `from_key`: [Source: crates/hprof-tui/src/input.rs:11–61]
- Story 7.2 (parallel — Theme struct): [Source: docs/implementation-artifacts/7-2-theme-system.md]

## Dev Agent Record

### Agent Model Used

<!-- filled by dev agent -->

### Debug Log References

### Completion Notes List

### File List
