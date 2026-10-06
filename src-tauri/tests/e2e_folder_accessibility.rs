#[path = "../src/e2e_folder_dialog/accessibility.rs"]
mod accessibility;

use accessibility::{confirm_button, Accessibility, ConfirmFailure};
use std::collections::HashMap;

const APP: usize = 0;
const HOST: usize = 1;
const PANEL: usize = 2;
const BUTTON: usize = 3;
const OTHER: usize = 4;
const IDENTIFIER: &str = "muniment-e2e-home-test";

#[derive(Clone)]
enum Value {
    Elements(Vec<usize>),
    Element(usize),
    String(String),
    Boolean(bool),
}

#[derive(Clone, Default)]
struct Tree(
    HashMap<(usize, &'static str), Value>,
    std::cell::RefCell<Vec<(usize, String)>>,
);

impl Tree {
    fn set(&mut self, element: usize, name: &'static str, value: Value) {
        self.0.insert((element, name), value);
    }

    fn get(&self, element: usize, name: &str) -> Result<Value, String> {
        self.1.borrow_mut().push((element, name.into()));
        self.0
            .get(&(element, name))
            .cloned()
            .ok_or_else(|| format!("The test element {element} has no {name}."))
    }

    fn panel(sheet: bool) -> Self {
        let mut tree = Self::default();
        tree.set(APP, "AXWindows", Value::Elements(vec![HOST]));
        tree.set(HOST, "AXRole", Value::String("AXWindow".into()));
        tree.set(
            HOST,
            "AXChildren",
            Value::Elements(if sheet { vec![PANEL] } else { vec![] }),
        );
        if !sheet {
            tree.set(APP, "AXWindows", Value::Elements(vec![HOST, PANEL]));
        }
        tree.set(PANEL, "AXChildren", Value::Elements(vec![BUTTON]));
        tree.set(
            PANEL,
            "AXRole",
            Value::String(if sheet { "AXSheet" } else { "AXWindow" }.into()),
        );
        tree.set(PANEL, "AXIdentifier", Value::String(IDENTIFIER.into()));
        tree.set(PANEL, "AXDefaultButton", Value::Element(BUTTON));
        tree.set(BUTTON, "AXRole", Value::String("AXButton".into()));
        tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(PANEL));
        // A sheet's button reports the host window, not the sheet, as AXWindow.
        tree.set(
            BUTTON,
            "AXWindow",
            Value::Element(if sheet { HOST } else { PANEL }),
        );
        tree.set(BUTTON, "AXEnabled", Value::Boolean(true));
        tree
    }

    fn lookup(&self) -> (Result<Option<usize>, ConfirmFailure>, Vec<ConfirmFailure>) {
        let mut failures = Vec::new();
        let result = confirm_button(
            self,
            &APP,
            IDENTIFIER,
            &mut failures,
            &mut Vec::new(),
            || Ok(()),
        );
        if let Err(reason) = result {
            failures.push(reason);
        }
        (result, failures)
    }

    fn confirm(&self) -> Result<Option<usize>, ConfirmFailure> {
        self.lookup().0
    }

    fn focused_remote_sheet() -> Self {
        let mut tree = Self::remote_panel(true);
        tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
        tree.set(HOST, "AXChildren", Value::Elements(vec![]));
        tree.set(PANEL, "AXIdentifier", Value::String("open-panel".into()));
        tree.0.remove(&(PANEL, "AXTitle"));
        tree
    }

    fn focused_remote_sheet_with_parent_chain() -> Self {
        let mut tree = Self::focused_remote_sheet();
        tree.0.remove(&(BUTTON, "AXTopLevelUIElement"));
        tree.set(BUTTON, "AXParent", Value::Element(OTHER));
        tree.set(OTHER, "AXParent", Value::Element(PANEL));
        tree
    }

    fn remote_panel(sheet: bool) -> Self {
        let mut tree = Self::panel(sheet);
        tree.0.remove(&(PANEL, "AXIdentifier"));
        tree.0.remove(&(PANEL, "AXDefaultButton"));
        tree.set(PANEL, "AXTitle", Value::String(IDENTIFIER.into()));
        tree.set(PANEL, "AXChildren", Value::Elements(vec![OTHER]));
        tree.set(OTHER, "AXRole", Value::String("AXGroup".into()));
        tree.set(OTHER, "AXChildren", Value::Elements(vec![BUTTON]));
        tree.set(BUTTON, "AXTitle", Value::String(IDENTIFIER.into()));
        tree
    }
}

impl Accessibility for Tree {
    type Element = usize;

    fn elements(&self, element: &usize, name: &str) -> Result<Vec<usize>, String> {
        match self.get(*element, name)? {
            Value::Elements(elements) => Ok(elements),
            _ => Err(format!("The test needs an element list for {name}.")),
        }
    }

    fn element(&self, element: &usize, name: &str) -> Result<usize, String> {
        match self.get(*element, name)? {
            Value::Element(element) => Ok(element),
            _ => Err(format!("The test needs an element for {name}.")),
        }
    }

    fn string(&self, element: &usize, name: &str) -> Result<String, String> {
        match self.get(*element, name)? {
            Value::String(value) => Ok(value),
            _ => Err(format!("The test needs text for {name}.")),
        }
    }

    fn boolean(&self, element: &usize, name: &str) -> Result<bool, String> {
        match self.get(*element, name)? {
            Value::Boolean(value) => Ok(value),
            _ => Err(format!("The test needs a boolean for {name}.")),
        }
    }
}

#[test]
fn attached_sheet_uses_its_own_top_level_element() {
    assert_eq!(Tree::panel(true).confirm(), Ok(Some(BUTTON)));
}

#[test]
fn standalone_panel_uses_its_own_top_level_element() {
    assert_eq!(Tree::panel(false).confirm(), Ok(Some(BUTTON)));
}

#[test]
fn foreign_buttons_fail_even_with_a_shared_host_window() {
    for sheet in [false, true] {
        for owner in [HOST, OTHER] {
            let mut tree = Tree::panel(sheet);
            tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(owner));
            assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
        }
    }
}

#[test]
fn duplicate_identifiers_fail_across_windows_and_sheets() {
    for sheet in [false, true] {
        let mut tree = Tree::panel(sheet);
        tree.set(HOST, "AXIdentifier", Value::String(IDENTIFIER.into()));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
    }
    let mut tree = Tree::panel(true);
    tree.set(HOST, "AXChildren", Value::Elements(vec![PANEL, OTHER]));
    tree.set(OTHER, "AXRole", Value::String("AXSheet".into()));
    tree.set(OTHER, "AXChildren", Value::Elements(vec![]));
    tree.set(OTHER, "AXIdentifier", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
}

#[test]
fn repeated_sheet_references_count_once() {
    let mut tree = Tree::panel(true);
    tree.set(APP, "AXWindows", Value::Elements(vec![HOST, PANEL]));
    tree.set(PANEL, "AXChildren", Value::Elements(vec![BUTTON, PANEL]));
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
}

#[test]
fn nested_sheet_is_reachable() {
    let mut tree = Tree::panel(true);
    tree.set(HOST, "AXChildren", Value::Elements(vec![OTHER]));
    tree.set(OTHER, "AXRole", Value::String("AXSheet".into()));
    tree.set(OTHER, "AXChildren", Value::Elements(vec![PANEL]));
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
}

#[test]
fn no_match_or_disabled_button_waits() {
    let mut tree = Tree::default();
    tree.set(APP, "AXWindows", Value::Elements(vec![]));
    assert_eq!(tree.confirm(), Ok(None));
    for sheet in [false, true] {
        let mut tree = Tree::panel(sheet);
        tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
        assert_eq!(tree.confirm(), Ok(None));
        tree.set(BUTTON, "AXEnabled", Value::Boolean(true));
        assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
        tree.set(PANEL, "AXIdentifier", Value::String("another-panel".into()));
        assert_eq!(tree.confirm(), Ok(None));
    }
}

#[test]
fn non_sheet_children_cannot_impersonate_the_panel() {
    let mut tree = Tree::panel(true);
    tree.set(PANEL, "AXRole", Value::String("AXGroup".into()));
    assert_eq!(tree.confirm(), Ok(None));
}

#[test]
fn remote_panel_uses_its_title_and_prompt_below_a_group() {
    for sheet in [false, true] {
        let tree = Tree::remote_panel(sheet);
        assert_eq!(
            tree.lookup(),
            (
                Ok(Some(BUTTON)),
                vec![
                    ConfirmFailure::IdentifierNotFound,
                    ConfirmFailure::NoDefaultButton,
                ],
            )
        );
        assert!(tree.1.borrow().iter().all(|(_, name)| name != "AXValue"));
    }
}

#[test]
fn focused_remote_sheet_uses_only_its_marker_child() {
    for role in ["AXGroup", "AXSplitGroup", "AXScrollArea"] {
        let mut tree = Tree::focused_remote_sheet();
        tree.set(OTHER, "AXRole", Value::String(role.into()));
        assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
        // A generic default button cannot inherit the child's prompt identity.
        tree.set(PANEL, "AXDefaultButton", Value::Element(5));
        tree.set(5, "AXRole", Value::String("AXButton".into()));
        tree.set(5, "AXTitle", Value::String("Open".into()));
        tree.set(5, "AXTopLevelUIElement", Value::Element(PANEL));
        tree.set(5, "AXEnabled", Value::Boolean(true));
        assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
    }
}

#[test]
fn remote_child_identity_requires_focus_and_an_unambiguous_panel() {
    let mut tree = Tree::focused_remote_sheet();
    tree.set(HOST, "AXChildren", Value::Elements(vec![PANEL]));
    tree.0.remove(&(APP, "AXFocusedWindow"));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(APP, "AXFocusedWindow", Value::Element(HOST));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
    tree.set(HOST, "AXIdentifier", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
}

#[test]
fn focused_remote_child_rejects_ambiguous_disabled_and_foreign_buttons() {
    let mut tree = Tree::focused_remote_sheet_with_parent_chain();
    tree.set(OTHER, "AXChildren", Value::Elements(vec![BUTTON, 5]));
    tree.set(5, "AXRole", Value::String("AXButton".into()));
    tree.set(5, "AXTitle", Value::String(IDENTIFIER.into()));
    tree.set(5, "AXTopLevelUIElement", Value::Element(PANEL));
    tree.set(5, "AXEnabled", Value::Boolean(true));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousButton));
    tree.set(OTHER, "AXChildren", Value::Elements(vec![BUTTON]));
    tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(BUTTON, "AXEnabled", Value::Boolean(true));
    tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(HOST));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
    let mut tree = Tree::focused_remote_sheet_with_parent_chain();
    tree.0.remove(&(BUTTON, "AXEnabled"));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
    tree.set(BUTTON, "AXEnabled", Value::String("invalid".into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
}

#[test]
fn focused_sheet_confirms_a_marker_child_with_parent_ownership() {
    for role in ["AXGroup", "AXSplitGroup", "AXScrollArea"] {
        let mut tree = Tree::focused_remote_sheet_with_parent_chain();
        tree.set(OTHER, "AXRole", Value::String(role.into()));
        tree.set(BUTTON, "AXIdentifier", Value::String("OKButton".into()));
        for invalid_top_level in [false, true] {
            if invalid_top_level {
                tree.set(
                    BUTTON,
                    "AXTopLevelUIElement",
                    Value::String("private AX text".into()),
                );
            }
            let (result, failures) = tree.lookup();
            assert_eq!(result, Ok(Some(BUTTON)));
            assert_eq!(
                failures,
                vec![
                    ConfirmFailure::TopLevelAttributeUnavailable,
                    ConfirmFailure::IdentifierNotFound,
                    ConfirmFailure::TopLevelAttributeUnavailable,
                ]
            );
            assert_eq!(failures[0].reason(), "AXTopLevelUIElement_unavailable");
            assert!(!tree.1.borrow().iter().any(|(_, name)| name == "AXValue"));
        }
    }
}

#[test]
fn parent_ownership_rejects_missing_foreign_and_invalid_chains() {
    for node in [BUTTON, OTHER] {
        let mut tree = Tree::focused_remote_sheet_with_parent_chain();
        tree.0.remove(&(node, "AXParent"));
        for invalid_parent in [false, true] {
            if invalid_parent {
                tree.set(node, "AXParent", Value::String("private AX text".into()));
            }
            assert_eq!(
                tree.lookup(),
                (
                    Err(ConfirmFailure::AttributeUnavailable),
                    vec![
                        ConfirmFailure::TopLevelAttributeUnavailable,
                        ConfirmFailure::ParentAttributeUnavailable,
                        ConfirmFailure::AttributeUnavailable,
                    ],
                )
            );
            assert_eq!(
                ConfirmFailure::ParentAttributeUnavailable.reason(),
                "AXParent_unavailable"
            );
        }
    }
    for owner in [HOST, BUTTON, OTHER] {
        let mut tree = Tree::focused_remote_sheet_with_parent_chain();
        tree.set(OTHER, "AXParent", Value::Element(owner));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
    }
    let mut tree = Tree::focused_remote_sheet_with_parent_chain();
    tree.set(BUTTON, "AXParent", Value::Element(5));
    tree.set(5, "AXParent", Value::Element(PANEL));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
    for role in ["AXWindow", "AXSheet", "AXButton", "AXTextField", ""] {
        let mut tree = Tree::focused_remote_sheet_with_parent_chain();
        tree.set(BUTTON, "AXParent", Value::Element(5));
        tree.set(5, "AXRole", Value::String(role.into()));
        tree.set(5, "AXParent", Value::Element(PANEL));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
    }
}

#[test]
fn parent_ownership_keeps_default_window_and_unfocused_paths_strict() {
    let mut default = Tree::focused_remote_sheet_with_parent_chain();
    default.set(PANEL, "AXDefaultButton", Value::Element(BUTTON));
    let mut window = Tree::focused_remote_sheet_with_parent_chain();
    window.set(PANEL, "AXRole", Value::String("AXWindow".into()));
    let mut unfocused = Tree::focused_remote_sheet_with_parent_chain();
    unfocused.0.remove(&(APP, "AXFocusedWindow"));
    unfocused.set(HOST, "AXChildren", Value::Elements(vec![PANEL]));
    unfocused.set(PANEL, "AXTitle", Value::String(IDENTIFIER.into()));
    for mut tree in [default, window, unfocused] {
        for invalid_top_level in [false, true] {
            if invalid_top_level {
                tree.set(
                    BUTTON,
                    "AXTopLevelUIElement",
                    Value::String("private AX text".into()),
                );
            }
            assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
            assert!(!tree.1.borrow().iter().any(|(_, name)| name == "AXParent"));
        }
    }
}

#[test]
fn parent_ownership_bounds_traversal_and_checks_the_deadline() {
    for groups in [0, 255, 256] {
        let mut tree = Tree::focused_remote_sheet_with_parent_chain();
        tree.set(
            BUTTON,
            "AXParent",
            Value::Element(if groups == 0 { PANEL } else { 5 }),
        );
        for node in 5..5 + groups {
            tree.set(node, "AXRole", Value::String("AXGroup".into()));
            tree.set(
                node,
                "AXParent",
                Value::Element(if node == 4 + groups { PANEL } else { node + 1 }),
            );
        }
        assert_eq!(
            tree.confirm(),
            if groups < 256 {
                Ok(Some(BUTTON))
            } else {
                Err(ConfirmFailure::ElementLimit)
            }
        );
        tree.1.borrow_mut().clear();
        assert_eq!(
            confirm_button(
                &tree,
                &APP,
                IDENTIFIER,
                &mut Vec::new(),
                &mut Vec::new(),
                || {
                    if tree.1.borrow().iter().any(|(_, name)| name == "AXParent") {
                        Err(ConfirmFailure::Deadline)
                    } else {
                        Ok(())
                    }
                },
            ),
            Err(ConfirmFailure::Deadline)
        );
        assert_eq!(
            tree.1
                .borrow()
                .iter()
                .filter(|(_, name)| name == "AXParent")
                .count(),
            1
        );
    }
}

#[test]
fn focused_child_scan_bounds_cycles_elements_and_deadlines() {
    let mut tree = Tree::focused_remote_sheet();
    tree.set(
        OTHER,
        "AXChildren",
        Value::Elements(vec![OTHER, PANEL, BUTTON, BUTTON]),
    );
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
    tree.set(OTHER, "AXRole", Value::String("AXSheet".into()));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(OTHER, "AXRole", Value::String("AXGroup".into()));
    tree.set(OTHER, "AXChildren", Value::Elements((5..=261).collect()));
    for node in 5..=261 {
        tree.set(node, "AXRole", Value::String("AXButton".into()));
    }
    assert_eq!(tree.confirm(), Err(ConfirmFailure::ElementLimit));
    tree.1.borrow_mut().clear();
    let mut candidates = Vec::new();
    assert_eq!(
        confirm_button(
            &tree,
            &APP,
            IDENTIFIER,
            &mut Vec::new(),
            &mut candidates,
            || {
                if tree.1.borrow().iter().any(|(node, _)| *node >= 5) {
                    Err(ConfirmFailure::Deadline)
                } else {
                    Ok(())
                }
            }
        ),
        Err(ConfirmFailure::Deadline)
    );
    assert_eq!(candidates[0].source, "focused_window");
    assert_eq!(candidates[1].source, "child_button");
}

#[test]
fn unmatched_focused_sheet_records_child_buttons_without_editable_values() {
    for title in ["Open", "Ouvrir", "", "muniment-e2e-home-other"] {
        let mut tree = Tree::focused_remote_sheet();
        tree.set(BUTTON, "AXTitle", Value::String(title.into()));
        let mut candidates = Vec::new();
        assert_eq!(
            confirm_button(
                &tree,
                &APP,
                IDENTIFIER,
                &mut Vec::new(),
                &mut candidates,
                || Ok(())
            ),
            Ok(None)
        );
        assert_eq!(candidates[0].source, "focused_window");
        assert_eq!(candidates[0].role.as_deref(), Some("AXSheet"));
        assert_eq!(candidates[0].identifier.as_deref(), Some("open-panel"));
        assert_eq!(candidates[0].title, None);
        assert_eq!(candidates[1].source, "child_button");
        assert_eq!(candidates[1].title.as_deref(), Some(title));
        assert_eq!(candidates[1].enabled, Some(true));
        assert!(!tree.1.borrow().iter().any(|(_, name)| name == "AXValue"));
    }
}

#[test]
fn child_prompt_identity_rechecks_focus_and_the_live_tree() {
    struct ChangedTree {
        initial: Tree,
        changed: Tree,
        scans: std::cell::Cell<usize>,
    }
    impl ChangedTree {
        fn current(&self) -> &Tree {
            if self.scans.get() > 1 {
                &self.changed
            } else {
                &self.initial
            }
        }
    }
    impl Accessibility for ChangedTree {
        type Element = usize;

        fn elements(&self, element: &usize, name: &str) -> Result<Vec<usize>, String> {
            if *element == OTHER && name == "AXChildren" {
                self.scans.set(self.scans.get() + 1);
            }
            self.current().elements(element, name)
        }

        fn element(&self, element: &usize, name: &str) -> Result<usize, String> {
            self.current().element(element, name)
        }

        fn string(&self, element: &usize, name: &str) -> Result<String, String> {
            self.current().string(element, name)
        }

        fn boolean(&self, element: &usize, name: &str) -> Result<bool, String> {
            self.current().boolean(element, name)
        }
    }
    for (element, name, value, expected) in [
        (APP, "AXFocusedWindow", Value::Element(HOST), Ok(None)),
        (OTHER, "AXChildren", Value::Elements(vec![]), Ok(None)),
        (OTHER, "AXChildren", Value::Elements(vec![5]), Ok(None)),
        (
            OTHER,
            "AXChildren",
            Value::Elements(vec![BUTTON, 5]),
            Err(ConfirmFailure::AmbiguousButton),
        ),
        (BUTTON, "AXTitle", Value::String("Open".into()), Ok(None)),
        (BUTTON, "AXEnabled", Value::Boolean(false), Ok(None)),
        (
            BUTTON,
            "AXTopLevelUIElement",
            Value::Element(HOST),
            Err(ConfirmFailure::WrongTopLevelElement),
        ),
    ] {
        for initial in [
            Tree::focused_remote_sheet(),
            Tree::focused_remote_sheet_with_parent_chain(),
        ] {
            let mut changed = initial.clone();
            changed.set(5, "AXRole", Value::String("AXButton".into()));
            changed.set(5, "AXTitle", Value::String(IDENTIFIER.into()));
            changed.set(5, "AXTopLevelUIElement", Value::Element(PANEL));
            changed.set(5, "AXEnabled", Value::Boolean(true));
            changed.set(element, name, value.clone());
            let ax = ChangedTree {
                initial,
                changed,
                scans: std::cell::Cell::new(0),
            };
            assert_eq!(
                confirm_button(
                    &ax,
                    &APP,
                    IDENTIFIER,
                    &mut Vec::new(),
                    &mut Vec::new(),
                    || Ok(())
                ),
                expected
            );
        }
    }
    for owner in [HOST, BUTTON] {
        let initial = Tree::focused_remote_sheet_with_parent_chain();
        let mut changed = initial.clone();
        changed.set(OTHER, "AXParent", Value::Element(owner));
        let ax = ChangedTree {
            initial,
            changed,
            scans: std::cell::Cell::new(0),
        };
        assert_eq!(
            confirm_button(
                &ax,
                &APP,
                IDENTIFIER,
                &mut Vec::new(),
                &mut Vec::new(),
                || Ok(()),
            ),
            Err(ConfirmFailure::WrongTopLevelElement)
        );
    }
}

#[test]
fn focused_default_precedes_window_and_child_button_lookups() {
    let mut tree = Tree::remote_panel(false);
    tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
    tree.set(APP, "AXWindows", Value::Elements(vec![HOST]));
    tree.0.remove(&(PANEL, "AXTitle"));
    tree.set(PANEL, "AXDefaultButton", Value::Element(BUTTON));
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
    let reads = tree.1.borrow();
    let position = |element, attribute| {
        reads
            .iter()
            .position(|(node, name)| *node == element && name == attribute)
            .unwrap()
    };
    assert!(position(APP, "AXFocusedWindow") < position(APP, "AXWindows"));
    assert!(position(PANEL, "AXDefaultButton") < position(HOST, "AXIdentifier"));
    assert!(!reads
        .iter()
        .any(|(node, name)| *node == OTHER && name == "AXChildren"));
}

#[test]
fn focused_window_requires_a_marker_and_valid_button_ownership() {
    let mut tree = Tree::panel(false);
    tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
    tree.0.remove(&(PANEL, "AXIdentifier"));
    tree.set(BUTTON, "AXTitle", Value::String("Open".into()));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(BUTTON, "AXTitle", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
    tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(HOST));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
    tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(PANEL));
    tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(BUTTON, "AXEnabled", Value::Boolean(true));
    tree.set(HOST, "AXIdentifier", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
}

#[test]
fn prompt_only_identity_rejects_a_changed_focus_or_default_button() {
    struct ChangedAttribute {
        tree: Tree,
        attribute: &'static str,
        reads: std::cell::Cell<usize>,
    }
    impl Accessibility for ChangedAttribute {
        type Element = usize;

        fn elements(&self, element: &usize, name: &str) -> Result<Vec<usize>, String> {
            self.tree.elements(element, name)
        }

        fn element(&self, element: &usize, name: &str) -> Result<usize, String> {
            if name == self.attribute {
                self.reads.set(self.reads.get() + 1);
                if self.reads.get() > 1 {
                    return Ok(OTHER);
                }
            }
            self.tree.element(element, name)
        }

        fn string(&self, element: &usize, name: &str) -> Result<String, String> {
            self.tree.string(element, name)
        }

        fn boolean(&self, element: &usize, name: &str) -> Result<bool, String> {
            self.tree.boolean(element, name)
        }
    }
    for attribute in ["AXFocusedWindow", "AXDefaultButton"] {
        let mut tree = Tree::panel(false);
        tree.set(APP, "AXWindows", Value::Elements(vec![PANEL]));
        tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
        tree.0.remove(&(PANEL, "AXIdentifier"));
        tree.set(BUTTON, "AXTitle", Value::String(IDENTIFIER.into()));
        tree.set(OTHER, "AXRole", Value::String("AXButton".into()));
        tree.set(OTHER, "AXTitle", Value::String("Open".into()));
        tree.set(OTHER, "AXTopLevelUIElement", Value::Element(PANEL));
        tree.set(OTHER, "AXEnabled", Value::Boolean(true));
        let ax = ChangedAttribute {
            tree,
            attribute,
            reads: std::cell::Cell::new(0),
        };
        assert_eq!(
            confirm_button(
                &ax,
                &APP,
                IDENTIFIER,
                &mut Vec::new(),
                &mut Vec::new(),
                || Ok(())
            ),
            Ok(None)
        );
    }
}

#[test]
fn unrelated_focus_does_not_override_a_marked_sheet() {
    let mut tree = Tree::panel(true);
    tree.set(APP, "AXFocusedWindow", Value::Element(HOST));
    tree.set(HOST, "AXDefaultButton", Value::Element(OTHER));
    tree.set(OTHER, "AXRole", Value::String("AXButton".into()));
    tree.set(OTHER, "AXTitle", Value::String("Open".into()));
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
}

#[test]
fn failed_lookup_lists_bounded_candidates_without_editable_values() {
    let mut tree = Tree::panel(false);
    tree.set(APP, "AXFocusedWindow", Value::Element(PANEL));
    tree.0.remove(&(PANEL, "AXIdentifier"));
    tree.set(BUTTON, "AXTitle", Value::String("é".repeat(200)));
    tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
    let mut candidates = Vec::new();
    assert_eq!(
        confirm_button(
            &tree,
            &APP,
            IDENTIFIER,
            &mut Vec::new(),
            &mut candidates,
            || Ok(())
        ),
        Ok(None)
    );
    assert_eq!(candidates[0].source, "focused_window");
    assert_eq!(candidates[0].role.as_deref(), Some("AXWindow"));
    assert_eq!(candidates[0].identifier, None);
    assert_eq!(candidates[1].source, "default_button");
    assert_eq!(candidates[1].role.as_deref(), Some("AXButton"));
    assert_eq!(candidates[1].enabled, Some(false));
    assert_eq!(candidates[1].title.as_ref().unwrap().chars().count(), 128);
    assert!(!tree.1.borrow().iter().any(|(_, name)| name == "AXValue"));

    tree.set(APP, "AXWindows", Value::Elements((5..45).collect()));
    for node in 5..45 {
        tree.set(node, "AXRole", Value::String("AXWindow".into()));
        tree.set(node, "AXChildren", Value::Elements(vec![]));
    }
    candidates.clear();
    assert_eq!(
        confirm_button(
            &tree,
            &APP,
            IDENTIFIER,
            &mut Vec::new(),
            &mut candidates,
            || Ok(())
        ),
        Ok(None)
    );
    assert_eq!(candidates.len(), 32);
}

#[test]
fn remote_button_can_replace_a_disabled_default_proxy() {
    let mut tree = Tree::remote_panel(false);
    tree.set(PANEL, "AXDefaultButton", Value::Element(5));
    tree.set(5, "AXRole", Value::String("AXButton".into()));
    tree.set(5, "AXTopLevelUIElement", Value::Element(PANEL));
    tree.set(5, "AXEnabled", Value::Boolean(false));
    assert_eq!(
        tree.lookup(),
        (
            Ok(Some(BUTTON)),
            vec![
                ConfirmFailure::IdentifierNotFound,
                ConfirmFailure::ButtonDisabled,
            ],
        )
    );
}

#[test]
fn lookup_reasons_distinguish_missing_identifier_default_and_disabled_button() {
    for sheet in [false, true] {
        let mut tree = Tree::panel(sheet);
        tree.0.remove(&(PANEL, "AXIdentifier"));
        assert_eq!(
            tree.lookup(),
            (Ok(None), vec![ConfirmFailure::IdentifierNotFound])
        );
        tree.set(PANEL, "AXIdentifier", Value::String(IDENTIFIER.into()));
        tree.0.remove(&(PANEL, "AXDefaultButton"));
        let expected = (
            Ok(None),
            vec![
                ConfirmFailure::NoDefaultButton,
                ConfirmFailure::PromptNotFound,
            ],
        );
        assert_eq!(tree.lookup(), expected);
        tree.set(PANEL, "AXDefaultButton", Value::String("invalid".into()));
        assert_eq!(tree.lookup(), expected);
        tree.set(PANEL, "AXDefaultButton", Value::Element(BUTTON));
        tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
        assert_eq!(
            tree.lookup(),
            (
                Ok(None),
                vec![
                    ConfirmFailure::ButtonDisabled,
                    ConfirmFailure::PromptNotFound
                ],
            )
        );
        tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(OTHER));
        assert_eq!(
            tree.lookup(),
            (
                Err(ConfirmFailure::WrongTopLevelElement),
                vec![ConfirmFailure::WrongTopLevelElement],
            )
        );
    }
}

#[test]
fn remote_lookup_rejects_foreign_disabled_and_ambiguous_buttons() {
    for sheet in [false, true] {
        for owner in [HOST, OTHER] {
            let mut tree = Tree::remote_panel(sheet);
            tree.set(BUTTON, "AXTopLevelUIElement", Value::Element(owner));
            assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
        }
        let mut tree = Tree::remote_panel(sheet);
        tree.set(BUTTON, "AXEnabled", Value::Boolean(false));
        assert_eq!(tree.confirm(), Ok(None));
        assert_eq!(
            tree.lookup().1.last(),
            Some(&ConfirmFailure::ButtonDisabled)
        );
        tree.set(BUTTON, "AXEnabled", Value::Boolean(true));
        tree.set(OTHER, "AXChildren", Value::Elements(vec![BUTTON, 5]));
        tree.set(5, "AXRole", Value::String("AXButton".into()));
        tree.set(5, "AXTitle", Value::String(IDENTIFIER.into()));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousButton));
    }
}

#[test]
fn remote_lookup_requires_exact_markers_and_rejects_other_sheets() {
    let mut tree = Tree::remote_panel(false);
    tree.set(BUTTON, "AXTitle", Value::String("Open".into()));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(BUTTON, "AXTitle", Value::String(IDENTIFIER.into()));
    tree.set(PANEL, "AXTitle", Value::String("Open".into()));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(PANEL, "AXTitle", Value::String(IDENTIFIER.into()));
    tree.set(OTHER, "AXRole", Value::String("AXSheet".into()));
    tree.set(OTHER, "AXTitle", Value::String("another-panel".into()));
    assert_eq!(tree.confirm(), Ok(None));
    tree.set(OTHER, "AXTitle", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
}

#[test]
fn title_and_identifier_matches_cannot_select_different_panels() {
    let mut tree = Tree::remote_panel(false);
    tree.set(HOST, "AXIdentifier", Value::String(IDENTIFIER.into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::AmbiguousPanel));
    tree.set(APP, "AXWindows", Value::Elements(vec![PANEL]));
    tree.set(PANEL, "AXRole", Value::String("AXGroup".into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongTopLevelElement));
}

#[test]
fn lookup_bounds_cycles_traversal_and_deadlines() {
    let mut tree = Tree::remote_panel(false);
    tree.set(
        OTHER,
        "AXChildren",
        Value::Elements(vec![OTHER, PANEL, BUTTON, BUTTON]),
    );
    assert_eq!(tree.confirm(), Ok(Some(BUTTON)));
    tree.set(OTHER, "AXChildren", Value::Elements((5..=261).collect()));
    for node in 5..=261 {
        tree.set(node, "AXRole", Value::String("AXButton".into()));
    }
    assert_eq!(tree.confirm(), Err(ConfirmFailure::ElementLimit));
    tree.1.borrow_mut().clear();
    assert_eq!(
        confirm_button(
            &tree,
            &APP,
            IDENTIFIER,
            &mut Vec::new(),
            &mut Vec::new(),
            || { Err(ConfirmFailure::Deadline) }
        ),
        Err(ConfirmFailure::Deadline)
    );
    assert!(tree.1.borrow().is_empty());
    let mut calls = 0;
    assert_eq!(
        confirm_button(
            &tree,
            &APP,
            IDENTIFIER,
            &mut Vec::new(),
            &mut Vec::new(),
            || {
                calls += 1;
                if calls > 2 {
                    Err(ConfirmFailure::Deadline)
                } else {
                    Ok(())
                }
            }
        ),
        Err(ConfirmFailure::Deadline)
    );
    assert!(calls < 10);
}

#[test]
fn lookup_errors_never_expose_attribute_payloads() {
    let tree = Tree::default();
    assert_eq!(
        tree.lookup(),
        (
            Err(ConfirmFailure::AttributeUnavailable),
            vec![ConfirmFailure::AttributeUnavailable]
        )
    );
    assert_eq!(
        ConfirmFailure::AttributeUnavailable.reason(),
        "attribute_unavailable"
    );
    for reason in [
        ConfirmFailure::IdentifierNotFound,
        ConfirmFailure::NoDefaultButton,
        ConfirmFailure::ButtonDisabled,
        ConfirmFailure::WrongTopLevelElement,
    ] {
        assert!(!reason.reason().contains(IDENTIFIER));
        assert!(!reason.reason().contains('/'));
    }
}

#[test]
fn remote_lookup_rejects_missing_or_invalid_ownership_and_enablement() {
    for name in ["AXTopLevelUIElement", "AXEnabled"] {
        let mut tree = Tree::remote_panel(false);
        tree.0.remove(&(BUTTON, name));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
        tree.set(BUTTON, name, Value::String("invalid".into()));
        assert_eq!(tree.confirm(), Err(ConfirmFailure::AttributeUnavailable));
    }
    let mut tree = Tree::remote_panel(false);
    tree.set(BUTTON, "AXRole", Value::String("AXTextField".into()));
    assert_eq!(tree.confirm(), Ok(None));
}

#[test]
fn default_button_must_be_a_button() {
    let mut tree = Tree::panel(false);
    tree.set(BUTTON, "AXRole", Value::String("AXTextField".into()));
    assert_eq!(tree.confirm(), Err(ConfirmFailure::WrongButtonRole));
}

fn prompt_labels(
    tree: &Tree,
    app: &usize,
    check_deadline: impl FnMut() -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let mut labels = Vec::new();
    accessibility::prompt_labels(tree, app, &mut labels, check_deadline)?;
    Ok(labels)
}

#[test]
fn prompt_labels_keep_the_owner_when_a_later_attribute_fails() {
    let mut tree = Tree::default();
    tree.set(APP, "AXWindows", Value::Elements(vec![HOST]));
    tree.set(HOST, "AXRole", Value::String("AXWindow".into()));
    tree.set(
        HOST,
        "AXTitle",
        Value::String("A helper needs authorization.".into()),
    );
    tree.set(HOST, "AXChildren", Value::Elements(vec![OTHER]));
    let mut labels = Vec::new();
    assert!(accessibility::prompt_labels(&tree, &APP, &mut labels, || Ok(())).is_err());
    assert_eq!(labels, vec!["A helper needs authorization."]);
}

#[test]
fn prompt_labels_read_window_titles_and_nested_static_text_alone() {
    let mut tree = Tree::panel(true);
    tree.set(HOST, "AXRole", Value::String("AXWindow".into()));
    tree.set(HOST, "AXTitle", Value::String("SecurityAgent".into()));
    tree.set(PANEL, "AXTitle", Value::String("Keychain access".into()));
    tree.set(PANEL, "AXChildren", Value::Elements(vec![BUTTON, OTHER, 5]));
    tree.set(OTHER, "AXRole", Value::String("AXGroup".into()));
    tree.set(OTHER, "AXChildren", Value::Elements(vec![6, PANEL]));
    tree.set(6, "AXRole", Value::String("AXStaticText".into()));
    tree.set(
        6,
        "AXValue",
        Value::String("A helper wants the login Keychain.".into()),
    );
    tree.set(5, "AXRole", Value::String("AXTextField".into()));
    tree.set(5, "AXValue", Value::String("secret".into()));
    let labels = prompt_labels(&tree, &APP, || Ok(())).unwrap();
    assert_eq!(
        labels,
        vec![
            "SecurityAgent",
            "Keychain access",
            "A helper wants the login Keychain."
        ]
    );
    for (element, name) in tree.1.borrow().iter() {
        assert!(name != "AXValue" || *element == 6);
        assert!(!matches!(name.as_str(), "AXDefaultButton" | "AXEnabled"));
    }
}

#[test]
fn prompt_labels_bound_text_and_handle_an_empty_window_list() {
    let mut tree = Tree::default();
    tree.set(APP, "AXWindows", Value::Elements(vec![]));
    assert_eq!(prompt_labels(&tree, &APP, || Ok(())), Ok(vec![]));
    tree.set(APP, "AXWindows", Value::Elements(vec![HOST, HOST]));
    tree.set(HOST, "AXRole", Value::String("AXStaticText".into()));
    tree.set(HOST, "AXValue", Value::String("é".repeat(600)));
    assert_eq!(
        prompt_labels(&tree, &APP, || Ok(())),
        Ok(vec!["é".repeat(512)])
    );
}

#[test]
fn prompt_labels_bound_traversal_and_report_inaccessible_attributes() {
    let mut tree = Tree::default();
    assert!(prompt_labels(&tree, &APP, || Ok(())).is_err());
    tree.set(APP, "AXWindows", Value::Elements((1..=65).collect()));
    for node in 1..=65 {
        tree.set(node, "AXRole", Value::String("AXButton".into()));
    }
    assert_eq!(
        prompt_labels(&tree, &APP, || Ok(())),
        Err("The system prompt snapshot reached its element limit.".into())
    );
    tree.1.borrow_mut().clear();
    assert_eq!(
        prompt_labels(&tree, &APP, || Err("timeout".into())),
        Err("timeout".into())
    );
    assert!(tree.1.borrow().is_empty());
    let mut calls = 0;
    assert_eq!(
        prompt_labels(&tree, &APP, || {
            calls += 1;
            if calls > 2 {
                Err("timeout".into())
            } else {
                Ok(())
            }
        }),
        Err("timeout".into())
    );
    assert_eq!(tree.1.borrow().len(), 2);
}

#[test]
fn missing_or_invalid_attributes_fail_closed() {
    for sheet in [false, true] {
        for (element, name) in [
            (APP, "AXWindows"),
            (HOST, "AXChildren"),
            (BUTTON, "AXTopLevelUIElement"),
            (BUTTON, "AXEnabled"),
        ] {
            let mut tree = Tree::panel(sheet);
            tree.0.remove(&(element, name));
            assert!(tree.confirm().is_err(), "The missing {name} must fail.");
            tree.set(element, name, Value::String("invalid".into()));
            assert!(tree.confirm().is_err(), "The invalid {name} must fail.");
        }
    }
}
