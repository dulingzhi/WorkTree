//! Tests for the settings window, split by domain.
//!
//! Shared helpers live here; each domain's tests are a child module that pulls
//! this module's namespace — and through it the facade's — with
//! `use super::*`.

pub(super) use super::*;
use crate::test_support::lock_visual_test;
use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use worktree_core::error::{Error, ErrorKind};
use worktree_core::process::{GitExecutableAvailability, GitExecutablePreference, GitRuntimeState};
use worktree_core::services::{GitBackend, GitRepository, Result};

const SESSION_FILE_ENV: &str = "WORKTREE_SESSION_FILE";

const DIFF_DEFAULTS_SESSION_SUBTEST_ENV: &str = "WORKTREE_DIFF_DEFAULTS_SESSION_SUBTEST";

struct TestBackend;

impl GitBackend for TestBackend {
    fn open(&self, _workdir: &Path) -> Result<std::sync::Arc<dyn GitRepository>> {
        Err(Error::new(ErrorKind::Unsupported(
            "Test backend does not open repositories",
        )))
    }
}

fn unique_session_file(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "worktree-settings-window-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create settings session temp dir");
    dir.join("session.json")
}

fn run_subtest_with_session_env(filter: &str, session_file: &Path) {
    let current_exe = std::env::current_exe().expect("locate current test binary");
    let output = Command::new(current_exe)
        .arg(filter)
        .arg("--nocapture")
        .env(SESSION_FILE_ENV, session_file)
        .env(DIFF_DEFAULTS_SESSION_SUBTEST_ENV, "1")
        .output()
        .expect("spawn settings subtest process");
    assert!(
        output.status.success(),
        "subtest {filter} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_debug_bounds_within(
    cx: &mut gpui::VisualTestContext,
    outer_selector: &'static str,
    inner_selector: &'static str,
) {
    let outer_bounds = cx
        .debug_bounds(outer_selector)
        .unwrap_or_else(|| panic!("expected `{outer_selector}` bounds"));
    let inner_bounds = cx
        .debug_bounds(inner_selector)
        .unwrap_or_else(|| panic!("expected `{inner_selector}` bounds"));
    let tolerance = px(0.5);

    assert!(
        inner_bounds.left() >= outer_bounds.left() - tolerance
            && inner_bounds.right() <= outer_bounds.right() + tolerance
            && inner_bounds.top() >= outer_bounds.top() - tolerance
            && inner_bounds.bottom() <= outer_bounds.bottom() + tolerance,
        "expected `{inner_selector}` to stay within `{outer_selector}` \
         (outer={outer_bounds:?}, inner={inner_bounds:?})"
    );
}

fn assert_debug_matching_horizontal_insets(
    cx: &mut gpui::VisualTestContext,
    outer_selector: &'static str,
    inner_selector: &'static str,
) {
    let outer_bounds = cx
        .debug_bounds(outer_selector)
        .unwrap_or_else(|| panic!("expected `{outer_selector}` bounds"));
    let inner_bounds = cx
        .debug_bounds(inner_selector)
        .unwrap_or_else(|| panic!("expected `{inner_selector}` bounds"));
    let left_inset = inner_bounds.left() - outer_bounds.left();
    let right_inset = outer_bounds.right() - inner_bounds.right();
    let tolerance = px(1.0);

    assert!(
        (left_inset - right_inset).abs() <= tolerance,
        "expected `{inner_selector}` to use the full horizontal content width inside \
         `{outer_selector}` (left inset={left_inset:?}, right inset={right_inset:?}, \
         outer={outer_bounds:?}, inner={inner_bounds:?})"
    );
}

/// Shared driver for dropdown wheel tests: wheel straight down over the
/// list must move the list while it can still scroll and hand off to the
/// page scroller only at the list's boundary.
fn assert_dropdown_wheel_stops_at_list(
    cx: &mut gpui::TestAppContext,
    list_container: &'static str,
    window_height_px: f32,
    scroll_of: impl Fn(&SettingsWindowView) -> &UniformListScrollHandle,
    setup: impl FnOnce(&mut SettingsWindowView, &mut gpui::Context<SettingsWindowView>),
) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            setup(settings, cx);
            settings.settings_window_scroll = ScrollHandle::default();
            cx.notify();
        });
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(
        px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX),
        px(window_height_px),
    ));
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let mut list_bounds = settings_cx
        .debug_bounds(list_container)
        .unwrap_or_else(|| panic!("expected `{list_container}` bounds"));

    // A wheel only reaches hitboxes inside the viewport; lists deep in a
    // tall card (AI source, merge tool) start below the fold of the short
    // test window, so page-scroll them into view first.
    if list_bounds.bottom() > px(window_height_px) {
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
            let needed = (list_bounds.top() - px(40.0)).max(px(0.0)).min(max_offset);
            settings
                .settings_window_scroll
                .set_offset(point(px(0.0), -needed));
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });
        list_bounds = settings_cx
            .debug_bounds(list_container)
            .expect("expected list bounds after scrolling into view");
        assert!(
            list_bounds.bottom() <= px(window_height_px) + px(0.5),
            "the test window is too short to show the dropdown list even after scrolling"
        );
    }

    let (outer_before, inner_before, outer_max, inner_max) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).1,
                settings.settings_window_scroll.max_offset().y.max(px(0.0)),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).2,
            )
        })
        .expect("settings window should remain readable");
    assert!(
        outer_max > px(0.0),
        "expected the settings page to be scrollable during the test"
    );
    assert!(
        inner_max > px(0.0),
        "expected the dropdown list to be scrollable during the test"
    );

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let (outer_after, inner_after) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).1,
            )
        })
        .expect("settings window should remain readable");
    assert!(
        inner_after > inner_before + px(0.5),
        "expected the dropdown list to consume wheel scroll first (inner {inner_before:?} -> {inner_after:?})"
    );
    assert!(
        (outer_after - outer_before).abs() <= px(0.5),
        "expected the outer settings page to stay still while the list can still scroll (outer {outer_before:?} -> {outer_after:?})"
    );

    // Jump the list to its boundary; the same wheel must now chain to the
    // page behind it.
    let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
        let (raw_offset, _scroll_offset, max_offset) =
            uniform_list_vertical_scroll_metrics(scroll_of(settings));
        let current_x = scroll_of(settings).0.borrow().base_handle.offset().x;
        let target_y = if raw_offset > px(0.0) {
            max_offset
        } else {
            -max_offset
        };
        scroll_of(settings)
            .0
            .borrow()
            .base_handle
            .set_offset(point(current_x, target_y));
        cx.notify();
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let outer_after_boundary_handoff = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            absolute_scroll_y(&settings.settings_window_scroll)
        })
        .expect("settings window should remain readable");
    assert!(
        outer_after_boundary_handoff > outer_after + px(0.5),
        "expected wheel scrolling to bubble to the outer settings page once the list reaches its boundary"
    );
}

/// The chain-through counterpart for lists too short to scroll: the wheel
/// must pass straight to the page scroller.
fn assert_dropdown_wheel_chains_to_outer_page_when_list_cannot_scroll(
    cx: &mut gpui::TestAppContext,
    list_container: &'static str,
    window_height_px: f32,
    scroll_of: impl Fn(&SettingsWindowView) -> &UniformListScrollHandle,
    setup: impl FnOnce(&mut SettingsWindowView, &mut gpui::Context<SettingsWindowView>),
) {
    let _visual_guard = lock_visual_test();
    let (store, events) = AppStore::new(std::sync::Arc::new(TestBackend));
    let (_main_view, cx) =
        cx.add_window_view(|window, cx| WorkTreeView::new(store, events, None, window, cx));

    cx.update(|window, app| {
        let _ = window.draw(app);
        open_settings_window(app);
    });
    cx.run_until_parked();

    let settings_window = cx.update(|_window, app| {
        app.windows()
            .into_iter()
            .find_map(|window| window.downcast::<SettingsWindowView>())
            .expect("settings window should be open")
    });

    cx.update(|_window, app| {
        let _ = settings_window.update(app, |settings, _window, cx| {
            setup(settings, cx);
            settings.settings_window_scroll = ScrollHandle::default();
            cx.notify();
        });
    });

    let mut settings_cx = gpui::VisualTestContext::from_window(*settings_window.deref(), cx);
    settings_cx.run_until_parked();
    settings_cx.simulate_resize(size(
        px(SETTINGS_WINDOW_DEFAULT_WIDTH_PX),
        px(window_height_px),
    ));
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let mut list_bounds = settings_cx
        .debug_bounds(list_container)
        .unwrap_or_else(|| panic!("expected `{list_container}` bounds"));

    // A wheel only reaches hitboxes inside the viewport; page-scroll the
    // list into view before wheeling over it (see the stops-at-list
    // variant above).
    if list_bounds.bottom() > px(window_height_px) {
        let _ = settings_window.update(&mut settings_cx, |settings, _window, cx| {
            let max_offset = settings.settings_window_scroll.max_offset().y.max(px(0.0));
            let needed = (list_bounds.top() - px(40.0)).max(px(0.0)).min(max_offset);
            settings
                .settings_window_scroll
                .set_offset(point(px(0.0), -needed));
            cx.notify();
        });
        settings_cx.run_until_parked();
        settings_cx.update(|window, app| {
            let _ = window.draw(app);
        });
        list_bounds = settings_cx
            .debug_bounds(list_container)
            .expect("expected list bounds after scrolling into view");
        assert!(
            list_bounds.bottom() <= px(window_height_px) + px(0.5),
            "the test window is too short to show the dropdown list even after scrolling"
        );
    }

    let (outer_before, inner_before, outer_max, inner_max) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).1,
                settings.settings_window_scroll.max_offset().y.max(px(0.0)),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).2,
            )
        })
        .expect("settings window should remain readable");
    assert!(
        inner_max <= px(0.0),
        "expected the dropdown list to be too short to scroll"
    );
    assert!(
        outer_max > px(0.0),
        "expected the settings page to be scrollable during the test"
    );

    settings_cx.simulate_mouse_move(list_bounds.center(), None, Modifiers::default());
    settings_cx.simulate_event(ScrollWheelEvent {
        position: list_bounds.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        ..Default::default()
    });
    settings_cx.run_until_parked();
    settings_cx.update(|window, app| {
        let _ = window.draw(app);
    });

    let (outer_after, inner_after) = settings_window
        .update(&mut settings_cx, |settings, _window, _cx| {
            (
                absolute_scroll_y(&settings.settings_window_scroll),
                uniform_list_vertical_scroll_metrics(scroll_of(settings)).1,
            )
        })
        .expect("settings window should remain readable");
    assert!(
        outer_after > outer_before + px(0.5),
        "expected the wheel to chain through a list that cannot scroll to the page"
    );
    assert!(
        (inner_after - inner_before).abs() <= px(0.5),
        "expected the unscrollable list to stay put"
    );
}

mod ai_commit;
mod diff;
mod external_editor;
mod general;
mod git_log;
mod git_runtime;
mod gpg_signing;
mod links;
mod merge_tool;
mod tags;
mod terminal;
mod window_chrome;
