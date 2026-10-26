//! Small text rules shared by the glossary and the atlas.

/// Lowercase words of letters and digits joined by single hyphens.
pub fn is_kebab(text: &str) -> bool {
    !text.is_empty()
        && text.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// A kebab id for a repo path: `crates/rosetta-core` becomes `crates-rosetta-core`.
pub fn kebab_of(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Length as JavaScript counts it, so limits match the TypeScript pilot and
/// the viewer.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_rules() {
        assert!(is_kebab("door"));
        assert!(is_kebab("crates-rosetta-core2"));
        assert!(!is_kebab("Door"));
        assert!(!is_kebab("a--b"));
        assert!(!is_kebab("-a"));
        assert!(!is_kebab(""));
        assert_eq!(kebab_of("crates/rosetta_core.rs"), "crates-rosetta-core-rs");
        assert_eq!(kebab_of("@naos/ui"), "naos-ui");
    }
}
