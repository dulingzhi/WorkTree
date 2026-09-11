use super::fingerprint::{shared_bytes_fingerprint, shared_text_fingerprint};
use super::*;

fn session_with_automatic_delta() -> worktree_core::conflict_session::ConflictSession {
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;

    ConflictSession::from_stage_inputs(
        std::path::PathBuf::from("file.txt"),
        FileConflictKind::BothModified,
        ConflictPayload::Text("start\nold-local\nmiddle\nold-conflict\nend\n".into()),
        ConflictPayload::Text("start\nnew-local\nmiddle\nours-conflict\nend\n".into()),
        ConflictPayload::Text("start\nold-local\nmiddle\ntheirs-conflict\nend\n".into()),
    )
}

#[test]
fn live_plan_projection_renders_an_automatic_delta_override() {
    use worktree_core::merge::MergeSource;

    let mut session = session_with_automatic_delta();
    let automatic_id = session
        .merge_plan
        .as_ref()
        .unwrap()
        .blocks
        .iter()
        .find(|block| block.is_delta && !block.original_conflict)
        .unwrap()
        .id;
    let (automatic, _) = conflict_session_plan_projection(&session).unwrap();
    assert!(automatic.contains("new-local\n"));

    assert!(session.replace_plan_block_selection(automatic_id, MergeSource::C.into()));
    let (overridden, _) = conflict_session_plan_projection(&session).unwrap();
    assert!(overridden.contains("old-local\n"));
    assert!(!overridden.contains("new-local\n"));
}

#[test]
fn an_unresolved_automatic_delta_gets_a_visible_plan_block_mapping() {
    use worktree_core::merge::MergeSource;

    let mut session = session_with_automatic_delta();
    let (automatic_index, automatic_id) = session
        .merge_plan
        .as_ref()
        .unwrap()
        .blocks
        .iter()
        .enumerate()
        .find(|(_, block)| block.is_delta && !block.original_conflict)
        .map(|(index, block)| (index, block.id))
        .unwrap();
    assert!(session.toggle_plan_block_source(automatic_id, MergeSource::B));
    let (projection, projected_plan_blocks) = conflict_session_plan_projection(&session).unwrap();
    let mut segments = conflict_resolver::parse_conflict_markers(projection.as_ref());
    let applied = conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
        &mut segments,
        &session,
        &projected_plan_blocks,
    )
    .expect("exact mapping");
    let plan_blocks = applied.block_plan_indices;
    assert!(plan_blocks.contains(&automatic_index));
    assert_eq!(
        plan_blocks,
        session.merge_plan.as_ref().unwrap().unresolved_blocks
    );
}

#[test]
fn plan_whitespace_classification_reaches_the_display_blocks() {
    use worktree_core::conflict_session::{ConflictPayload, ConflictSession};
    use worktree_core::domain::FileConflictKind;

    // Both sides only respaced the same line, so kdiff3's per-row rule
    // marks the block whitespace-only.
    let session = ConflictSession::from_stage_inputs(
        std::path::PathBuf::from("file.txt"),
        FileConflictKind::BothModified,
        ConflictPayload::Text("value = 1\n".into()),
        ConflictPayload::Text("value=1\n".into()),
        ConflictPayload::Text("value  =  1\n".into()),
    );
    assert!(
        session
            .merge_plan
            .as_ref()
            .expect("plan-backed session")
            .blocks
            .iter()
            .any(|block| block.whitespace_conflict),
        "fixture should produce a whitespace conflict"
    );

    let (projection, projected_plan_blocks) = conflict_session_plan_projection(&session).unwrap();
    let mut segments = conflict_resolver::parse_conflict_markers(projection.as_ref());
    conflict_resolver::apply_plan_session_region_resolutions_with_index_map(
        &mut segments,
        &session,
        &projected_plan_blocks,
    )
    .expect("exact mapping");

    assert!(
        segments.iter().any(|segment| matches!(
            segment,
            conflict_resolver::ConflictSegment::Block(block) if block.whitespace_only
        )),
        "the plan's whitespace verdict should land on the display block"
    );
}

#[test]
fn conflict_file_source_fingerprint_is_stable_across_fresh_allocations() {
    let make_file = || worktree_state::model::ConflictFile {
        path: std::path::PathBuf::from("index.html").into(),
        base_bytes: Some(std::sync::Arc::<[u8]>::from(b"base\nbytes\n".as_slice())),
        ours_bytes: None,
        theirs_bytes: Some(std::sync::Arc::<[u8]>::from(b"theirs\nbytes\n".as_slice())),
        current_bytes: None,
        base: Some(std::sync::Arc::<str>::from("base\ntext\n")),
        ours: Some(std::sync::Arc::<str>::from("ours\ntext\n")),
        theirs: Some(std::sync::Arc::<str>::from("theirs\ntext\n")),
        current: Some(std::sync::Arc::<str>::from(
            "<<<<<<< ours\nbody\n=======\nbody\n>>>>>>> theirs\n",
        )),
    };

    let left = make_file();
    let right = make_file();

    assert_eq!(
        conflict_file_source_fingerprint(&left),
        conflict_file_source_fingerprint(&right),
        "content-identical conflict files should keep the lightweight resync path even when backing Arcs are freshly allocated",
    );
}

#[test]
fn shared_content_fingerprints_keep_domains_distinct() {
    let none_text = None;
    let empty_text = Some(std::sync::Arc::<str>::from(""));
    let text = Some(std::sync::Arc::<str>::from("shared payload"));

    let none_bytes = None;
    let empty_bytes = Some(std::sync::Arc::<[u8]>::from(b"".as_slice()));
    let bytes = Some(std::sync::Arc::<[u8]>::from(b"shared payload".as_slice()));

    assert_ne!(
        shared_text_fingerprint(&none_text),
        shared_text_fingerprint(&empty_text),
        "missing text should not collide with an empty text payload",
    );
    assert_ne!(
        shared_bytes_fingerprint(&none_bytes),
        shared_bytes_fingerprint(&empty_bytes),
        "missing bytes should not collide with an empty byte payload",
    );
    assert_ne!(
        shared_text_fingerprint(&text),
        shared_bytes_fingerprint(&bytes),
        "text and byte payloads use separate fingerprint domains",
    );
}
