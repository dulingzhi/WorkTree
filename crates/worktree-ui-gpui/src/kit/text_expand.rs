use gpui::SharedString;

/// Expand tabs to the four spaces the row painters use for a tab stop, or
/// borrow the input unchanged when it contains none. The four-space convention
/// is shared with the markdown flow painter (`MARKDOWN_FLOW_TAB_COLUMNS`).
pub(crate) fn maybe_expand_tabs(s: &str) -> SharedString {
    if !s.contains('\t') {
        return SharedString::new(s);
    }

    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' => out.push_str("    "),
            _ => out.push(ch),
        }
    }
    out.into()
}
