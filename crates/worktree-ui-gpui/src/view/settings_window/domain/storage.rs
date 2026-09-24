//! Storage category: what the on-disk history cache holds, and the one button
//! that matters — dropping it.
//!
//! The cache is reached through `worktree_core::history_cache`, never directly:
//! the gix backend is an optional dependency of this crate, and without it every
//! accessor returns `None` (no card rows at all).

use super::*;
use gpui::Stateful;

impl SettingsWindowView {
    pub(in crate::view::settings_window) fn storage_card(
        &self,
        theme: AppTheme,
        no_separator: gpui::Rgba,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let card = self.card(
            "settings_window_storage",
            tr_str("settings.nav.storage"),
            theme,
        );

        // Nothing registered a cache in this build: say so rather than showing
        // a path and a size that belong to nothing.
        let Some(dir) = worktree_core::history_cache::history_cache_dir() else {
            return card.child(
                div()
                    .id("settings_window_storage_unavailable")
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .text_color(theme.colors.foreground.secondary)
                    .child(tr_str("settings.storage.unavailable")),
            );
        };

        let (bytes, entries) =
            worktree_core::history_cache::history_cache_usage().unwrap_or((0, 0));

        card.child(self.info_row(
            "settings_window_storage_path",
            tr_str("settings.storage.path"),
            dir.display().to_string().into(),
            theme,
        ))
        .child(self.info_row(
            "settings_window_storage_size",
            tr_str("settings.storage.size"),
            human_bytes(bytes).into(),
            theme,
        ))
        .child(
            self.info_row(
                "settings_window_storage_entries",
                tr_str("settings.storage.entries"),
                entries.to_string().into(),
                theme,
            )
            .border_color(no_separator),
        )
        .child(
            div()
                .id("settings_window_storage_clear_row")
                .px_2()
                .pt_1()
                .pb_2()
                .flex()
                .child(
                    components::Button::new(
                        "settings_window_storage_clear",
                        tr("settings.storage.clear"),
                    )
                    .style(components::ButtonStyle::Outlined)
                    .on_click(theme, cx, |_this, _event, _window, cx| {
                        let _ = worktree_core::history_cache::clear_history_cache();
                        // Rows are read live from the cache hooks, so a repaint
                        // is all it takes to show the emptied numbers.
                        cx.notify();
                    }),
                ),
        )
    }
}

/// `1_048_576 -> "1.0 MiB"`. Binary units, because that is what the byte
/// ceilings in the cache itself are written in.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::human_bytes;

    #[test]
    fn human_bytes_stays_whole_below_a_kibibyte() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
    }

    #[test]
    fn human_bytes_uses_binary_units() {
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes(256 * 1024 * 1024), "256.0 MiB");
    }
}
