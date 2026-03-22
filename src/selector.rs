/// A single `namespace:value` selector for filtering documents.
///
/// Selectors provide a uniform query syntax across tools. Each tool
/// defines which namespaces are valid and how matching works for its
/// document type.
#[derive(Debug, Clone)]
pub struct Selector {
    pub namespace: String,
    pub value: String,
}

impl Selector {
    /// Parse a `namespace:value` string. Returns `None` if there is no colon.
    pub fn parse(s: &str) -> Option<Self> {
        let (namespace, value) = s.split_once(':')?;
        Some(Selector {
            namespace: namespace.to_string(),
            value: value.to_string(),
        })
    }
}

/// Returns true if the item matches all selectors (AND semantics).
pub fn matches_all<T>(selectors: &[Selector], item: &T, matcher: impl Fn(&Selector, &T) -> bool) -> bool {
    selectors.iter().all(|s| matcher(s, item))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let s = Selector::parse("tag:ml").unwrap();
        assert_eq!(s.namespace, "tag");
        assert_eq!(s.value, "ml");
    }

    #[test]
    fn parse_no_colon() {
        assert!(Selector::parse("no-colon").is_none());
    }

    #[test]
    fn parse_empty_value() {
        let s = Selector::parse("status:").unwrap();
        assert_eq!(s.namespace, "status");
        assert_eq!(s.value, "");
    }

    #[test]
    fn parse_colon_in_value() {
        let s = Selector::parse("key:val:ue").unwrap();
        assert_eq!(s.namespace, "key");
        assert_eq!(s.value, "val:ue");
    }

    #[test]
    fn matches_all_and_semantics() {
        let selectors = vec![
            Selector::parse("a:1").unwrap(),
            Selector::parse("b:2").unwrap(),
        ];

        // Both match
        assert!(matches_all(&selectors, &"test", |s, _| {
            s.namespace == "a" || s.namespace == "b"
        }));

        // One doesn't match
        assert!(!matches_all(&selectors, &"test", |s, _| {
            s.namespace == "a"
        }));
    }

    #[test]
    fn matches_all_empty_selectors() {
        let selectors: Vec<Selector> = vec![];
        assert!(matches_all(&selectors, &"anything", |_, _| false));
    }
}
