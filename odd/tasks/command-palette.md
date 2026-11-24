# command-palette

Objective: a command palette (Ctrl+Shift+P) to find and run any action. Branch `feat/command-palette` (from main daf9b0c). Commits only when Rafael asks; run `./install.sh` after he approves. Runner: `cargo test --workspace --offline`, `cargo clippy --workspace --all-targets --offline`.

## Specs

- S1: "Ahora vamos con la tarea numero 4." — item 4 of the list: "Paleta de comandos (`Ctrl+Shift+P`). Busca y ejecuta cualquier acción: nueva sesión, cambiar de cuenta, reanudar una sesión guardada, abrir un proyecto."
- S2 (design announced in chat, assumptions): overlay at the top center with a search field and a filtered list; ↑/↓ move, Enter runs, Esc closes; fuzzy matching favoring word starts and consecutive letters; entries: every action with its shortcut, go to each open session, resume each saved session, open each agent, switch to each Claude account. "Abrir un proyecto" covered by going to / resuming sessions of that project (no separate project list yet).

## Tasks

| ID | Specs | Route | Status | Evidence |
|---|---|---|---|---|
| T1 | S2 | inline | done in code | `palette.rs`: `score` (in-order letters, case-insensitive, DP best placement: letter 1, run +8, word start +6, first +10) and stable `filter` — RED observed, 4 tests (rebalanced run vs word start after a failing case) |
| T2 | S1 S2 | inline | done; user confirmed ("Sí, funciona") | `workspace/command_palette.rs` overlay (field, top 12 matches with hints, selection highlight, ↑/↓ via the container since the field ignores arrows, Enter/Tab, Esc, click outside), `CommandPalette` key context excluded from shortcuts (test), `TogglePalette` Ctrl+Shift+P |
| T3 | S1 S2 | inline | done; user confirmed ("Sí, funciona") | entries: actions with shortcuts (Remove from split only in a split), switch to each other Claude account, open each installed agent, Go to each open session, Resume each saved one; each runs the existing handler after closing the palette; README; 210 tests, clippy 0 |

## Log

- L1 (2026-11-24, user): "Ahora vamos con la tarea numero 4."
- L2 (2026-11-24, user): "Sí, funciona" → T2, T3 confirmed; installed with ./install.sh.
