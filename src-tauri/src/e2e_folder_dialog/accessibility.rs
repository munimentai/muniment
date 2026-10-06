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
    TopLevelAttributeUnavailable,
    ParentAttributeUnavailable,
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
            Self::TopLevelAttributeUnavailable => "AXTopLevelUIElement_unavailable",
            Self::ParentAttributeUnavailable => "AXParent_unavailable",
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
        let matches_title = ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier);
        let matches_panel = matches_identifier || matches_title || matches_prompt;
        let child_prompt = if is_focused && !matches_panel {
            child_confirm_button(
                ax,
                &element,
                identifier,
                role == "AXSheet",
                failures,
                candidates,
                &mut check_deadline,
            )?
        } else {
            None
        };
        if matches_panel || child_prompt.is_some() {
            // Keep child-only identity separate. It cannot authorize a different default button.
            panels.push((element.clone(), child_prompt));
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
    let Some((panel, child_identity)) = panels.first() else {
        return Ok(None);
    };
    check_deadline()?;
    if let Some(expected) = child_identity {
        // Recheck the live child tree and focus before trusting child-only prompt identity.
        let button = child_confirm_button(
            ax,
            panel,
            identifier,
            ax.string(panel, "AXRole").ok().as_deref() == Some("AXSheet"),
            failures,
            candidates,
            &mut check_deadline,
        )?;
        check_deadline()?;
        if button.as_ref() != Some(expected)
            || ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(panel)
        {
            failures.push(PromptNotFound);
            return Ok(None);
        }
        return Ok(button);
    }
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
            if enabled_button(ax, &button, panel, false, failures, &mut check_deadline)? {
                return Ok(Some(button));
            }
            failures.push(ButtonDisabled);
        }
        Err(_) => failures.push(NoDefaultButton),
    }

    child_confirm_button(
        ax,
        panel,
        identifier,
        false,
        failures,
        candidates,
        &mut check_deadline,
    )
}

// Keyboard navigation needs the marked panel to own focus, not just a visible button.
pub(super) fn focused_panel<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    button: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<A::Element, ConfirmFailure> {
    check_deadline()?;
    let panel = ax
        .element(app, "AXFocusedWindow")
        .map_err(|_| ConfirmFailure::AttributeUnavailable)?;
    let role = ax
        .string(&panel, "AXRole")
        .map_err(|_| ConfirmFailure::AttributeUnavailable)?;
    if !matches!(role.as_str(), "AXSheet" | "AXWindow") {
        return Err(ConfirmFailure::WrongTopLevelElement);
    }
    if !enabled_button(
        ax,
        button,
        &panel,
        role == "AXSheet",
        &mut Vec::new(),
        &mut check_deadline,
    )? {
        return Err(ConfirmFailure::ButtonDisabled);
    }
    check_deadline()?;
    if ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(&panel) {
        return Err(ConfirmFailure::WrongTopLevelElement);
    }
    Ok(panel)
}

pub(super) fn has_sheet<A: Accessibility>(
    ax: &A,
    panel: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let mut pending = ax
        .elements(panel, "AXChildren")
        .map_err(|_| AttributeUnavailable)?;
    let mut visited = vec![panel.clone()];
    while let Some(element) = pending.pop() {
        check_deadline()?;
        if visited.contains(&element) {
            continue;
        }
        if visited.len() == 256 {
            return Err(ElementLimit);
        }
        visited.push(element.clone());
        match ax
            .string(&element, "AXRole")
            .map_err(|_| AttributeUnavailable)?
            .as_str()
        {
            "AXSheet" => return Ok(true),
            "AXWindow"
                if ax.string(&element, "AXIdentifier").ok().as_deref() == Some("GoToWindow") =>
            {
                return Ok(true);
            }
            "AXGroup" | "AXSplitGroup" | "AXScrollArea" => {
                pending.extend(
                    ax.elements(&element, "AXChildren")
                        .map_err(|_| AttributeUnavailable)?,
                );
            }
            _ => {}
        }
    }
    Ok(false)
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "macos", derive(serde::Serialize))]
pub(super) struct NavigationEvent {
    pub(super) stage: &'static str,
    pub(super) role: Option<String>,
    pub(super) identifier: Option<String>,
}

impl NavigationEvent {
    pub(super) fn record(self, events: &mut Vec<Self>) {
        // Keep milestones even when repeated focus changes fill the diagnostic limit.
        let keep = if self.stage == "focused_window" {
            events.last() != Some(&self) && events.len() < 64
        } else {
            !events.contains(&self)
        };
        if keep {
            events.push(self);
        }
    }

    pub(super) fn stage(stage: &'static str) -> Self {
        Self {
            stage,
            role: None,
            identifier: None,
        }
    }

    pub(super) fn focused_window<A: Accessibility>(ax: &A, app: &A::Element) -> Self {
        let window = ax.element(app, "AXFocusedWindow").ok();
        let label = |name| {
            window
                .as_ref()
                .and_then(|window| ax.string(window, name).ok())
                .map(|text| text.chars().take(128).collect())
        };
        Self {
            stage: "focused_window",
            role: label("AXRole"),
            identifier: label("AXIdentifier"),
        }
    }
}

// Match GoToWindow and PathTextField, never a generic text field or a localized label.
pub(super) fn go_to_folder_field<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let Ok(window) = ax.element(app, "AXFocusedWindow") else {
        return Ok(None);
    };
    // A remote panel can retain window focus while its Go to Folder field has keyboard focus.
    // Use the field's owner only when this drive's panel still owns window focus.
    let sheet = if window == *panel {
        let Ok(field) = ax.element(app, "AXFocusedUIElement") else {
            return Ok(None);
        };
        if ax.string(&field, "AXIdentifier").ok().as_deref() != Some("PathTextField") {
            return Ok(None);
        }
        ax.element(&field, "AXParent")
            .map_err(|_| AttributeUnavailable)?
    } else {
        window.clone()
    };
    if sheet == *panel || ax.string(&sheet, "AXIdentifier").ok().as_deref() != Some("GoToWindow") {
        return Ok(None);
    }
    if !matches!(
        ax.string(&sheet, "AXRole").ok().as_deref(),
        Some("AXSheet" | "AXWindow")
    ) {
        return Err(WrongTopLevelElement);
    }
    let children = ax
        .elements(&sheet, "AXChildren")
        .map_err(|_| AttributeUnavailable)?;
    if children.len() > 256 {
        return Err(ElementLimit);
    }
    let mut fields = Vec::new();
    for child in children {
        check_deadline()?;
        if ax.string(&child, "AXIdentifier").ok().as_deref() == Some("PathTextField")
            && !fields.contains(&child)
        {
            fields.push(child);
        }
    }
    if fields.len() > 1 {
        return Err(AttributeUnavailable);
    }
    let Some(field) = fields.pop() else {
        return Ok(None);
    };
    check_deadline()?;
    if ax.string(&field, "AXRole").ok().as_deref() != Some("AXTextField")
        || ax
            .element(&field, "AXParent")
            .map_err(|_| AttributeUnavailable)?
            != sheet
    {
        return Err(WrongTopLevelElement);
    }
    if !ax
        .boolean(&field, "AXEnabled")
        .map_err(|_| AttributeUnavailable)?
        || ax.element(app, "AXFocusedUIElement").ok().as_ref() != Some(&field)
        || ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(&window)
    {
        return Ok(None);
    }
    // Both the edit and Return require this sheet to belong to the marked panel.
    check_deadline()?;
    if ax
        .element(&sheet, "AXParent")
        .map_err(|_| AttributeUnavailable)?
        != *panel
    {
        return Err(WrongTopLevelElement);
    }
    Ok(Some(field))
}

fn child_confirm_button<A: Accessibility>(
    ax: &A,
    panel: &A::Element,
    identifier: &str,
    allow_parent_chain: bool,
    failures: &mut Vec<ConfirmFailure>,
    candidates: &mut Vec<ConfirmCandidate>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    let read = |_| AttributeUnavailable;
    // Remote panels can expose the live button below an AXGroup instead of AXDefaultButton.
    // Match the prompt set on this panel, never a localized or generic "Open" label.
    check_deadline()?;
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
    if !enabled_button(
        ax,
        &button,
        panel,
        allow_parent_chain,
        failures,
        check_deadline,
    )? {
        failures.push(ButtonDisabled);
        return Ok(None);
    }
    Ok(Some(button))
}

fn enabled_button<A: Accessibility>(
    ax: &A,
    button: &A::Element,
    panel: &A::Element,
    allow_parent_chain: bool,
    failures: &mut Vec<ConfirmFailure>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    let read = |_| AttributeUnavailable;
    if ax.string(button, "AXRole").map_err(read)? != "AXButton" {
        return Err(WrongButtonRole);
    }
    // AXWindow names the parent window even when the button belongs to a sheet.
    match ax.element(button, "AXTopLevelUIElement") {
        Ok(owner) if owner != *panel => return Err(WrongTopLevelElement),
        Ok(_) => {}
        Err(_) => {
            failures.push(TopLevelAttributeUnavailable);
            if !allow_parent_chain {
                return Err(AttributeUnavailable);
            }
            // Only focused sheet children can use ancestry when the top-level attribute is unavailable.
            parent_chain(ax, button, panel, failures, check_deadline)?;
        }
    }
    check_deadline()?;
    ax.boolean(button, "AXEnabled").map_err(read)
}

fn parent_chain<A: Accessibility>(
    ax: &A,
    button: &A::Element,
    panel: &A::Element,
    failures: &mut Vec<ConfirmFailure>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<(), ConfirmFailure> {
    use ConfirmFailure::*;
    let mut current = button.clone();
    let mut visited = vec![current.clone()];
    for _ in 0..256 {
        check_deadline()?;
        let parent = ax.element(&current, "AXParent").map_err(|_| {
            failures.push(ParentAttributeUnavailable);
            AttributeUnavailable
        })?;
        if parent == *panel {
            return Ok(());
        }
        if visited.contains(&parent) {
            return Err(WrongTopLevelElement);
        }
        check_deadline()?;
        let role = ax
            .string(&parent, "AXRole")
            .map_err(|_| AttributeUnavailable)?;
        if !matches!(role.as_str(), "AXGroup" | "AXSplitGroup" | "AXScrollArea") {
            return Err(WrongTopLevelElement);
        }
        visited.push(parent.clone());
        current = parent;
    }
    Err(ElementLimit)
}
