//! Workspace actions and their default key bindings.
//!
//! Bindings use Ctrl+Shift so plain Ctrl combinations keep reaching the shell
//! (Ctrl+C, Ctrl+D, Ctrl+R...).

use gpui::{App, KeyBinding, actions};

actions!(
    workspace,
    [
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        SplitRight,
        ClosePane,
        FocusNextPane,
        FocusPrevPane,
        ToggleSidebar,
        SwitchAccount,
        FocusSearch,
        FindInTerminal,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ToggleGitPanel,
    ]
);

/// Key context of the workspace root.
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// Key context of the accounts dialog.
pub const ACCOUNT_MODAL_CONTEXT: &str = "AccountModal";
/// Key context of the settings dialog.
pub const SETTINGS_MODAL_CONTEXT: &str = "SettingsModal";
/// Shortcuts apply in the workspace but not while the accounts dialog has
/// focus: they would move focus to a terminal hidden behind it.
const SHORTCUT_CONTEXT: &str = "Workspace && !AccountModal && !SettingsModal";

/// Registers the default shortcuts.
pub fn bind_default_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-t", NewTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-q", CloseTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-tab", NextTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-pagedown", NextTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-pageup", PrevTab, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-d", SplitRight, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-w", ClosePane, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-right", FocusNextPane, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-left", FocusPrevPane, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-b", ToggleSidebar, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-a", SwitchAccount, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-k", FocusSearch, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-f", FindInTerminal, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-=", ZoomIn, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-+", ZoomIn, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl--", ZoomOut, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-0", ZoomReset, Some(SHORTCUT_CONTEXT)),
        KeyBinding::new("ctrl-shift-g", ToggleGitPanel, Some(SHORTCUT_CONTEXT)),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyBindingContextPredicate, KeyContext};

    fn matches(stack: &[&str]) -> bool {
        let predicate = KeyBindingContextPredicate::parse(SHORTCUT_CONTEXT).unwrap();
        let stack: Vec<KeyContext> = stack
            .iter()
            .map(|name| KeyContext::parse(name).unwrap())
            .collect();
        // gpui tries every depth of the focused element's context stack.
        (1..=stack.len()).any(|depth| predicate.eval_inner(&stack[..depth], &stack))
    }

    #[test]
    fn shortcuts_work_in_the_workspace() {
        assert!(matches(&["Workspace"]));
    }

    #[test]
    fn shortcuts_are_off_while_a_dialog_has_focus() {
        assert!(!matches(&["Workspace", "AccountModal"]));
        assert!(!matches(&[WORKSPACE_CONTEXT, SETTINGS_MODAL_CONTEXT]));
        assert!(!matches(&[]));
    }
}
