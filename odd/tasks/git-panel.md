# git-panel

Objective: a read-only git panel for the focused session. Branch `feat/git-panel` (from feat/agent-status-all f56acd3). Commits only when Rafael asks. Runner: `cargo test --workspace --offline`, `cargo clippy --workspace --all-targets --offline`.

## Specs

- S1: "Vamos mejor con la 3." — item 3 of the proposal: "Panel de git por sesión. Muestra la rama, los archivos modificados y un visor de diff de lo que cambió el agente, sin salir de la terminal."
- S2 (design announced in chat, assumptions): right-side panel toggled by a title bar button and Ctrl+Shift+G; repo of the focused pane's cwd; branch with ahead/behind; changed files with status letter and +/− lines; click → colored diff vs HEAD (untracked shown as all added); refresh every 2 s while open and on demand; read-only; uses the system `git`, no new dependency.

## Tasks

| ID | Specs | Route | Status | Evidence |
|---|---|---|---|---|
| T1 | S1 S2 | inline | done in code | `git.rs` parse_status (porcelain v2 -z: headers, 1/2/u/?, renames), apply_numstat (-z, renames, binaries), parse_diff — RED observed, 4 tests |
| T2 | S1 S2 | inline | done in code | `repo_root`, `status` (+numstat vs HEAD, skipped without HEAD), `diff` (tracked vs HEAD with -M; untracked via --no-index /dev/null), git run with --no-optional-locks, GIT_OPTIONAL_LOCKS=0, no color, --no-ext-diff, 5000-line cap — RED observed, 2 tests on a real temp repo |
| T3 | S1 S2 | inline | done; user confirmed ("Sí, funciona") | `workspace/git_panel.rs`: header (repo, branch ↑↓, ⟳, ✕), file list (status letter colored, name + faint dir, +/−), diff view (← Files, colored lines in the terminal font), background refresh, selection reset when the repo changes or the file is no longer changed |
| T4 | S2 | inline | done; user confirmed ("Sí, funciona") | `ui/git.svg` title bar button, `ToggleGitPanel` Ctrl+Shift+G, refresh on the 2 s tick while open; README section; 206 tests ×3, clippy 0 |

Route evidence: one feature, shared workspace files; inline.

## Log

- L1 (2026-11-24, user): "Vamos mejor con la 3."
- L2 (2026-11-24, user): committed agent-status work first ("Commit then branch (Recommended)"): f4349c0, 739cd41, f56acd3 (first attempt left the Gemini svg deletion in the wrong commit; redone locally before any push, each commit checked with cargo check).
- L3 (2026-11-24): instance race test hardened again (claims that error count as not primary): git tests spawn more processes, so forked copies of the lock show up more often. Never two primaries is still asserted.
- L4 (2026-11-24, user): "Sí, funciona" → T3, T4 confirmed.
