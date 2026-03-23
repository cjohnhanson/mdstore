use rand::RngExt;

const PREFIX_LEN: usize = 4;
const PREFIX_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Generate a unique 4-character `[a-z0-9]` prefix that doesn't collide with `existing`.
pub fn generate_prefix(existing: &[String]) -> String {
    let mut rng = rand::rng();
    loop {
        let prefix: String = (0..PREFIX_LEN)
            .map(|_| PREFIX_CHARS[rng.random_range(0..PREFIX_CHARS.len())] as char)
            .collect();
        if !existing.iter().any(|e| e == &prefix) {
            return prefix;
        }
    }
}

/// Returns true if `id` has a short-id prefix (4 chars of `[a-z0-9]` followed by `-`).
pub fn has_prefix(id: &str) -> bool {
    extract_prefix(id).is_some()
}

/// Extract the prefix and slug portions from a prefixed ID.
/// Returns `None` if the ID has no prefix.
pub fn extract_prefix(id: &str) -> Option<(&str, &str)> {
    if id.len() < PREFIX_LEN + 2 {
        // Need at least "xxxx-y"
        return None;
    }
    let (candidate, rest) = id.split_at(PREFIX_LEN);
    if !rest.starts_with('-') {
        return None;
    }
    if !candidate
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        return None;
    }
    Some((candidate, &rest[1..]))
}

pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_slugification() {
        assert_eq!(slugify("Fix the widget"), "fix-the-widget");
        assert_eq!(slugify("Write tests"), "write-tests");
    }

    #[test]
    fn special_characters() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("foo--bar"), "foo-bar");
        assert_eq!(slugify("  leading spaces  "), "leading-spaces");
    }

    #[test]
    fn punctuation_replaced_with_hyphens() {
        assert_eq!(slugify("clc.yaml"), "clc-yaml");
        assert_eq!(slugify("clc.yaml support"), "clc-yaml-support");
        assert_eq!(slugify("fix...multiple dots"), "fix-multiple-dots");
        assert_eq!(slugify("foo@bar#baz"), "foo-bar-baz");
        assert_eq!(slugify("a.b.c.d"), "a-b-c-d");
    }

    #[test]
    fn already_slugged() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }

    #[test]
    fn unicode_normalized_to_ascii() {
        assert_eq!(slugify("Über die Grenze"), "-ber-die-grenze");
        assert_eq!(slugify("naïve résumé"), "na-ve-r-sum");
        assert_eq!(slugify("café"), "caf");
        assert_eq!(slugify("日本語テスト"), "");
        assert_eq!(slugify("mix of ASCII and 日本語"), "mix-of-ascii-and");
    }

    // -- Short ID prefix tests --

    #[test]
    fn generate_prefix_format() {
        let prefix = generate_prefix(&[]);
        assert_eq!(prefix.len(), 4);
        assert!(
            prefix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
    }

    #[test]
    fn generate_prefix_avoids_existing() {
        let existing = vec!["ab12".to_string(), "cd34".to_string()];
        for _ in 0..100 {
            let prefix = generate_prefix(&existing);
            assert_ne!(prefix, "ab12");
            assert_ne!(prefix, "cd34");
        }
    }

    #[test]
    fn has_prefix_detects_prefixed_ids() {
        assert!(has_prefix("ab12-fix-the-widget"));
        assert!(has_prefix("zz99-some-slug"));
        assert!(has_prefix("a1b2-x"));
    }

    #[test]
    fn has_prefix_rejects_unprefixed_ids() {
        assert!(!has_prefix("fix-the-widget"));
        assert!(!has_prefix("abc-something")); // 3 chars, not 4
        assert!(!has_prefix("abcde-something")); // 5 chars, not 4
        assert!(!has_prefix("ABCD-something")); // uppercase
        assert!(!has_prefix("ab!2-something")); // non-alphanumeric
    }

    #[test]
    fn extract_prefix_on_prefixed_id() {
        assert_eq!(
            extract_prefix("ab12-fix-the-widget"),
            Some(("ab12", "fix-the-widget"))
        );
    }

    #[test]
    fn extract_prefix_on_unprefixed_id() {
        assert_eq!(extract_prefix("fix-the-widget"), None);
    }

    #[test]
    fn extract_prefix_on_short_prefix_only() {
        // Just a prefix with no slug after it — not valid
        assert_eq!(extract_prefix("ab12"), None);
    }

    #[test]
    fn prefixed_id_roundtrip() {
        let prefix = "ab12";
        let slug = slugify("Fix the widget");
        let full_id = format!("{prefix}-{slug}");
        assert_eq!(full_id, "ab12-fix-the-widget");
        let (p, s) = extract_prefix(&full_id).unwrap();
        assert_eq!(p, "ab12");
        assert_eq!(s, "fix-the-widget");
    }
}
