# my-terminal

A lightweight Rust workspace for Claude Code sessions: a real terminal with
tabs, splits, an agent sidebar and one-click account switching.

## Layout

```
crates/
  term-core/   Headless terminal sessions: PTY + alacritty_terminal grid. No UI.
  agent-status/ Claude Code hook events, agent state, socket IPC.
               Binary `my-terminal-hook` (the hook command).
  app/         gpui desktop app (binary `my-terminal`): window, sidebar, tabs.
vendor/        Patched third-party crates; see vendor/README.md.
odd/tasks/     Feature specs and task log.
```

`term-core` owns everything that can be tested without a window. The UI crate
only renders the grid and forwards input, so most behavior stays covered by
fast headless tests.

## Requirements

- Rust 1.85+ (edition 2024)
- For the UI crate (Ubuntu):

  ```sh
  sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
    libfontconfig-dev libxcb1-dev libx11-xcb-dev libssl-dev clang
  ```

## Run

```sh
cargo run --release -p app
```

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+T` | New tab (opens in the current directory) |
| `Ctrl+Shift+Q` | Close tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab (also `Ctrl+PgDn` / `Ctrl+PgUp`) |
| `Ctrl+Shift+D` | Split right |
| `Ctrl+Shift+W` | Close pane |
| `Ctrl+Shift+Right` / `Left` | Focus next / previous pane |
| `Ctrl+Shift+B` | Show / hide the agent sidebar |
| `Ctrl+Shift+A` | Switch the Claude account used by new panes |
| `Ctrl+Shift+F` | Search sessions in the sidebar (Enter opens the first, Esc clears) |
| `Ctrl+Shift+C` | Copy the selection |
| `Ctrl+Shift+V` | Paste (bracketed when the program asks) |

Mouse: drag to select (double-click selects a word, triple-click a line); the
selection also goes to the primary selection (middle-click paste in other apps). Typing
clears it. The wheel or touchpad scrolls the history; in full-screen programs
it sends arrow keys, or wheel reports when the program tracks the mouse.

Sessions are listed in the sidebar only; the title bar has no tabs. Splits
are vertical: panes sit side by side, and every session shown in the split is
highlighted in the sidebar. Right-click a session in the sidebar:

- **Add to split** (a session not on screen): moves it next to the focused
  pane.
- **Remove from split** (a session in a split of two or more): moves it to
  its own tab; it keeps running and stays in the sidebar.

The buttons at the right of the title bar split the focused pane to the right
(it gives the new pane half its width) and, in a split, remove the focused
pane from it. `Ctrl+Shift+W` still closes the focused pane and ends its
session. Drag the line between two panes to resize them; no pane gets
narrower than a tenth of the window. Click a pane to focus it; with several
panes, a blue line marks the focused one.

Drag the empty title bar to move the window; double-click it to maximize.

## Agent bar

The row of icons at the bottom of the sidebar shows Claude Code, Codex, Gemini
CLI, Qwen Code, OpenCode and Antigravity CLI (`agy`). A green dot means the CLI has saved credentials
(a gray dot: signed out); a dimmed icon means it is not on `PATH`.

- Click: start the agent in a new tab, or run its sign-in when it is signed
  out. On Claude Code, open the accounts dialog (see below). On a dimmed
  icon, open its install page in the browser.
- Right-click: menu with **Open**, **Sign in** (useful when saved credentials
  expired) and **Install or repair**, which opens the official install page.

my-terminal never reads or stores agent credentials: it runs each CLI's own
status command (`claude auth status`, `codex login status`,
`opencode auth list`) or checks that its credentials file exists
(`~/.gemini/oauth_creds.json`, `~/.qwen/oauth_creds.json`). Antigravity CLI
has no status command, so its icon shows no dot; it signs in during its first run. The states are
checked at start, every 5 minutes, and whenever a tab closes.

A Claude Code session is titled like in Claude's own session list: the latest
title Claude wrote to its transcript (`custom-title`), or its first prompt
until there is one. That title is also what saved sessions show.

In the session list, a pane running one of these agents shows its logo and
name (found by looking at the processes under the pane's shell every 2
seconds), in the brand color. Claude Code panes add a status dot on the logo:
working (blue), waiting (yellow), done (green). The search box above the
filters keeps the sessions whose title, activity or project contain every word.

## Saved sessions

Agent sessions (Claude Code, Codex, Gemini, ...) are saved whenever they
change, to `$XDG_STATE_HOME/my-terminal/sessions.json` (default
`~/.local/state/my-terminal/`, file mode 0600): title, folder, agent, account
and, for Claude Code, its session id. Plain shells are not saved.

At the next start they show in the sidebar dimmed, as **Saved · click to
resume**, and nothing runs until you click one: then a tab opens in its
folder with its account and runs `claude --resume <id>` (Claude Code gets its
whole conversation back from its own history) or starts the other agent
fresh. A shell's screen and running commands cannot come back.
Right-click a saved session and choose **Forget session** to drop it.

Click a project name in the sidebar to collapse or expand its group; the
choice is saved too. Searching shows matches in collapsed groups as well.

When my-terminal itself is started from inside a Claude Code session, the
shells it opens do not inherit that session's `CLAUDECODE` and `CLAUDE_*`
variables (except `CLAUDE_CONFIG_DIR` and `CLAUDE_CODE_OAUTH_TOKEN`), so
Claude Code in them saves its history normally.

## Claude accounts

Click the Claude Code icon in the agent bar to open the accounts dialog (its
tooltip names the active account). New panes start with the active account;
running panes keep theirs. `Ctrl+Shift+A` cycles through the accounts without
opening the dialog.

- **Open** (or `Enter` for the active one): start Claude in a new tab with
  that account, which also becomes the active account.
- **Default** uses the login stored by `claude` itself (`/login`); its
  **Sign in** runs `claude auth login` in a new tab.
- **Add account**: enter a name, click **Open claude setup-token** (a tab runs
  it on the default login), sign in with the other account in the browser,
  copy the printed token (select it, `Ctrl+Shift+C`), click the Claude icon again and
  paste it with `Ctrl+V`. The token field only shows dots.
- **Rename** / **Remove** act on the token file; removing never touches panes
  that are already running.

Each extra account is a file `<name>.token` (mode 0600) in
`$XDG_CONFIG_HOME/my-terminal/accounts` (default `~/.config/...`, mode 0700).
Panes started with it get the token as `CLAUDE_CODE_OAUTH_TOKEN`. Files that
group or others can read are ignored.

## Agent sidebar

Every pane exports `MY_TERMINAL_PANE` and `MY_TERMINAL_SOCKET`. Claude Code
hooks call `my-terminal-hook`, which forwards the event to the app over a Unix
socket in `$XDG_RUNTIME_DIR` (mode 0700). Outside my-terminal the hook does
nothing; it always exits 0 and prints nothing.

Register it for these events in `~/.claude/settings.json` (append to existing
hooks, do not replace them):

```json
{
  "hooks": {
    "SessionStart":     [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }],
    "PreToolUse":       [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }],
    "Notification":     [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }],
    "SessionEnd":       [{ "hooks": [{ "type": "command", "command": "my-terminal-hook" }] }]
  }
}
```

Install the binary with `cargo install --path crates/agent-status`.

## Development

```sh
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

## Known limitations

- Not yet: text selection, mouse-wheel scrollback, resizing
  the window from its edges (see `odd/tasks/terminal-mvp.md`).

- A program that prints and exits within milliseconds can lose its last
  output on Linux: the PTY reports `EIO` before the kernel flushes its
  buffer. Long-running programs (shells, `claude`) are not affected.
