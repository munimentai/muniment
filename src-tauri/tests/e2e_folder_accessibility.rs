#[path = "../src/e2e_folder_dialog/accessibility.rs"]
mod accessibility;

use accessibility::{confirm_button, Accessibility};
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

#[derive(Default)]
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

    fn confirm(&self) -> Result<Option<usize>, String> {
        confirm_button(self, &APP, IDENTIFIER)
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
            assert_eq!(
                tree.confirm(),
                Err("The Home picker button belongs to another window or sheet.".into())
            );
        }
    }
}

#[test]
fn duplicate_identifiers_fail_across_windows_and_sheets() {
    for sheet in [false, true] {
        let mut tree = Tree::panel(sheet);
        tree.set(HOST, "AXIdentifier", Value::String(IDENTIFIER.into()));
        assert_eq!(
            tree.confirm(),
            Err("The Home picker needs exactly one matching Accessibility window or sheet.".into())
        );
    }
    let mut tree = Tree::panel(true);
    tree.set(HOST, "AXChildren", Value::Elements(vec![PANEL, OTHER]));
    tree.set(OTHER, "AXRole", Value::String("AXSheet".into()));
    tree.set(OTHER, "AXChildren", Value::Elements(vec![]));
    tree.set(OTHER, "AXIdentifier", Value::String(IDENTIFIER.into()));
    assert!(tree.confirm().unwrap_err().contains("exactly one"));
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
            (PANEL, "AXDefaultButton"),
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
