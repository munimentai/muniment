pub(super) fn matches_home(selected: Option<&str>, home: &str) -> bool {
    // The panel answers a resolved path, so compare canonical forms.
    let canonical = |path: &str| std::fs::canonicalize(path).ok();
    let expected = canonical(home);
    expected.is_some() && selected.and_then(canonical) == expected
}
