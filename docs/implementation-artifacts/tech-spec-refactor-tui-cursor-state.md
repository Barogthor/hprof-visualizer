---
title: 'Refactor TUI Navigation — CursorState<Id> Composition'
slug: 'refactor-tui-cursor-state'
created: '2026-03-11'
status: 'Implementation Complete'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['rust', 'ratatui', 'crossterm']
files_to_modify:
  - 'crates/hprof-tui/src/views/cursor.rs'
  - 'crates/hprof-tui/src/views/mod.rs'
  - 'crates/hprof-tui/src/views/thread_list.rs'
  - 'crates/hprof-tui/src/views/stack_view/state.rs'
  - 'crates/hprof-tui/src/views/stack_view/widget.rs'
  - 'crates/hprof-tui/src/views/favorites_panel.rs'
  - 'crates/hprof-tui/src/app/mod.rs'
code_patterns:
  - 'StatefulWidget avec State séparé (SearchableList/ThreadListState, StackView/StackState)'
  - 'flat_items() -> Vec<Id> calculé à la demande pour la navigation'
  - 'set_visible_height() appelé pendant render, page_up/down pendant input handler'
  - 'let-chains (edition 2024)'
test_patterns:
  - 'Tests inline dans chaque fichier (#[cfg(test)] mod tests)'
  - 'Accès direct aux champs dans les tests existants (ex: state.cursor)'
  - 'Helpers make_frame(), make_var(), make_threads() locaux à chaque module'
---

# Tech-Spec: Refactor TUI Navigation — CursorState<Id> Composition

**Created:** 2026-03-11

## Overview

### Problem Statement

`ThreadListState` et `StackState` dupliquent la même logique de navigation
(move_up/down/home/end/page_up/page_down + sync `ListState`). `FavoritesPanelState`
n'a pas de navigation state du tout (le cursor vit sur le widget struct non-mutable).

### Solution

Extraire une struct générique `CursorState<Id>` dans `views/cursor.rs` qui encapsule
`cursor: Id`, `list_state: ListState`, et `visible_height: usize`. Elle expose les
méthodes de navigation en recevant `items: &[Id]` à chaque appel. Les trois states
composent `CursorState` et délèguent via des wrappers one-liner.

### Scope

**In Scope:**
- Struct `CursorState<Id: PartialEq + Clone>` avec toutes les méthodes de navigation
- Composition dans `ThreadListState`, `StackState`, `FavoritesPanelState`
- Suppression du code dupliqué dans les trois states
- Harmonisation de l'API page_up/page_down : visible_height stockée dans le state
  (actuellement ThreadListState prend `n: usize`, StackState lit `self.visible_height`)
- Tests unitaires de `CursorState` en isolation

**Out of Scope:**
- Focused border rendering
- Logique métier de chaque state (filtre threads, expansion objets, épinglage)
- Implémentation réelle de la navigation favorites (terrain préparé, pas activé)

## Context for Development

### Codebase Patterns

- `ThreadListState` (thread_list.rs:22) : navigation par serial stable à travers les
  filtres. `filtered_serials: Vec<u32>` est la liste plate courante. `move_up/down`
  recalculent le serial depuis l'index. `page_up(n)/page_down(n)` prennent la taille
  de page en paramètre — incohérent avec StackState.
- `StackState` (stack_view/state.rs:18) : navigation via `StackCursor` (enum `Debug +
  Clone + PartialEq + Eq`, 8 variants avec `Vec<usize>` field_path). `flat_items()`
  génère la liste plate à la demande. `move_page_up/down` lisent `self.visible_height:
  u16`. `move_home/end` absents.
- `FavoritesPanelState` (favorites_panel.rs:36) : seulement `list_state: ListState`,
  pas de cursor, pas de navigation. `pinned_cursor: usize` vit dans `App` (struct champ
  ligne 102), passé au widget via `FavoritesPanel { pinned_cursor: self.pinned_cursor }`.
- `App` (app/mod.rs) : possède `thread_list_height: u16` (ligne 96), set au render
  (ligne 895), lu dans les input handlers pour `page_down(h)`. Ce champ disparaît après
  migration.
- Edition 2024, `let-chains` disponibles.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-tui/src/views/thread_list.rs` | Source : move_up/down/home/end/page_up(n)/page_down(n) à migrer |
| `crates/hprof-tui/src/views/stack_view/state.rs` | Source : move_up/down/page_up/down + sync_list_state à migrer |
| `crates/hprof-tui/src/views/stack_view/types.rs` | `StackCursor` : `PartialEq + Clone + Eq` confirmés ✓ |
| `crates/hprof-tui/src/views/stack_view/widget.rs` | Ligne 38 : accès direct `&mut state.list_state` → `state.list_state_mut()` |
| `crates/hprof-tui/src/views/favorites_panel.rs` | Destination : ajouter `CursorState<usize>`, supprimer `pinned_cursor` du widget |
| `crates/hprof-tui/src/views/mod.rs` | Ajouter `pub mod cursor` |
| `crates/hprof-tui/src/app/mod.rs` | Supprimer `thread_list_height`, adapter page_up/down, migrer `pinned_cursor` |
| `crates/hprof-tui/src/views/stack_view/tests.rs` | Tests existants : accès `state.cursor` direct → `state.cursor()` (syntaxe seule) |

### Technical Decisions

- `CursorState<Id>` générique sur `Id: PartialEq + Clone`. Pas de trait object (`dyn`)
  nécessaire — les trois usages sont statiques.
- `visible_height: usize` stocké dans `CursorState` (pas `u16` pour éviter les casts).
  Mis à jour via `set_visible_height(h: usize)`. `App::thread_list_height: u16` (ligne 96)
  est supprimé — la hauteur est désormais stockée dans `thread_list.nav`.
- Les méthodes de navigation reçoivent `items: &[Id]` — pas de stockage interne des
  items dans `CursorState` (chaque state calcule sa propre liste plate).
- `move_home` et `move_end` ajoutés à `StackState` via `CursorState` (absents aujourd'hui).
- API normalisée : `page_up()` / `page_down()` sans paramètre, utilisent `visible_height`.
  `ThreadListState` perd son `page_up(n: usize)` — les appelants dans `app/mod.rs`
  (lignes 306, 311, 352, 357) appellent `page_down()`/`page_up()` sans arg. `set_visible_height`
  est appelé dans le render (ligne 895) pour les deux states.
- `FavoritesPanelState` reçoit `nav: CursorState<usize>` (cursor = index dans `pinned`).
  `pinned_cursor: usize` migre de `App` vers `FavoritesPanelState`. Le champ disparaît
  du widget struct `FavoritesPanel` et du struct `App`.
- `widget.rs:38` — `&mut state.list_state` → `state.list_state_mut()`.
- Tests existants dans `stack_view/tests.rs` : `state.cursor` → `state.cursor()`.
  Changement de syntaxe uniquement, aucune logique de test modifiée.

## Implementation Plan

### Tasks

- [x] Task 1: Créer `cursor.rs` et exposer le module
  - File: `crates/hprof-tui/src/views/cursor.rs` (nouveau fichier)
  - Action: Définir `pub struct CursorState<Id>` avec champs privés `cursor: Id`,
    `list_state: ListState`, `visible_height: usize`. Implémenter :
    - `new(initial: Id) -> Self` — `visible_height` défaut = 1
    - `move_up`, `move_down`, `move_home`, `move_end` reçoivent `items: &[Id]` —
      guard `if items.is_empty() { return; }` en tête, `saturating_sub(1)` sur len
    - `move_page_up`, `move_page_down` — utilisent `self.visible_height`
    - `cursor() -> &Id`, `list_state_mut() -> &mut ListState`,
      `set_visible_height(h: usize)`, `sync(items: &[Id])` (cherche cursor dans items,
      met à jour list_state avec l'index trouvé ou `None` si absent)
    - `set_cursor_and_sync(c: Id, items: &[Id])` — assigne `self.cursor = c` sans
      réinitialiser `visible_height`, puis appelle `sync(items)` ; utilisé par
      `StackState::set_cursor` pour éviter de perdre la hauteur visible
    - Module `#[cfg(test)]` avec tests couvrant les ACs 1 à 7b
  - File: `crates/hprof-tui/src/views/mod.rs`
  - Action: Ajouter `pub mod cursor;`

- [x] Task 2: Migrer `ThreadListState` + adapter `app/mod.rs` (page navigation)
  - File: `crates/hprof-tui/src/views/thread_list.rs`
  - Action:
    - Remplacer champs `selected_serial: Option<u32>`, `list_state: ListState`
      par `nav: CursorState<u32>`
    - `apply_filter` — séquence exacte :
      1. Recalculer `filtered_serials`
      2. Si `filtered_serials` contient `*nav.cursor()` : `nav.sync(&filtered_serials)`
      3. Sinon si non vide : `self.nav = CursorState::new(*filtered_serials.first().unwrap())`
      4. Sinon (vide) : ne pas réinitialiser `nav` (cursor orphelin, `selected_serial()` → `None`)
    - Toutes méthodes `move_*` : wrappers one-liner sur `self.nav`
    - `selected_serial()` : `if filtered_serials.is_empty() { None } else { Some(*self.nav.cursor()) }`
    - Remplacer `page_up(n: usize)` / `page_down(n: usize)` par
      `page_up()` / `page_down()` + `set_visible_height(h: usize)`
    - Adapter `SearchableList::render` : `state.list_state` (ligne 267) →
      `state.nav.list_state_mut()` ; `state.filtered_serials` reste accessible
      car `SearchableList` et `ThreadListState` sont dans le même module
      (`thread_list.rs`) — pas de changement de visibilité requis
    - Adapter les tests `thread_list.rs` qui appellent `page_down(n)` / `page_up(n)` :
      ajouter `state.set_visible_height(n)` avant l'appel, puis appeler sans argument
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action:
    - Supprimer champ `thread_list_height: u16` (ligne 96)
    - Dans render (ligne 895) : appeler
      `self.thread_list.set_visible_height(list_area.height.saturating_sub(2) as usize)`
    - Remplacer les 4 appels `page_down(h)` / `page_up(h)` (lignes 306, 311, 352, 357)
      par `page_down()` / `page_up()`

- [x] Task 3: Migrer `StackState` + adapter `widget.rs` et `tests.rs`
  - File: `crates/hprof-tui/src/views/stack_view/state.rs`
  - Action:
    - Remplacer champs `cursor: StackCursor`, `list_state: ListState`,
      `visible_height: u16` par `nav: CursorState<StackCursor>`
    - `move_up/down/page_up/page_down` : wrappers one-liner via `flat_items()`
    - Ajouter `move_home` / `move_end` : wrappers via `flat_items()` (nouveau)
    - `set_cursor(c)` : appeler `self.nav.set_cursor_and_sync(c, &self.flat_items())`
      — ajouter `set_cursor_and_sync(c: Id, items: &[Id])` à `CursorState` qui assigne
      `self.cursor = c` **sans réinitialiser `visible_height`**, puis appelle `sync(items)`
      (si `c` absent des items, `list_state` devient `None` — cursor orphelin voulu)
    - `cursor()` → `self.nav.cursor()`
    - `set_visible_height(h: usize)` → `self.nav.set_visible_height(h)`
    - Dans `app/mod.rs` lignes 897 et 900 : ajouter le cast `as usize` explicitement :
      `ss.set_visible_height(stack_area.height.saturating_sub(2) as usize)`
    - Dans `resync_cursor_after_collapse` et `set_expansion_failed` : les assignations
      directes `self.cursor = fallback` (lignes 465-493, 402-411) deviennent
      `self.nav.set_cursor_and_sync(fallback, &self.flat_items())` — même pattern
      que `set_cursor`, sans réinitialiser `visible_height`
    - Supprimer `sync_list_state` — remplacer ses **4 callsites** :
      - `set_expansion_failed` → `self.nav.sync(&self.flat_items())` (après le
        `set_cursor_and_sync` qui gère le cursor de fallback)
      - `resync_cursor_after_collapse` → `self.nav.sync(&self.flat_items())`
      - `toggle_expand` → `self.nav.sync(&self.flat_items())`
      - `set_cursor` → géré par `set_cursor_and_sync` (voir ci-dessus)
    - `StackState::new` : quand `frames.is_empty()`, utiliser
      `CursorState::new(StackCursor::NoFrames)` — cursor orphelin permanent,
      `list_state` reste `None`, navigation no-op (guard `is_empty()` dans move_*)
  - File: `crates/hprof-tui/src/views/stack_view/widget.rs`
  - Action: ligne 38 — `&mut state.list_state` → `state.list_state_mut()`
  - File: `crates/hprof-tui/src/views/stack_view/tests.rs`
  - Action:
    - Remplacer toutes les lectures `state.cursor` par `state.cursor()` (~40 sites
      dans les `assert_eq!` et autres expressions)
    - Remplacer les 4 assignations directes `state.cursor = SomeVariant` par
      `state.set_cursor(SomeVariant)` — `set_cursor` est une méthode publique existante

- [x] Task 4: Migrer `FavoritesPanelState` + adapter `app/mod.rs` (pinned_cursor)
  - File: `crates/hprof-tui/src/views/favorites_panel.rs`
  - Action:
    - Ajouter champ `nav: CursorState<usize>` dans `FavoritesPanelState`
      (init : `CursorState::new(0)`)
    - Exposer `pub fn selected_index(&self) -> usize { *self.nav.cursor() }`
    - Exposer `pub fn set_selected_index(&mut self, idx: Option<usize>)` :
      si `Some(i)` → `self.nav.set_cursor_and_sync(i, &(0..len).collect::<Vec<_>>())` ;
      si `None` → forcer `list_state.select(None)` sans changer cursor
      (nécessaire pour `sync_favorites_list_state` qui passe `None` quand `pinned` vide)
    - Supprimer `pinned_cursor: usize` du widget struct `FavoritesPanel` ;
      `FavoritesPanel::render` lira le cursor via `state.selected_index()` passé
      depuis `App` ou stocké dans `FavoritesPanelState` — adapter le render en
      conséquence (remplacer `self.pinned_cursor` par `state.selected_index()`)
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action:
    - Supprimer champ `pinned_cursor: usize` du struct `App` (ligne 102)
    - Remplacer tous les accès `self.pinned_cursor` par
      `self.favorites_list_state.selected_index()` (nom réel du champ : `favorites_list_state`)
    - Remplacer toutes les mutations `self.pinned_cursor = ...` par
      `self.favorites_list_state.set_selected_index(...)`
    - Adapter `sync_favorites_list_state` : remplacer l'écriture directe
      `self.favorites_list_state.list_state.select(sel)` par
      `self.favorites_list_state.set_selected_index(sel)` — passe `Option<usize>`
      directement pour préserver la déselection totale quand `pinned` est vide
    - Supprimer la méthode `sync_favorites_list_state` après avoir remplacé
      tous ses callsites par des appels directs à `set_selected_index`
    - Supprimer `pinned_cursor` de la construction `FavoritesPanel { ... }` (ligne 949)
    - Remplacer les mutations `self.pinned_cursor = ...` par
      `self.favorites_list_state.set_selected_index(Some(...))`
  - Notes: Ne pas câbler `move_up/down` dans les input handlers (hors scope)

- [x] Task 5: Vérification finale
  - Action: `cargo test --all` — aucune régression
  - Action: `cargo clippy --all-targets -- -D warnings` — aucun warning

### Acceptance Criteria

- [x] AC-1: Given `CursorState::new(0u32)` et items `[0, 1, 2]`, when `move_down(&[0, 1, 2])`, then `cursor() == 1` et list_state sélectionne l'index 1

- [x] AC-2: Given cursor sur le dernier item, when `move_down`, then cursor reste sur le dernier item (pas de panic, pas de wrap)

- [x] AC-3: Given cursor sur le premier item, when `move_up`, then cursor reste sur le premier item (clamp)

- [x] AC-4: Given cursor quelconque et items non vide, when `move_home`, then cursor == premier item ; when `move_end`, then cursor == dernier item

- [x] AC-5: Given `set_visible_height(3)` et items `[0..9]` et cursor sur 0, when `move_page_down(&items)`, then cursor == 3

- [x] AC-6: Given `set_visible_height(10)` et items `[0, 1, 2]` et cursor sur 0, when `move_page_down(&items)`, then cursor == 2 (clamp au dernier)

- [x] AC-6b: Given `CursorState::new(0u32)` sans `set_visible_height` (défaut = 1) et items `[0, 1, 2]`, when `move_page_down(&items)`, then cursor == 1 (avance d'au moins 1)

- [x] AC-7: Given `CursorState::new(0u32)` et items `[]`, when toute méthode move_* est appelée, then pas de panic et cursor inchangé

- [x] AC-7b: Given `CursorState::new(0u32)` et `set_visible_height(5)` et items `[0]`, when `move_page_down(&[0])`, then cursor == 0 et pas de panic (pas d'underflow usize)

- [x] AC-8: Given 3 threads et filtre "worker" (2 threads visibles), when `move_down()`, then `selected_serial()` retourne le serial du 2e thread worker (tests existants passent)

- [x] AC-8b: Given 3 threads (serials 1, 2, 3) et serial 2 sélectionné, when `apply_filter("worker")` avec serials 2 et 3 visibles, then `selected_serial() == Some(2)` (serial conservé)

- [x] AC-8c: Given 3 threads et filtre "xyz" (0 résultats), when `apply_filter("xyz")`, then `selected_serial() == None` (pas de panic, pas de serial orphelin exposé)

- [x] AC-9: Given StackState avec frames et vars expandues, when `move_down()`, `move_page_down()`, `set_cursor()`, then tests existants dans `stack_view/tests.rs` passent après :
  - lectures `state.cursor` → `state.cursor()`
  - assignations `state.cursor = X` → `state.set_cursor(X)` (4 sites)

- [x] AC-10: Given `FavoritesPanelState::default()` avec cursor initial 0, when `state.nav.move_down(&[0, 1, 2])`, then `state.selected_index() == 1`

- [x] AC-11: Given codebase après migration complète, when `cargo test --all`, then aucune régression ; when `cargo clippy --all-targets -- -D warnings`, then aucun warning

- [x] AC-12: Given codebase après migration complète, when `cargo build`, then compilation sans erreur confirmant que `App` ne possède plus `pinned_cursor: usize` ni `thread_list_height: u16`

## Additional Context

### Dependencies

Aucune nouvelle dépendance externe. `ratatui::widgets::ListState` est déjà disponible.

### Testing Strategy

- **TDD sur `CursorState`** : écrire les tests (AC-1 à AC-7b) dans `cursor.rs` avant
  l'implémentation de la struct — approche Red/Green/Refactor
- **Tests en isolation** : `CursorState<u32>` testable sans ratatui rendering
  (`ListState` est un simple wrapper d'index, pas de terminal nécessaire)
- **Filet de régression** : les tests existants de `ThreadListState` (thread_list.rs)
  et `StackState` (stack_view/tests.rs) valident le comportement post-migration ;
  seule adaptation : `state.cursor` → `state.cursor()` dans les assertions
- **Validation manuelle** : après migration, lancer l'app sur
  `assets/heapdump-visualvm.hprof` et vérifier PageUp/PageDown sur les deux panels

### Notes

- `StackCursor` doit implémenter `PartialEq + Clone` — vérifier que c'est déjà le cas
  (très probable vu les usages existants)
- `CursorState` avec items vide : `cursor` peut être dans un état "orphelin" (ne figure
  pas dans `items`). Les méthodes move_* vérifient `items.is_empty()` en guard.
- Ne pas rendre `list_state` public dans `CursorState` — exposer `list_state_mut()` pour
  que ratatui puisse le passer en `&mut` au render, sans exposer le champ directement.
  `list_state_mut()` est réservé au render : aucune mutation externe du ListState.
- Toutes les méthodes de navigation utilisent `saturating_sub(1)` sur la longueur des
  items (jamais `items.len() - 1` direct) — défense contre l'underflow usize si un guard
  `is_empty()` est absent ou contourné.

### Contrats issus du pre-mortem

- **Validation cursor ∈ items** : `CursorState` ne valide *pas* que son `cursor` figure
  dans `items` — c'est la responsabilité du state propriétaire. `StackState` conserve
  `resync_cursor_after_collapse()` ; elle met à jour `nav` via `nav.sync(&flat_items())`
  après avoir forcé le cursor via `set_cursor`.
- **`visible_height` par défaut = 1** : `CursorState::new()` initialise `visible_height`
  à 1 (pas 0) pour que `page_up/down` soit utilisable avant le premier render.
- **`set_visible_height` et ordre render/input** : dans la boucle TUI, le premier render
  se fait avant d'entrer dans la boucle d'événements — `set_visible_height` est donc
  toujours appelé avant le premier `page_up/page_down`. Le défaut à 1 est un filet de
  sécurité pour les tests unitaires de `CursorState` en isolation uniquement, pas une
  protection contre un cas réel.
- **`flat_items()` alloue intentionnellement** : `StackState::flat_items()` alloue un
  `Vec<StackCursor>` à chaque appel de navigation. C'est acceptable car déclenché
  uniquement sur input utilisateur (pas pendant le render). Pas de micro-optimisation.

### Architecture Decision Records

**ADR-1 : `CursorState<Id>` générique vs index pur**
- *Décision :* cursor = identité sémantique (`u32` serial, `StackCursor`), pas un `usize`
- *Rejeté :* `NavState { selected_idx: usize }` avec mapping index↔identité dans chaque state
- *Rationale :* l'identité stable à travers les mutations (filtre, collapse) est la propriété
  fondamentale de la navigation — la déléguer à chaque state propriétaire duplique le risque
  de désynchronisation

**ADR-2 : `items: &[Id]` passé à chaque appel**
- *Décision :* `CursorState` ne stocke pas la liste plate
- *Rejeté :* `set_items(Vec<Id>)` synchronisé à chaque mutation du state
- *Rationale :* les listes plates sont dérivées dynamiquement ; stocker une copie introduit
  un risque de désync si `set_items` est oublié. Coût d'allocation acceptable (input seul)

**ADR-3 : `sync_list_state` supprimé**
- *Décision :* callsites appellent directement `nav.sync(&self.flat_items())`
- *Rejeté :* wrapper `sync_list_state` → `nav.sync` (un wrapper sans logique propre)
- *Rationale :* rend le coût (allocation `flat_items()`) explicite à chaque callsite

**ADR-4 : `visible_height` dans `CursorState`, `thread_list_height` supprimé de `App`**
- *Décision :* chaque state navigable possède sa propre hauteur visible
- *Rejeté :* garder `thread_list_height` dans `App` et le passer en paramètre
- *Rationale :* la hauteur est une propriété du state navigable, pas de l'orchestrateur ;
  réduit le couplage render→input dans `App`

**ADR-5 : `FavoritesPanelState` prépare `nav` sans câbler la navigation**
- *Décision :* `nav: CursorState<usize>` ajouté, navigation non câblée dans `App`
- *Rejeté :* (a) ne rien faire, (b) câbler la navigation complète
- *Rationale :* (a) laisserait `pinned_cursor` dans `App` — incohérent après refactor ;
  (b) hors scope. La navigation favorites sera câblée dans une story dédiée
