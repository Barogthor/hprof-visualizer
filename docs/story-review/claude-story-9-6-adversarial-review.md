# Adversarial Review — Story 9.6: Search & Favorites UX Polish

**Date:** 2026-03-12
**Reviewer:** Claude (adversarial mode)
**Artifact reviewed:** `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md`

---

## Findings

1. **AC1 ne spécifie pas le comportement de Esc dans StackFrames vis-à-vis du filtre.**
   L'AC dit "a subsequent Esc from the thread list (not from the stack view) clears the filter"
   mais ne décrit pas ce qui se passe quand l'utilisateur est dans StackFrames et presse Esc :
   retourne-t-il à ThreadList ? Le filtre persiste-t-il ? Le chemin "revenir aux threads puis
   Esc" est supposé mais non décrit. Un dev peut implémenter n'importe quoi pour ce cas.

2. **AC2 promet "highlighting the position" mais l'implémentation ne le garantit pas.**
   Task 2.2f dit "best-effort… leave cursor at top if no match". Le cas "pas de match sur
   frame_id" est probable (cf. note type mismatch). En pratique AC2 peut naviguer vers le bon
   thread sans positionner le curseur. L'AC est trompeuse sur ce point.

3. **Dépendance implicite non documentée entre Task 2 et Task 1.7.**
   Task 2.2e appelle `open_stack_for_selected_thread()` qui n'existe qu'après Task 1.7. La
   story ne signale pas cette dépendance. Un dev qui attaque Task 2 en premier ne compile pas.

4. **Task 3.2 change silencieusement le comportement existant sans le justifier.**
   Les Dev Notes AC3 disent "focus returns to ThreadList" (comportement actuel). Task 3.2
   change cela vers StackFrames conditionnellement. C'est une breaking behavior change déguisée
   en "fix". Aucun argument ne justifie pourquoi StackFrames est préférable à ThreadList.

5. **Le mécanisme d'émission de warning vers la status bar n'est pas documenté.**
   Task 2.2d prescrit "emit status bar warning" pour deux cas mais aucune dev note n'explique
   l'API — quel champ sur `App`, quelle méthode, quel format. Le fichier `status_bar.rs` n'est
   pas dans les References. Un dev sans contexte Stories 3.x devra deviner.

6. **Test 5.3 n'est pas testable en unit test sur `ThreadListState`.**
   Le test vérifie que Esc "depuis un autre focus ne clear pas le filtre" mais le routing par
   focus est dans `App::handle_input()`, pas dans `ThreadListState`. Pour être valide, ce test
   doit exercer `App` avec `focus = StackFrames`. La story ne précise ni le niveau ni le fichier
   cible du test.

7. **Le grep prescrit en Task 1.1 est trop étroit.**
   Task 1.1 dit de grepper les tests pour `activate_search`/`deactivate_search`. Mais si
   d'autres call sites non-test (dans `App` ou ailleurs) dépendent du comportement clear-filter
   actuel de `deactivate_search()`, ils ne seront pas trouvés. Le grep doit couvrir tout le
   codebase, pas seulement les tests.

8. **AC4 exclut implicitement la Favorites panel sans le dire.**
   L'AC dit "any expanded object node in the stack frame view". La Favorites panel affiche aussi
   des objets expandés mais `i` ne fait rien là-bas (Task 4.3). Cette limitation n'est ni dans
   l'AC ni dans les Dev Notes — elle sera découverte par l'utilisateur comme un bug.

9. **`show_object_ids` est éphémère mais la story ne le dit pas.**
   Un dev consciencieux pourrait le persister dans le TOML de config (Epic 6 pattern). La story
   ne dit pas "éphémère seulement, ne pas persister". Ce silence invite la sur-ingénierie.

10. **La Definition of Done compte 15 tests mais le compte réel est supérieur.**
    Tests 5.12 et 5.14 sont décrits comme "parametric" couvrant plusieurs cas chacun. Le compte
    réel est 17+. Un DoD avec un nombre fixe de tests paramétriques est faux dès le départ.

11. **AC2 pour `PinnedSnapshot::Primitive` n'est pas couvert.**
    Un integer pinné a un `PinKey::Var` avec `frame_id`. La navigation fonctionne mais
    "highlighting the position" sur une valeur primitive n'a pas de sens — pas d'objet à
    sélectionner. Le comportement dans ce cas n'est pas spécifié.

12. **Task 4.5 ne tranche pas entre réutiliser `THEME.null_value` ou créer `THEME.object_id`.**
    `null_value` est sémantiquement incorrect pour les IDs d'objet. La story propose les deux
    options sans décider. Un dev utilisera l'un, un autre utilisera l'autre — incohérence
    garantie si la story est implémentée par plusieurs personnes ou reprise plus tard.
