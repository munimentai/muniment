pub(super) trait Accessibility {
    type Element: Clone + PartialEq;

    fn elements(&self, element: &Self::Element, name: &str) -> Result<Vec<Self::Element>, String>;
    fn element(&self, element: &Self::Element, name: &str) -> Result<Self::Element, String>;
    fn string(&self, element: &Self::Element, name: &str) -> Result<String, String>;
    fn boolean(&self, element: &Self::Element, name: &str) -> Result<bool, String>;
}

// Read labels alone. Never read editable values or act on another app.
pub(super) fn prompt_labels<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    labels: &mut Vec<String>,
    mut check_deadline: impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    check_deadline()?;
    let mut pending = ax.elements(app, "AXWindows")?;
    let mut visited = Vec::new();
    while let Some(element) = pending.pop() {
        check_deadline()?;
        if visited.contains(&element) {
            continue;
        }
        if visited.len() == 64 {
            return Err("The system prompt snapshot reached its element limit.".into());
        }
        visited.push(element.clone());
        let role = ax.string(&element, "AXRole")?;
        let attribute = match role.as_str() {
            "AXWindow" | "AXSheet" => Some("AXTitle"),
            "AXStaticText" => Some("AXValue"),
            "AXGroup" => None,
            _ => continue,
        };
        if let Some(attribute) = attribute {
            check_deadline()?;
            if let Ok(label) = ax.string(&element, attribute) {
                let label: String = label.chars().take(512).collect();
                if !label.is_empty() && !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        if role != "AXStaticText" {
            check_deadline()?;
            pending.extend(ax.elements(&element, "AXChildren")?);
        }
    }
    Ok(())
}

pub(super) fn confirm_button<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    identifier: &str,
) -> Result<Option<A::Element>, String> {
    let mut pending = ax.elements(app, "AXWindows")?;
    let mut visited = Vec::new();
    let mut panels = Vec::new();
    while let Some(element) = pending.pop() {
        // A sheet may appear in both AXWindows and its parent's AXChildren.
        if visited.contains(&element) {
            continue;
        }
        visited.push(element.clone());
        if ax.string(&element, "AXIdentifier").ok().as_deref() == Some(identifier) {
            panels.push(element.clone());
        }
        // AppKit exposes attached sheets as children, not necessarily as windows.
        for child in ax.elements(&element, "AXChildren")? {
            if ax.string(&child, "AXRole").ok().as_deref() == Some("AXSheet") {
                pending.push(child);
            }
        }
    }
    if panels.len() > 1 {
        return Err(
            "The Home picker needs exactly one matching Accessibility window or sheet.".into(),
        );
    }
    if panels.is_empty() {
        return Ok(None);
    }
    let panel = &panels[0];
    let button = ax.element(panel, "AXDefaultButton")?;
    // AXWindow names the parent window even when the button belongs to a sheet.
    if ax.element(&button, "AXTopLevelUIElement")? != *panel {
        return Err("The Home picker button belongs to another window or sheet.".into());
    }
    if !ax.boolean(&button, "AXEnabled")? {
        return Ok(None);
    }
    Ok(Some(button))
}
