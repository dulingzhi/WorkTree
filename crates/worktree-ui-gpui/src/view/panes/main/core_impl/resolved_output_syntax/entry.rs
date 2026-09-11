//! Row measurement and resolver input construction.

use crate::kit::text_model::TextModelSnapshot;
use crate::ui_scale;
use crate::view::components;
use crate::view::panes::main::helpers::ResolvedOutputSourceRevision;
use crate::view::panes::main::helpers::coalesce_resolved_output_edit_deltas;
use crate::view::panes::main::helpers::resolved_outline_delta_for_snapshot_transition;
use crate::view::panes::main::helpers::resolved_output_snapshot_is_modified;
use crate::view::panes::main::state::MainPaneView;
use gpui::AppContext;
use gpui::Entity;
use gpui::Window;
/// The row the resolved-output column measures its width against.
///
/// O(1): the rope carries the widest row in its summary, so the measurement
/// never scans the document. Ties keep the earliest row, matching the linear
/// scan this replaced.
pub(in crate::view::panes::main::core_impl) fn resolved_output_measure_row(
    snapshot: &TextModelSnapshot,
) -> usize {
    snapshot.rope().longest_row() as usize
}

/// Create the resolved-output editor input and its edit-observe
/// subscription: the pane-wide keystroke path for this domain. Called from
/// `MainPaneView::new` in the original input-creation order.
pub(in crate::view::panes::main::core_impl) fn new_conflict_resolver_input(
    window: &mut Window,
    cx: &mut gpui::Context<MainPaneView>,
) -> (Entity<components::TextInput>, gpui::Subscription) {
    let conflict_resolver_input = cx.new(|cx| {
        let mut input = components::TextInput::new(
            components::TextInputOptions {
                placeholder: "Resolve file contents…".into(),
                multiline: true,
                chromeless: true,
                ..Default::default()
            },
            window,
            cx,
        );
        input.set_suppress_right_click(true);
        input.set_line_height(
            Some(ui_scale::design_px_from_percent(
                20.0,
                ui_scale::current(cx).percent,
            )),
            cx,
        );
        input
    });

    let conflict_resolver_subscription = cx.observe(&conflict_resolver_input, |this, input, cx| {
        let _perf_scope =
            crate::view::perf::span(crate::view::perf::ViewPerfSpan::ResolvedOutputEditObserve);
        let (output_snapshot, edit_deltas) = input.update(cx, |input, _| {
            (input.text_snapshot(), input.drain_recent_utf8_edit_deltas())
        });
        let outline_edit_delta = (edit_deltas.len() == 1)
            .then(|| edit_deltas.first().cloned())
            .flatten();
        // Fold the tree forward before anything else looks at the
        // buffer, so the very next frame paints from a tree that
        // already describes what was just typed.
        let syntax_edit = coalesce_resolved_output_edit_deltas(&edit_deltas);
        this.apply_conflict_resolved_output_edit_deltas(edit_deltas, &output_snapshot.rope());
        if !this.conflict_resolved_output_is_streamed() {
            this.refresh_conflict_resolved_output_syntax(&output_snapshot, syntax_edit, cx);
        }
        let source_revision = ResolvedOutputSourceRevision::from_snapshot(&output_snapshot);
        let output_modified = resolved_output_snapshot_is_modified(
            this.conflict_resolved_output_saved_snapshot.as_ref(),
            &output_snapshot,
        );
        if this.conflict_resolved_output_modified != output_modified {
            this.conflict_resolved_output_modified = output_modified;
            cx.notify();
        }
        let outline_delta = resolved_outline_delta_for_snapshot_transition(
            &this.conflict_resolved_preview_text,
            &output_snapshot,
            outline_edit_delta,
        );

        let path = this.conflict_resolver.path.clone();
        let needs_update = this.conflict_resolved_preview_path.as_ref() != path.as_ref()
            || this.conflict_resolved_preview_source_revision != Some(source_revision);
        if !needs_update {
            return;
        }

        this.conflict_resolved_preview_path = path.clone();
        this.conflict_resolved_preview_source_revision = Some(source_revision);
        this.schedule_conflict_resolved_outline_recompute(path, source_revision, outline_delta, cx);
        // The Save gates derive effective resolutions from the live
        // editor text, so the containing toolbar must re-render for
        // every edit even while session state remains deferred.
        cx.notify();
    });
    (conflict_resolver_input, conflict_resolver_subscription)
}
