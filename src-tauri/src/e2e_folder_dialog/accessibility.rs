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

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "macos", derive(serde::Serialize))]
pub(super) struct ConfirmCandidate {
    pub(super) source: &'static str,
    pub(super) role: Option<String>,
    pub(super) title: Option<String>,
    pub(super) identifier: Option<String>,
    pub(super) enabled: Option<bool>,
}

fn record_candidate<A: Accessibility>(
    ax: &A,
    element: &A::Element,
    source: &'static str,
    candidates: &mut Vec<ConfirmCandidate>,
) {
    // Bound the snapshot. Read labels alone, never editable values or AX error payloads.
    if candidates.len() == 32 {
        return;
    }
    let label = |name| {
        ax.string(element, name)
            .ok()
            .map(|text| text.chars().take(128).collect())
    };
    candidates.push(ConfirmCandidate {
        source,
        role: label("AXRole"),
        title: label("AXTitle"),
        identifier: label("AXIdentifier"),
        enabled: ax.boolean(element, "AXEnabled").ok(),
    });
}

pub(super) fn confirm_button<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    identifier: &str,
    failures: &mut Vec<ConfirmFailure>,
    candidates: &mut Vec<ConfirmCandidate>,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    let read = |_| AttributeUnavailable;
    check_deadline()?;
    // The remote Open panel can be absent from AXWindows but remain the focused window.
    let focused = ax.element(app, "AXFocusedWindow").ok();
    check_deadline()?;
    let mut pending = ax.elements(app, "AXWindows").map_err(read)?;
    if let Some(panel) = &focused {
        pending.push(panel.clone());
    }
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
        let is_focused = focused.as_ref() == Some(&element);
        record_candidate(
            ax,
            &element,
            if is_focused {
                "focused_window"
            } else {
                "window_or_sheet"
            },
            candidates,
        );
        check_deadline()?;
        let role = ax.string(&element, "AXRole").map_err(read)?;
        if !matches!(role.as_str(), "AXWindow" | "AXSheet") {
            return Err(WrongTopLevelElement);
        }
        let matches_identifier =
            ax.string(&element, "AXIdentifier").ok().as_deref() == Some(identifier);
        identifier_found |= matches_identifier;
        check_deadline()?;
        let default = ax.element(&element, "AXDefaultButton").ok();
        if let Some(button) = &default {
            record_candidate(ax, button, "default_button", candidates);
        }
        check_deadline()?;
        // The prompt identifies the focused panel even when its title and identifier do not propagate.
        // Never trust focus alone or a generic Open label.
        let matches_prompt = is_focused
            && default.as_ref().is_some_and(|button| {
                ax.string(button, "AXTitle").ok().as_deref() == Some(identifier)
            });
        if matches_identifier
            || ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier)
            || matches_prompt
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
            // Recheck prompt-only identity because focus or the default button can change during the scan.
            if ax.string(panel, "AXIdentifier").ok().as_deref() != Some(identifier)
                && ax.string(panel, "AXTitle").ok().as_deref() != Some(identifier)
            {
                check_deadline()?;
                if ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(panel)
                    || ax.string(&button, "AXTitle").ok().as_deref() != Some(identifier)
                {
                    failures.push(PromptNotFound);
                    return Ok(None);
                }
            }
            check_deadline()?;
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
            record_candidate(ax, &element, "child_button", candidates);
            check_deadline()?;
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
