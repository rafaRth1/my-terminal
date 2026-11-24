# terminal-extras

Objective: seven quality-of-life features on top of the MVP. Branch `feat/terminal-extras` (from main 4077ade). Delivery: ask-on-risk; commits only when Rafael asks (his rule overrides automatic work-unit commits). Test runner: `cargo test --workspace`, `cargo clippy --workspace --all-targets`.

## Specs

User request (L1): "Vamos por el momento todas menos la 1." — items 2–8 of the list proposed in the conversation:

- S1 (item 2): "Notificación de escritorio cuando Claude pasa a *Waiting* (pide permiso) o a *Done*, y la app no tiene el foco." Via `notify-send`; nothing when the window is active.
- S2 (item 3): "Links clicables: `Ctrl+clic` sobre una URL o una ruta `archivo:línea` la abre."
- S3 (item 4): "Buscar en la salida con `Ctrl+F` dentro de la terminal, con resaltado y siguiente/anterior." Shortcut changed to Ctrl+Shift+F by the user (L2).
- S4 (item 5): "Configuración: fuente, tamaño (`Ctrl+=` / `Ctrl+-`) y tema, en un archivo `config.toml`."
- S5 (item 6): "Restaurar el split: que las sesiones reanudadas vuelvan a su disposición original."
- S6 (item 7): "Una sola instancia: que abrir la app de nuevo enfoque la ventana existente, y así no se pisa `sessions.json`."
- S8 (see L5): "Con respecto a los links tambien puedes agregar que sean rutas de las carpetas" → answer: "Osea al hacer click a una ruta de carpeta, debe abrirse esa carpeta con el explorador de archivos en este caso de linux".
- S9 (see L5): "por otro lado no me salio la config o la tuerca" → a gear icon opening a settings dialog (font, size, theme) that edits `config.toml`.
- S7 (item 8): "Renombrar una sesión desde la barra lateral con clic derecho → *Rename*."

## Tasks

| ID | Specs | Route | Status | Evidence |
|---|---|---|---|---|
| T1 | S7 | inline | done in code; not exercised by user | `SavedSession.name` (optional, old files load; RED observed), `saved::clean_name`; `workspace/rename.rs` inline TextField in the row (Enter keeps, Esc cancels, empty = automatic title); session menu now lists several actions (Add/Remove from split, Rename, Forget); names win over automatic titles and survive restarts; 156 tests, clippy 0 |
| T2 | S1 | inline | done in code; not exercised by user | `notify.rs`: `message(prev, next, window_active)` (Waiting/Done only when status changes and window inactive), `escape` (markup, 80 chars) — RED observed, 2 tests; `send` via `notify-send --app-name=my-terminal` (spawned, reaped by a thread); window activation tracked with `observe_window_activation`; body = Claude notification text when waiting; manual `notify-send` arg check exit 0 |
| T3 | S3 | inline | done in code; not exercised by user | Shortcut decision (L2). `term_core::search` (`find_all` literal + smart case via alacritty `RegexIter`, 1000 cap; `segments`; `reveal`) RED observed, 4 tests; `TextFieldEvent::SubmitBack` (Shift+Enter, RED observed); pane search bar (`terminal_view/search_bar.rs`): highlights all matches, current in yellow, n/m counter, ↑ ↓ ✕, Enter older / Shift+Enter newer, Esc closes, matches refreshed on new output; terminal ignores keys while its bar has focus; sidebar search moved to Ctrl+Shift+K; 162 tests, clippy 0 |
| T4 | S2 | inline | done in code; not exercised by user | `term_core::links` (`find` token under a char: http/https/file URLs with trailing punctuation trimmed, paths with `/` or `name.ext:line`, `:line:col`; `link_at` maps grid columns incl. wide chars) RED observed, 3 tests; `open_link.rs` (`resolve` ~ and cwd, `editor` VISUAL>EDITOR>nvim, `editor_command` +line with single-quoted path) RED observed, 3 tests; Ctrl+click emits `OpenLink`; URL → browser, existing file → new tab `editor +line path`, no editor → xdg default app; 168 tests, clippy 0 |
| T5 | S4 | inline | done in code; not exercised by user | `config.rs` (`$XDG_CONFIG_HOME/my-terminal/config.toml`: font_family, font_size 6..=48, theme dark/light; unknown keys and bad values reported, defaults kept; `zoom`) RED observed, 4 tests; `toml` 0.8 added to workspace deps (0.8.23 already in Cargo.lock, built --offline: no new crate); light theme + `search_current_fg` (RED observed, 1 test); `TerminalSettings` gpui global read by every pane (font family/size/theme, line height); Ctrl+= / Ctrl++ / Ctrl+- / Ctrl+0 zoom all panes for the run (not written back); smoke: temp config font_size 99 → "font_size 99 is outside 6..=48; using 48", no panic; 173 tests, clippy 0 |
| T6 | S6 | inline | done in code; activation not seen by user | `instance.rs` (`claim`: connect → ask "activate" and exit; stale socket replaced; non-socket file never touched; `listen` thread, 64-byte single-line requests) RED observed, 3 tests on real sockets; `main.rs` claims before gpui starts (`Start::Run/Exit`), activates the first window on request; only with `$XDG_RUNTIME_DIR`; smoke: second start exited 0 after 8 ms with "already running; brought its window to the front"; 176 tests, clippy 0 |
| T8 | S8 | inline | done; user confirmed ("Sí, funciona") | `open_link::target` (folder → file manager via `file_url`, file → editor, no editor → default app) and percent-encoded `file_url` — RED observed, 2 tests |
| T9 | S9 | inline | done; user confirmed ("Sí, funciona") | gear button (`assets/ui/settings.svg`) → `workspace/settings_modal.rs` (font family on Enter, size −/+, Dark/Light), live via `TerminalSettings` global and saved with `config::save` (temp + rename) — RED observed (save round trip, `SettingsModal` key context blocks shortcuts) |
| T7 | S5 | inline | done in code; resume into split not exercised by user | `Tab.id` (stable, `open_tab_with_id`, `reserve_ids_through`, `insert_into` at a place with a width, others shrink in proportion) RED observed, 3 tests; `SavedSession.split/position/width` (bad widths dropped) RED observed, 1 test; save writes split id, place and width for tabs with 2+ panes; load reserves saved ids; resume puts a pane into its split's tab when open (place from restored positions), else opens the tab with that id; smoke: saved split (ids, positions, widths, name) loaded and written back unchanged; README updated; 180 tests, clippy 0 |

Route evidence: units share `workspace.rs` / `terminal_view.rs`, so parallel writers would collide; inline, one at a time.

## Log

- L1 (2026-11-23, user): "Vamos por el momento todas menos la 1."
- L2 (2026-11-23, user, AskUserQuestion): terminal search shortcut → "Ctrl+Shift+F in pane (Recommended)": Ctrl+Shift+F searches the focused terminal; sidebar search moves to Ctrl+Shift+K.
- L3 (2026-11-23): decision for S2: no $EDITOR/$VISUAL set and nvim installed → files open in a new tab with `$VISUAL`/`$EDITOR`/`nvim +line path`, else the default app.
- L4 (2026-11-23): all seven tasks done in code; checks: `cargo test --workspace` 180 passed, `cargo clippy --workspace --all-targets` 0 warnings, release smokes (config warning, single instance 8 ms exit, split round trip). Visual checks pending for the user. No commits (Rafael commits on request only).
- L5 (2026-11-23, user): "Con respecto a los links tambien puedes agregar que sean rutas de las carpetas, por otro lado no me salio la config o la tuerca" + AskUserQuestion answer quoted in S8 → T8, T9.
- L6 (2026-11-23): independent verifier (risk: guards around shell commands, local socket, external process; toml dependency). Blocker B1: simultaneous starts could both become primary (4/100). Correction batch: lock file (`File::try_lock`, held for the process lifetime) decides the primary, later starts retry connecting for 1 s; listener read timeout 1 s (silent client no longer blocks); config parsed key by key (one bad key keeps the others; nan reported); tab ids never overflow; saved titles/names drop control chars, huge split ids dropped; relative paths starting with `-` get `./`; search keeps the newest 1000 matches; MSRV 1.85 → 1.89 for `File::try_lock` (README updated; let-chains lint fixed in ipc.rs). New regression tests from the verifier probes (race ×200, silent client, bad key, huge id, control chars, `-` path, newest matches). Recheck of B1: instance tests 10/10 runs green. Not changed: URLs stop at `(`; commands typed through the PTY assume a UTF-8 shell.
- L7 (2026-11-23): Gentle review: assess high (`review_due`), consent relayed to Rafael → "Revisar este cambio" (granted). Lineage `review-737a8ffc82b99dbe` started with lenses risk/resilience/reliability; all 3 reviewer captures failed: "Claude Code can't use your Claude subscription, usually because a payment is past due" (same as the MVP lineage). STATUS reoffers the 3 slots; not retried (same cause). Review pending; nothing committed.
- L8 (2026-11-23, user): "Sí, funciona" → gear settings dialog and folder links confirmed (T8, T9).
