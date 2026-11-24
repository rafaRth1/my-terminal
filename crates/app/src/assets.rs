//! Files embedded in the binary and served to gpui by path.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Path and bytes of every embedded file.
const FILES: [(&str, &[u8]); 8] = [
    (
        "icons/claude.svg",
        include_bytes!("../assets/icons/claude.svg"),
    ),
    (
        "icons/openai.svg",
        include_bytes!("../assets/icons/openai.svg"),
    ),
    (
        "icons/googlegemini.svg",
        include_bytes!("../assets/icons/googlegemini.svg"),
    ),
    ("icons/qwen.svg", include_bytes!("../assets/icons/qwen.svg")),
    (
        "icons/opencode.svg",
        include_bytes!("../assets/icons/opencode.svg"),
    ),
    (
        "icons/antigravity.svg",
        include_bytes!("../assets/icons/antigravity.svg"),
    ),
    (
        "ui/split-right.svg",
        include_bytes!("../assets/ui/split-right.svg"),
    ),
    ("ui/unsplit.svg", include_bytes!("../assets/ui/unsplit.svg")),
];

/// Serves [`FILES`]; registered with `Application::with_assets`.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::AGENTS;

    #[test]
    fn every_agent_icon_is_an_svg() {
        for agent in &AGENTS {
            let bytes = Assets.load(agent.icon).unwrap().expect(agent.icon);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg"), "{}", agent.icon);
            assert!(!text.contains("<script"), "{}", agent.icon);
        }
    }

    #[test]
    fn ui_icons_are_svgs() {
        for path in ["ui/split-right.svg", "ui/unsplit.svg"] {
            let bytes = Assets.load(path).unwrap().expect(path);
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.starts_with("<svg"), "{path}");
            assert!(!text.contains("<script"), "{path}");
        }
    }

    #[test]
    fn unknown_paths_load_nothing() {
        assert!(Assets.load("icons/missing.svg").unwrap().is_none());
    }

    #[test]
    fn list_filters_by_prefix() {
        let icons = Assets.list("icons/").unwrap();
        assert_eq!(icons.len(), AGENTS.len());
        assert_eq!(Assets.list("ui/").unwrap().len(), 2);
        assert_eq!(Assets.list("other/").unwrap(), Vec::<SharedString>::new());
    }
}
