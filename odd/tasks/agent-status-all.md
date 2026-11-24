# agent-status-all

Objective: live Working / Waiting / Done status for every agent CLI, not only Claude Code. Branch `feat/agent-status-all` (from main f6f32f0). Delivery: commits only when Rafael asks. Test runner: `cargo test --workspace --offline`, `cargo clippy --workspace --all-targets --offline`.

## Specs

- S1: "Haz la 1. era lo principal pensaba que tambien lo habias hecho" — item 1 of the proposal: "Estado en vivo para todos los agentes. Hoy solo Claude muestra Working / Waiting / Done. Se podría extender a Codex, Gemini y Antigravity si sus CLIs ofrecen hooks o eventos".
- S3 (see L5): "me gustaria que elimine todo referido a GEMINI CLI porque no usare eso, el de antigravity si dejalo eso si usare ahora".
- S2 (L2): user approved "Yes, code + configs (Recommended)": register hooks (backups, append only) in ~/.codex/hooks.json, ~/.gemini/config/hooks.json (new, Antigravity), ~/.gemini/settings.json, ~/.qwen/settings.json, a new ~/.config/opencode/plugins/my-terminal.ts, and reinstall my-terminal-hook with cargo install.

## Tasks

| ID | Specs | Route | Status | Evidence |
|---|---|---|---|---|
| T1 | S1 | inline | done in code | `HookKind::PermissionRequest`→Waiting, `Interrupt`/`StopFailure`→Done (RED observed, 1 test) |
| T2 | S1 | inline | done in code | `agent_status::adapt` (`Source::from_arg`, `to_claude` gemini renames + agy camelCase reshape, `reply` agy `{}`/Stop `{"decision":"allow"}`) RED observed, 5 tests; `my-terminal-hook --agent`: reply printed first (also outside my-terminal), then reshaped payload forwarded; 3 new binary tests (RED observed) |
| T3 | S1 | inline | done in code | pane title falls back to the running agent name; agent status dropped when no agent process runs in the pane (agy has no SessionEnd) |
| T4 | S1 | inline | done in code | `integrations/opencode/my-terminal.ts` (session.created/status busy|idle/permission.asked/question.asked/session.idle/error/deleted; child sessions skipped; Bun.spawn my-terminal-hook); `bun build` ok; run against a fake socket delivered SessionStart, PreToolUse, Notification and skipped the child session |
| T6 | S3 | inline | done in code | Gemini CLI removed: agent bar entry + `googlegemini.svg` + icon README row, `adapt::Source::Gemini` and its tests (`--agent gemini` now reads as Claude-shaped), README rows; RED observed (`gemini_cli_is_not_an_agent`); my 6 hook groups removed from ~/.gemini/settings.json (user's AfterTool/SessionStart kept, backup in a new dated dir under ~/.my-terminal-backups, mode 664 kept); hook reinstalled; 200 tests, clippy 0. Antigravity untouched |
| T5 | S2 | inline | done; live agents not exercised | backups in `~/.my-terminal-backups/20261124-020434/` (dotfile names); hooks appended (existing kept) in ~/.codex/hooks.json (7 events), ~/.gemini/settings.json (6, `--agent gemini`), ~/.qwen/settings.json (8), new ~/.gemini/config/hooks.json (`my-terminal`: 4 events, `--agent agy`); modes restored to 0600 for codex/qwen/agy files; plugin copied to ~/.config/opencode/plugins/; `cargo install --path crates/agent-status` (manual `--agent agy` Stop → `{"decision":"allow"}`, exit 0) |

Route evidence: research delegated to one read-only explorer (5 CLIs, many lookups); implementation inline (one crate + small app changes).

## Log

- L1 (2026-11-24, user): "Haz la 1. era lo principal pensaba que tambien lo habias hecho"
- L2 (2026-11-24): explorer report: Codex 0.171 `~/.codex/hooks.json` (Claude shape, PermissionRequest/Interrupt, hooks must be trusted via `/hooks`); Gemini 0.70 `settings.json` hooks (BeforeAgent/BeforeTool/AfterTool/AfterAgent/Notification, env inherited); agy 1.5.16 `~/.gemini/config/hooks.json` keyed by hook name, camelCase stdin, Stop needs `decision` (anything but "continue" stops — https://antigravity.google/docs/hooks.md); Qwen 0.28 Claude-compatible `settings.json` hooks; OpenCode 1.19.6 plugin events (session.status busy, permission.asked, question.asked, session.idle, session.deleted). User approved code + configs.
- L3 (2026-11-24): agy hooks doc (https://antigravity.google/docs/hooks.md): Stop requires `decision`, anything but "continue" lets the agent stop; PreToolUse decision required → not hooked. Race test made robust (another test forking can briefly hold the previous round's lock; asserts never two primaries, >150/200 wins). Checks: 199 tests ×5 green, clippy 0.
- L4 (2026-11-24): independent verifier (risk item 5: agent configs edited). Blocker B1: agy 1.5.16 reads SessionStart/PreInvocation/PostInvocation/Stop as flat handler lists; my grouped entries made `agy plugin validate` skip the whole file ("command hook must specify 'command'"). Fixed: flat entries (PostToolUse keeps its group), mode 0600; recheck: `agy plugin validate` on a scratch plugin with the exact file → "✔ Validation passed", hooks 1. Also fixed: status dropped only when the agent seen in the pane exits (not on every refresh without one); OpenCode plugin falls back to ~/.cargo/bin/my-terminal-hook; backup dir 0700; backup path corrected to 020434; README agy snippet; 2 regression tests from the verifier (non-agy never prints, agy replies to malformed input). Codex/Gemini/Qwen/OpenCode checked statically against their parsers; live sessions not run.
- L5 (2026-11-24, user): "Estos lo probare despues, me gustaria que elimine todo referido a GEMINI CLI porque no usare eso, el de antigravity si dejalo eso si usare ahora a que te refieres con esta tarea? 2. Sesiones en git worktree." → T6. Not touched (not requested explicitly, ask first): MCP/gentle-ai entries earlier added to ~/.gemini/settings.json for Gemini CLI.
