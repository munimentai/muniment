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

// Reasons contain no AX text, paths, identifiers, or attribute error payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfirmFailure {
    IdentifierNotFound,
    NoDefaultButton,
    ButtonDisabled,
    WrongTopLevelElement,
    WrongButtonRole,
    PromptNotFound,
    AmbiguousPanel,
    AmbiguousButton,
    AttributeUnavailable,
    ElementLimit,
    Deadline,
}

impl ConfirmFailure {
    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::IdentifierNotFound => "identifier_not_found_in_windows_or_sheets",
            Self::NoDefaultButton => "no_default_button",
            Self::ButtonDisabled => "button_disabled",
            Self::WrongTopLevelElement => "wrong_top_level_element",
            Self::WrongButtonRole => "wrong_button_role",
            Self::PromptNotFound => "confirm_prompt_not_found",
            Self::AmbiguousPanel => "ambiguous_panel",
            Self::AmbiguousButton => "ambiguous_button",
            Self::AttributeUnavailable => "attribute_unavailable",
            Self::ElementLimit => "element_limit",
            Self::Deadline => "deadline",
        }
    }
}

pub(super) fn confirm_button<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    identifier: &str,
    failures: &mut Vec<ConfirmFailure>,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    let read = |_| AttributeUnavailable;
    check_deadline()?;
    let mut pending = ax.elements(app, "AXWindows").map_err(read)?;
    let mut visited = Vec::new();
    let mut panels = Vec::new();
    let mut identifier_found = false;
    while let Some(element) = pending.pop() {
        check_deadline()?;
        // A sheet may appear in both AXWindows and its parent's AXChildren.
        if visited.contains(&element) {
            continue;
        }
        if visited.len() == 256 {
            return Err(ElementLimit);
        }
        visited.push(element.clone());
        let role = ax.string(&element, "AXRole").map_err(read)?;
        if !matches!(role.as_str(), "AXWindow" | "AXSheet") {
            return Err(WrongTopLevelElement);
        }
        let matches_identifier =
            ax.string(&element, "AXIdentifier").ok().as_deref() == Some(identifier);
        identifier_found |= matches_identifier;
        // NSSavePanel forwards its title to remote panel content, unlike an AX identifier.
        if matches_identifier || ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier)
        {
            panels.push(element.clone());
        }
        // AppKit exposes attached sheets as children, not necessarily as windows.
        for child in ax.elements(&element, "AXChildren").map_err(read)? {
            check_deadline()?;
            if ax.string(&child, "AXRole").ok().as_deref() == Some("AXSheet") {
                pending.push(child);
            }
        }
    }
    if !identifier_found {
        failures.push(IdentifierNotFound);
    }
    if panels.len() > 1 {
        return Err(AmbiguousPanel);
    }
    let Some(panel) = panels.first() else {
        return Ok(None);
    };
    check_deadline()?;
    match ax.element(panel, "AXDefaultButton") {
        Ok(button) => {
            if enabled_button(ax, &button, panel)? {
                return Ok(Some(button));
            }
            failures.push(ButtonDisabled);
        }
        Err(_) => failures.push(NoDefaultButton),
    }

    // Remote panels can expose the live button below an AXGroup instead of AXDefaultButton.
    // Match the prompt set on this panel, never a localized or generic "Open" label.
    let mut pending = ax.elements(panel, "AXChildren").map_err(read)?;
    let mut visited = vec![panel.clone()];
    let mut buttons = Vec::new();
    while let Some(element) = pending.pop() {
        check_deadline()?;
        if visited.contains(&element) {
            continue;
        }
        if visited.len() == 256 {
            return Err(ElementLimit);
        }
        visited.push(element.clone());
        let role = ax.string(&element, "AXRole").map_err(read)?;
        if role == "AXButton" {
            if ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier) {
                buttons.push(element);
            }
        } else if matches!(role.as_str(), "AXGroup" | "AXSplitGroup" | "AXScrollArea") {
            pending.extend(ax.elements(&element, "AXChildren").map_err(read)?);
        }
    }
    if buttons.len() > 1 {
        return Err(AmbiguousButton);
    }
    let Some(button) = buttons.pop() else {
        failures.push(PromptNotFound);
        return Ok(None);
    };
    check_deadline()?;
    if !enabled_button(ax, &button, panel)? {
        failures.push(ButtonDisabled);
        return Ok(None);
    }
    Ok(Some(button))
}

fn enabled_button<A: Accessibility>(
    ax: &A,
    button: &A::Element,
    panel: &A::Element,
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    let read = |_| AttributeUnavailable;
    if ax.string(button, "AXRole").map_err(read)? != "AXButton" {
        return Err(WrongButtonRole);
    }
    // AXWindow names the parent window even when the button belongs to a sheet.
    if ax.element(button, "AXTopLevelUIElement").map_err(read)? != *panel {
        return Err(WrongTopLevelElement);
    }
    ax.boolean(button, "AXEnabled").map_err(read)
}
