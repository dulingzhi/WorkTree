use gpui::BorrowAppContext;
use worktree_state::session;

/// Row-rhythm tiers for the main lists. `Comfortable` is the default;
/// `Compact` keeps the pre-density dense rhythm for large-repository
/// scanning (the few 22px stragglers unify onto the shared 24px row so the
/// list reads evenly in both tiers). Density composes with (multiplies
/// through) the percentage UI scale — it only adjusts rhythm metrics, never
/// font sizes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "comfortable" => Some(Density::Comfortable),
            "compact" => Some(Density::Compact),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AppUiDensity {
    pub(crate) density: Density,
    pub(crate) initialized: bool,
}

impl Default for AppUiDensity {
    fn default() -> Self {
        Self {
            density: Density::Comfortable,
            initialized: false,
        }
    }
}

impl gpui::Global for AppUiDensity {}

pub(crate) fn current<C>(cx: &mut C) -> AppUiDensity
where
    C: BorrowAppContext,
{
    cx.update_default_global::<AppUiDensity, _>(|density, _cx| *density)
}

pub(crate) fn current_or_initialize_from_session<C>(
    ui_session: &session::UiSession,
    cx: &mut C,
) -> AppUiDensity
where
    C: BorrowAppContext,
{
    let current = current(cx);
    if current.initialized {
        return current;
    }

    let next = AppUiDensity {
        density: sanitize(ui_session.ui_density.as_deref()),
        initialized: true,
    };
    cx.set_global(next);
    next
}

pub(crate) fn set_current<C>(cx: &mut C, density: Density) -> AppUiDensity
where
    C: BorrowAppContext,
{
    let next = AppUiDensity {
        density,
        initialized: true,
    };
    cx.set_global(next);
    next
}

/// Unknown or missing keys fall back to the default tier, so a future tier
/// added to a session file still loads on this build.
pub(crate) fn sanitize(key: Option<&str>) -> Density {
    key.and_then(Density::from_key)
        .unwrap_or(Density::Comfortable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_keys_round_trip() {
        for density in [Density::Comfortable, Density::Compact] {
            assert_eq!(Density::from_key(density.key()), Some(density));
        }
        assert_eq!(Density::from_key("cozy"), None);
    }

    #[test]
    fn density_sanitize_falls_back_to_comfortable() {
        assert_eq!(sanitize(None), Density::Comfortable);
        assert_eq!(sanitize(Some("compact")), Density::Compact);
        assert_eq!(sanitize(Some("spacious")), Density::Comfortable);
    }
}
