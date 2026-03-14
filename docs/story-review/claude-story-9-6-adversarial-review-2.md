# Adversarial Review 2 — Story 9.6: Search & Favorites UX Polish

**Date:** 2026-03-12
**Reviewer:** Claude (adversarial mode)
**Artifact reviewed:** `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md`
**Round:** Post-corrections (after findings from review 1)

---

## Findings

1. **AC1 introduit un conflit avec le comportement Esc progressif existant dans StackFrames.**
   `handle_stack_frames_input()` à `app.rs:419-446` implémente déjà un Esc progressif : collapse
   collection → exit expansion → go back to threads. L'AC dit "Esc depuis StackFrames → focus
   retourne à ThreadList" sans préciser si ce comportement s'applique à tous les états ou
   seulement quand aucune expansion n'est active. Risque de casser l'Esc progressif existant.

2. **La Dev Notes AC3 contredit Task 3.2.**
   La section "AC3 – Already implemented" dit encore `"sets focus to ThreadList when the list
   becomes empty"`. Task 3.2 change exactement ce comportement vers un focus conditionnel.
   La Dev Note est fausse et trompera le dev agent.

3. **`status_bar.rs` est absent du Project Structure Notes.**
   La section "Status bar warning mechanism" prescrit d'ajouter `transient_message` à `App` et
   de modifier `status_bar.rs`. Aucun des deux ne figure dans le tableau Project Structure Notes.

4. **L'interface de `open_stack_for_selected_thread()` est non spécifiée.**
   La méthode lit-elle la sélection courante du thread list, ou reçoit-elle le serial en
   paramètre ? Les deux sont valides et incompatibles. Task 2.2e dépend de ce contrat mais ne
   le définit pas.

5. **`show_object_ids` propagé à `FavoritesPanel` mais `i` n'y est pas actif — incohérence.**
   Les IDs apparaissent dans les favoris (via `render_variable_tree`) quand le toggle est on,
   mais l'utilisateur ne peut pas les masquer depuis la Favorites panel. Non documenté comme
   choix délibéré dans l'AC ni dans les Dev Notes.

6. **Test 5.9 non-implémentable sans précisions sur la vérification du warning.**
   Le test doit vérifier que `transient_message` est positionné. La story ne dit pas comment
   mocker le moteur pour retourner zéro matches, ni quelle assertion faire sur l'état de `App`.

7. **`i` est réservé globalement dans `input.rs` mais documenté comme scoped à StackFrames.**
   La key est consommée pour tout focus. Toute story future voulant lier `i` dans ThreadList
   ou Favorites sera bloquée. Ce choix devrait être explicite dans la story.

8. **Task 1.7 n'indique pas de mettre à jour le call site original.**
   Après extraction dans une méthode privée, le code à `app.rs:368-373` doit appeler la
   nouvelle méthode. La story ne le mentionne pas — risque de duplication silencieuse.

9. **Aucun test ne couvre le frame positioning (Task 2.2f).**
   C'est l'étape la plus susceptible d'échouer silencieusement (frame_id mismatch). La story
   elle-même note ce risque de "silent degradation" mais ne prévoit aucun test pour le valider.

10. **Test 5.4 est un test de rendu sans infrastructure définie.**
    Vérifier qu'une barre de recherche est "visible" nécessite soit du rendu ratatui (Frame/Rect
    mock), soit de tester un prédicat d'état. La story ne tranche pas — le test est ambigu.

11. **Dev Notes AC2 référence "Task 2.2c" pour les warnings de duplicates mais le label est "2.2d".**
    Après renommage des étapes de Task 2.2, les warnings dupliqués sont à l'étape d, pas c.
    Référence cassée dans les Dev Notes.

12. **Aucun test ne couvre le comportement Esc StackFrames → filtre préservé (promesse principale d'AC1).**
    Les tests 5.1–5.7 couvrent uniquement les transitions dans ThreadList. Le chemin
    "revenir de StackFrames via Esc → filtre toujours actif" n'est pas testé.
