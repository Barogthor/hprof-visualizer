---
title: 'Refactor StackState — Extract ExpansionRegistry'
slug: 'refactor-stack-state-expansion-registry'
created: '2026-03-11'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
reviewed: true
tech_stack: ['Rust edition 2024', 'ratatui', 'hprof_engine types']
files_to_modify:
  - 'crates/hprof-tui/src/views/stack_view/state.rs'
  - 'crates/hprof-tui/src/views/stack_view/format.rs'
  - 'crates/hprof-tui/src/views/stack_view/mod.rs'
  - 'crates/hprof-tui/src/views/stack_view/tests.rs'
  - 'crates/hprof-tui/src/app/mod.rs'
  - 'crates/hprof-tui/src/favorites.rs'
files_to_create:
  - 'crates/hprof-tui/src/views/stack_view/expansion.rs'
code_patterns:
  - 'pub(super) pour champs intra-module stack_view'
  - 'pub(crate) sur le champ expansion pour accès depuis app/mod.rs'
  - 'free functions pub(super) dans format.rs'
  - 'use super::* dans tests.rs'
test_patterns:
  - 'Unit tests dans stack_view/tests.rs avec helpers make_frame/make_var'
  - 'Accès directs aux champs internes via pub(super) (même module)'
---

# Tech-Spec: Refactor StackState — Extract ExpansionRegistry

**Created:** 2026-03-11

## Overview

### Problem Statement

`stack_view/state.rs` (~960 lignes) mélange quatre responsabilités dans un seul struct
à 9 champs : navigation/curseur de frame, cycle de vie des expansions d'objets,
pagination des collections, et utilitaires de rendu. Cette concentration nuit à la
lisibilité et à la maintenabilité.

### Solution

Extraire un sous-struct `ExpansionRegistry` dans un nouveau fichier `expansion.rs`
qui regroupe `object_phases`, `object_fields`, `object_errors` et `collection_chunks`
avec leurs méthodes propres. `StackState` le possède comme champ `pub(crate)` et délègue.
Déplacer `format_entry_line` (méthode statique sur `StackState`) vers `format.rs`.
Ajouter des commentaires de section dans `state.rs` pour délimiter visuellement
les responsabilités restantes.

### Scope

**In Scope:**
- Nouveau fichier `expansion.rs` contenant `ExpansionRegistry`
- Mise à jour de `state.rs` : champ `expansion: ExpansionRegistry`, délégation des
  méthodes concernées, commentaires de section
- Migration de `format_entry_line` vers `format.rs`
- Mise à jour des usages dans `app/mod.rs` (`s.collection_chunks.*` → `s.expansion.collection_chunks.*`)
- Mise à jour de `mod.rs` (déclaration du module `expansion`)
- Migration des accès directs dans `tests.rs`

**Out of Scope:**
- Changement de la signature de `tree_render::render_variable_tree` (4 params conservés)
- Déplacement de `build_items` vers `widget.rs`
- Tout changement de comportement ou de logique

## Context for Development

### Codebase Patterns

- Modules Rust dans `crates/hprof-tui/src/views/stack_view/` : `state.rs`, `types.rs`,
  `format.rs`, `widget.rs`, `mod.rs`, `tests.rs`
- `pub(super)` pour les champs internes au module ; `pub(crate)` pour le champ
  `expansion` sur `StackState` (accédé par `app/mod.rs`)
- `format.rs` : free functions `pub(crate)` — imports existants :
  `hprof_engine::{FieldInfo, FieldValue, FrameInfo}`, `ratatui::style::Style`,
  `THEME`, `ExpansionPhase` — `format_entry_line` nécessite d'ajouter `EntryInfo`
- `tree_render::render_variable_tree` reçoit les 4 maps en paramètres séparés —
  après refactoring, `build_items` passera `&self.expansion.object_fields`, etc.
- `app/mod.rs` — 11 accès directs à `s.collection_chunks` répartis en :
  - `remove(&cid)` : lignes 409, 625, 748, 772 (×4)
  - `contains_key(&oid/cid)` : lignes 484, 503, 538 (×3)
  - `insert(cid, chunks)` : ligne 602 (×1)
  - `get_mut(&cid)` : lignes 611, 734, 763 (×3)
- `tests.rs` — accès directs `state.object_phases` (×1) et
  `state.collection_chunks` (×4) → migrent vers `state.expansion.*`

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-tui/src/views/stack_view/state.rs` | Source principale — struct à refactorer (~960 lignes) |
| `crates/hprof-tui/src/views/stack_view/types.rs` | Types: `ExpansionPhase`, `ChunkState`, `CollectionChunks` (non modifié) |
| `crates/hprof-tui/src/views/stack_view/format.rs` | Destination de `format_entry_line` |
| `crates/hprof-tui/src/views/stack_view/mod.rs` | Déclaration du module `expansion` |
| `crates/hprof-tui/src/views/stack_view/tests.rs` | 5 accès directs aux champs à migrer |
| `crates/hprof-tui/src/app/mod.rs` | 11 accès `s.collection_chunks` → `s.expansion.collection_chunks` |
| `crates/hprof-tui/src/views/tree_render.rs` | Consommateur des 4 maps — pas de changement d'API |

### Technical Decisions

- `expansion: ExpansionRegistry` est `pub(crate)` sur `StackState` — `app/mod.rs`
  accède directement à `s.expansion.collection_chunks.*` sans wrappers délégataires
  (cérémonie inutile, risque borrow checker évité)
- Champs internes d'`ExpansionRegistry` : tous `pub(crate)` — `state.rs`, `tests.rs`
  (sous-module de `stack_view`) et `favorites.rs` (test interne) y accèdent tous.
  `pub(super)` serait insuffisant : il ne couvre pas les sous-modules enfants comme `tests`
- `format_entry_line` : `pub(super)` dans `format.rs`, non re-exportée dans `mod.rs`
- `set_expansion_failed` splitté — ordre impératif :
  1. `self.expansion.set_expansion_failed(object_id, error)` (mutation d'abord)
  2. `if self.flat_index().is_none()` (check curseur sur état déjà mis à jour)
- `collapse_object_recursive` reste sur `StackState` : boucle puis
  `resync_cursor_after_collapse` en dernier (dépend du curseur)
- `expansion_state` sur `ExpansionRegistry` : `pub(super)` — délégation depuis
  `StackState::expansion_state` (Rust ne requiert pas la même visibilité)

## Implementation Plan

### Tasks

- [ ] **T1 — Créer `expansion.rs`**
  - File: `crates/hprof-tui/src/views/stack_view/expansion.rs`
  - Action: Créer le fichier avec le module docstring, le bloc `use` minimal, le struct
    et son impl
  - Notes:
    - Bloc `use` minimal :
      ```rust
      use std::collections::HashMap;
      use hprof_engine::FieldInfo;
      use super::types::{ChunkState, CollectionChunks, ExpansionPhase};
      ```
      (`CollectionPage` **non inclus** — non utilisé directement ; il est encapsulé
      dans `CollectionChunks`. L'inclure déclencherait un warning clippy sous `-D warnings`.)
    - Module docstring obligatoire (CLAUDE.md) :
      ```rust
      //! Expansion lifecycle state for the stack view.
      //!
      //! [`ExpansionRegistry`] owns all object expansion data (phases, decoded fields,
      //! errors) and collection pagination state, decoupled from cursor and frame logic.
      ```
    - Struct :
      ```rust
      pub struct ExpansionRegistry {
          pub(crate) object_phases: HashMap<u64, ExpansionPhase>,
          pub(crate) object_fields: HashMap<u64, Vec<FieldInfo>>,
          pub(crate) object_errors: HashMap<u64, String>,
          pub(crate) collection_chunks: HashMap<u64, CollectionChunks>,
      }
      ```
      **Visibilité `pub(crate)` obligatoire** (pas `pub(super)`) : `pub(super)` sur un
      champ de `expansion.rs` signifie "visible dans `stack_view`" (parent direct), mais
      PAS dans `stack_view::tests` (sous-module enfant) ni dans `favorites.rs` (hors module).
      `tests.rs` accède à `state.expansion.object_phases` / `state.expansion.collection_chunks`
      directement ; `favorites.rs` (test à la ligne 686) accède aussi à `collection_chunks`.
    - Méthodes à migrer depuis `state.rs` :
      `new`, `expansion_state` (**`pub(crate)`** — deux `expansion_state` existeront :
      celle-ci sur `ExpansionRegistry`, et une `pub fn expansion_state` sur `StackState`
      qui délègue ; Rust autorise une délégation `pub → pub(crate)`),
      `set_expansion_loading`, `set_expansion_done`,
      **`set_expansion_failed` (mutation uniquement, sans logique curseur)** :
      insère dans `object_errors` et `object_phases` seulement — la récupération du
      curseur et `sync_list_state()` restent dans `StackState::set_expansion_failed`,
      `cancel_expansion`,
      `collapse_object` (**NE PAS** toucher `collection_chunks` dans cette méthode —
      `app/mod.rs` gère ce cycle de vie explicitement via `s.expansion.collection_chunks.remove`),
      `chunk_state`
    - `collapse_object_recursive` **reste** sur `StackState`

- [ ] **T2a — Mettre à jour `state.rs` — Structure** *(compiler avant de continuer)*
  - File: `crates/hprof-tui/src/views/stack_view/state.rs`
  - Action: Remplacer les 4 champs par `pub(crate) expansion: ExpansionRegistry`,
    ajouter l'import, initialiser dans `new()`
  - Notes:
    - Ajouter `use super::expansion::ExpansionRegistry;`
    - `expansion: ExpansionRegistry::new()` dans `Self { ... }`
    - Lancer `cargo build` et corriger les erreurs avant de passer à T2b

- [ ] **T2b — Mettre à jour `state.rs` — Méthodes**
  - File: `crates/hprof-tui/src/views/stack_view/state.rs`
  - Action: Écrire le test AC9, déléguer les méthodes, adapter toutes les méthodes
    qui lisent les 4 maps, ajouter les sections, supprimer `format_entry_line`
  - Notes:
    - **Le test AC9 existe déjà** (`set_expansion_failed_recovers_cursor_from_loading_node_top_level`
      ligne 194 de `tests.rs`) — ne pas réécrire. Vérifier qu'il reste vert après
      l'adaptation de `set_expansion_failed`.
    - Méthodes à déléguer : `expansion_state`, `set_expansion_loading`,
      `set_expansion_done`, `cancel_expansion`, `collapse_object`, `chunk_state`
    - `set_expansion_failed` — **ordre impératif dans le wrapper `StackState`** :
      1. `self.expansion.set_expansion_failed(object_id, error)` (mutation phases/errors)
      2. `if self.flat_index().is_none()` + logique curseur (récupération vers OnVar)
      3. `self.sync_list_state()` (**ne pas oublier** — son absence cause un desync
         visuel silencieux entre le curseur logique et le highlight ratatui)
    - `collapse_object_recursive` : boucle sur ids → `self.expansion.collapse_object(id)`,
      puis appel unique de `resync_cursor_after_collapse` après la boucle.
      **Adapter aussi** `collect_descendants(object_id, &self.expansion.object_fields, ...)`
      dans le corps de cette méthode (call site non délégué)
    - Méthodes à adapter (remplacer `self.X` → `self.expansion.X`) :
      `resolve_object_at_path`, `collection_entry_obj_cursor_field`,
      `selected_loading_object_id`, `selected_field_ref_id`,
      `selected_field_collection_info`, `selected_collection_entry_count`,
      `selected_chunk_info`, `selected_collection_entry_ref_id`,
      `selected_collection_entry_obj_field_ref_id`,
      `emit_object_children`, `emit_collection_children`,
      `emit_collection_entry_obj_children`, `toggle_expand`,
      `build_items` (inclut l'appel `render_variable_tree` — mettre à jour ses
      4 arguments : `&self.expansion.object_fields`, `&self.expansion.collection_chunks`,
      `&self.expansion.object_phases`, `&self.expansion.object_errors`)
    - Ajouter les 4 commentaires de section dans l'ordre :
      `// === Frames & Vars ===`, `// === Cursor & Navigation ===`,
      `// === Expansion (delegated) ===`, `// === Rendering ===`
    - Supprimer `format_entry_line` du fichier — **mettre à jour le call-site** :
      `Self::format_entry_line(...)` → `super::format::format_entry_line(...)`
      (ou ajouter `use super::format::format_entry_line;` en haut du fichier)
    - **Vérification** : `grep -n 'object_phases\|object_fields\|object_errors\|collection_chunks' state.rs`
      — aucun champ direct résiduel

- [ ] **T3 — Mettre à jour `format.rs`**
  - File: `crates/hprof-tui/src/views/stack_view/format.rs`
  - Action: Ajouter `EntryInfo` aux imports et ajouter `format_entry_line`
  - Notes:
    - Ajouter `EntryInfo` dans `use hprof_engine::{..., EntryInfo}`
    - Coller le corps de `format_entry_line` depuis `state.rs` en `pub(super) fn`
    - Signature inchangée : `fn format_entry_line(entry: &EntryInfo, indent: &str, value_phase: Option<&ExpansionPhase>) -> String`
    - Non re-exportée dans `mod.rs`
    - **Vérification** : `grep -n 'format_entry_line' state.rs` — aucune occurrence
    - Confirmer aussi : `grep -rn 'format_entry_line' src/` — seul `format.rs` matche

- [ ] **T4 — Mettre à jour `mod.rs`**
  - File: `crates/hprof-tui/src/views/stack_view/mod.rs`
  - Action: Ajouter `mod expansion;`
  - Notes:
    - Pas de re-export de `ExpansionRegistry` — aucun usage hors du module
    - Ne **pas** ajouter `format_entry_line` à la liste `pub(crate) use format::{...}`
      existante — elle reste `pub(super)` et interne à `stack_view`

- [ ] **T5 — Mettre à jour `app/mod.rs`**
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action: Remplacer les 11 occurrences de `s.collection_chunks` par
    `s.expansion.collection_chunks`
  - Notes:
    - Utiliser grep pour localiser : `grep -n 'collection_chunks' app/mod.rs`
      (les numéros de lignes peuvent avoir changé depuis la rédaction du spec)
    - 4 patterns à remplacer : `.remove(`, `.contains_key(`, `.insert(`, `.get_mut(`

- [ ] **T6 — Mettre à jour `tests.rs` et `favorites.rs`**
  - Files:
    - `crates/hprof-tui/src/views/stack_view/tests.rs`
    - `crates/hprof-tui/src/favorites.rs`
  - Action: Migrer les accès directs aux champs (**uniquement les accès directs,
    pas les appels de méthodes publiques** — `state.expansion_state(...)` et autres
    appels de méthodes sur `StackState` ne nécessitent aucune modification)
  - Notes:
    - `tests.rs` : `state.object_phases.is_empty()` → `state.expansion.object_phases.is_empty()`
    - `tests.rs` : `state.collection_chunks.insert(...)` (×4) → `state.expansion.collection_chunks.insert(...)`
    - `favorites.rs` (test ligne 686) : `state.collection_chunks.insert(...)` →
      `state.expansion.collection_chunks.insert(...)`
    - Le test AC9 existe déjà (`set_expansion_failed_recovers_cursor_from_loading_node_top_level`)
      — **ne pas réécrire**. Vérifier qu'il passe après le refactoring.

- [ ] **T7 — Vérifier**
  - Action: `cargo build --all-targets && cargo test && cargo clippy --all-targets -- -D warnings`
  - Notes: Tous les tests doivent passer, aucun warning clippy

### Acceptance Criteria

- [ ] **AC1** — Given le projet compile, When `cargo build --all-targets` est exécuté,
  Then aucune erreur de compilation.
- [ ] **AC2** — Given les tests existants, When `cargo test` est exécuté,
  Then tous les tests passent sans modification.
- [ ] **AC3** — Given `state.rs`, When on recherche les champs `object_phases`,
  `object_fields`, `object_errors`, `collection_chunks`, Then aucun ne subsiste comme
  champ direct sur `StackState` — seul `expansion: ExpansionRegistry` est présent.
- [ ] **AC4** — Given `expansion.rs`, When le fichier est ouvert, Then il contient
  `pub struct ExpansionRegistry` avec les 4 champs `pub(super)` et les méthodes migrées.
- [ ] **AC5** — Given `format_entry_line`, When on recherche dans `state.rs`,
  Then aucune occurrence — la fonction existe dans `format.rs` avec la même signature.
- [ ] **AC6** — Given `state.rs`, When on lit le fichier, Then les quatre commentaires
  de section sont présents dans l'ordre : `// === Frames & Vars ===`,
  `// === Cursor & Navigation ===`, `// === Expansion (delegated) ===`,
  `// === Rendering ===`.
- [ ] **AC7** — Given `clippy`, When `cargo clippy --all-targets -- -D warnings` est
  exécuté, Then aucun warning.
- [ ] **AC8** — Given `app/mod.rs`, When on recherche `s\.collection_chunks` (point échappé),
  Then aucune occurrence — tous les accès sont via `s.expansion.collection_chunks`.
- [ ] **AC9** — Given un `StackState` avec curseur `OnObjectLoadingNode { frame_idx: 0,
  var_idx: 0, field_path: [] }` pour l'objet X, When `set_expansion_failed(X, err)`
  est appelé, Then le curseur est `OnVar { frame_idx: 0, var_idx: 0 }`.
  Setup requis pour le test : 1 frame expansée avec 1 var `ObjectRef { id: X }`,
  objet X en phase `Loading` via `state.set_expansion_loading(X)`, curseur positionné
  sur `OnObjectLoadingNode` via `state.set_cursor(...)`.

## Additional Context

### Dependencies

Aucune dépendance externe nouvelle. Refactoring interne au crate `hprof-tui`.

### Testing Strategy

Refactoring pur — un seul nouveau test nécessaire (comportement non couvert
identifié par pre-mortem) :

- **`set_expansion_failed_with_loading_cursor_recovers_to_var`** dans `tests.rs` :
  vérifie AC9 — curseur `OnObjectLoadingNode` (field_path vide) → `OnVar` après
  `set_expansion_failed`
- Écrire ce test en T2b **avant** d'adapter `set_expansion_failed` (TDD)
- Tous les autres comportements sont couverts par les tests existants dans
  `stack_view/tests.rs` et `tree_render.rs`

### Notes

- `collapse_object_recursive` reste sur `StackState` : appelle
  `resync_cursor_after_collapse` (dépend du curseur) — couplage circulaire si migré.
  Son call site `collect_descendants(_, &self.object_fields, ...)` doit être adapté
  en `&self.expansion.object_fields` (non listé dans les méthodes déléguées — adapté
  directement dans le corps).
- `tree_render::render_variable_tree` reçoit toujours 4 params séparés — une future
  story pourrait unifier avec `&ExpansionRegistry`, mais hors scope ici.
- Risque borrow checker écarté : pas de getter `&mut CollectionChunks` exposé ;
  `app/mod.rs` accède directement à `s.expansion.collection_chunks`.
- `ExpansionRegistry` est `pub(crate)` structurellement (accessible via
  `s.expansion` depuis `app/mod.rs`) mais le type lui-même n'est pas re-exporté —
  les consommateurs accèdent aux champs sans nommer le type dans un `use`.
- `format_entry_line` reste `pub(super)` : aucun usage externe aujourd'hui.
  Si un futur consommateur hors `stack_view/` en a besoin, élever à `pub(crate)`
  et ajouter au re-export de `mod.rs`.
