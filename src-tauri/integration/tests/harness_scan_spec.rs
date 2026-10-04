use muniment_core::harness_scan::REGISTRY;

#[test]
fn registry_covers_every_spec_table_row() {
    let spec = include_str!("../../../SPEC.md");
    let table = spec
        .split("| Assistant | Root and override | Counted | Never |")
        .nth(1)
        .unwrap();
    let names: Vec<_> = table
        .lines()
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .map(|line| line.split('|').nth(1).unwrap().trim())
        .collect();
    assert_eq!(REGISTRY.len(), names.len());
    assert_eq!(
        REGISTRY
            .iter()
            .map(|row| row.display_name)
            .collect::<Vec<_>>(),
        names
    );
    let ids: std::collections::BTreeSet<_> = REGISTRY.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), REGISTRY.len());
    for row in REGISTRY {
        assert!(!row.counted.is_empty());
        assert!(!row.never.is_empty());
    }
}
