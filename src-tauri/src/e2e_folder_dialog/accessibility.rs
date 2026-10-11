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
    KeyboardFocusLost,
    NavigationSheetOpen,
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
            Self::KeyboardFocusLost => "keyboard_focus_lost",
            Self::NavigationSheetOpen => "navigation_sheet_open",
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

// The validated lookup result. Final confirmation revalidates this exact context,
// never a panel rediscovered from whatever window owns focus at that time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ConfirmTarget<E> {
    pub(super) button: E,
    pub(super) panel: E,
    // The focused window the Open panel sheet is attached to, when the panel is a host sheet.
    pub(super) host: Option<E>,
}

// The native drive keeps the full target. The button-only form serves the shared test support.
#[allow(dead_code)]
pub(super) fn confirm_button<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    identifier: &str,
    failures: &mut Vec<ConfirmFailure>,
    candidates: &mut Vec<ConfirmCandidate>,
    check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    confirm_target(ax, app, identifier, failures, candidates, check_deadline)
        .map(|target| target.map(|target| target.button))
}

pub(super) fn confirm_target<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    identifier: &str,
    failures: &mut Vec<ConfirmFailure>,
    candidates: &mut Vec<ConfirmCandidate>,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<ConfirmTarget<A::Element>>, ConfirmFailure> {
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
        check_deadline()?;
        // The remote Open panel can be a sheet attached to the focused window
        // instead of the focused window itself.
        let mut host = None;
        if let Some(window) = focused.as_ref().filter(|_| !is_focused) {
            if attached_host(ax, window, &element, &mut check_deadline)? {
                host = Some(window.clone());
            }
        }
        record_candidate(
            ax,
            &element,
            if is_focused {
                "focused_window"
            } else if host.is_some() {
                "focused_host_sheet"
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
        // The focused host of an Open panel sheet is not itself the panel.
        // Its prompt-only identity and content scan belong to the sheet.
        let hosts_sheet = if is_focused {
            host_sheet(ax, &element, &mut check_deadline)?.is_some()
        } else {
            false
        };
        check_deadline()?;
        let default = ax.element(&element, "AXDefaultButton").ok();
        if let Some(button) = &default {
            record_candidate(ax, button, "default_button", candidates);
        }
        check_deadline()?;
        // The prompt identifies the focused panel even when its title and identifier do not propagate.
        // Never trust focus alone or a generic Open label.
        let matches_prompt = is_focused
            && !hosts_sheet
            && default.as_ref().is_some_and(|button| {
                ax.string(button, "AXTitle").ok().as_deref() == Some(identifier)
            });
        let matches_title = ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier);
        let matches_panel = matches_identifier || matches_title || matches_prompt;
        // A host-attached sheet always proves identity through its exact prompt button.
        let unmarked_focus = is_focused && !matches_panel && !hosts_sheet;
        let child_prompt = if unmarked_focus || host.is_some() {
            child_confirm_button(
                ax,
                &element,
                identifier,
                role == "AXSheet",
                host.is_some(),
                failures,
                candidates,
                &mut check_deadline,
            )?
        } else {
            None
        };
        if matches_panel || child_prompt.is_some() {
            // Keep child-only identity separate. It cannot authorize a different default button.
            panels.push((element.clone(), child_prompt, host));
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
    let Some((panel, child_identity, host)) = panels.first() else {
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
            host.is_some(),
            failures,
            candidates,
            &mut check_deadline,
        )?;
        check_deadline()?;
        let focused_now = ax.element(app, "AXFocusedWindow").ok();
        let owns_focus = match (host, &focused_now) {
            // Revalidate the attachment, not just the host's focus, before the button can act.
            (Some(host), Some(window)) => {
                window == host && attached_host(ax, host, panel, &mut check_deadline)?
            }
            (Some(_), None) => false,
            (None, window) => window.as_ref() == Some(panel),
        };
        if button.as_ref() != Some(expected) || !owns_focus {
            failures.push(PromptNotFound);
            return Ok(None);
        }
        return Ok(button.map(|button| ConfirmTarget {
            button,
            panel: panel.clone(),
            host: host.clone(),
        }));
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
                return Ok(Some(ConfirmTarget {
                    button,
                    panel: panel.clone(),
                    host: None,
                }));
            }
            failures.push(ButtonDisabled);
        }
        Err(_) => failures.push(NoDefaultButton),
    }

    let button = child_confirm_button(
        ax,
        panel,
        identifier,
        false,
        false,
        failures,
        candidates,
        &mut check_deadline,
    )?;
    Ok(button.map(|button| ConfirmTarget {
        button,
        panel: panel.clone(),
        host: None,
    }))
}

// Keyboard navigation needs the marked panel to own focus, not just a visible button.
pub(super) fn focused_panel<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    button: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<A::Element, ConfirmFailure> {
    check_deadline()?;
    let window = ax
        .element(app, "AXFocusedWindow")
        .map_err(|_| ConfirmFailure::AttributeUnavailable)?;
    let role = ax
        .string(&window, "AXRole")
        .map_err(|_| ConfirmFailure::AttributeUnavailable)?;
    if !matches!(role.as_str(), "AXSheet" | "AXWindow") {
        return Err(ConfirmFailure::WrongTopLevelElement);
    }
    // An Open panel sheet attached to the focused window owns that window's focus.
    let sheet = host_sheet(ax, &window, &mut check_deadline)?;
    let (panel, sheet_panel) = match &sheet {
        Some(sheet) if is_ok_button(ax, button) => (sheet.clone(), true),
        Some(_) => return Err(ConfirmFailure::WrongTopLevelElement),
        None => (window.clone(), role == "AXSheet"),
    };
    if !enabled_button(
        ax,
        button,
        &panel,
        sheet_panel,
        &mut Vec::new(),
        &mut check_deadline,
    )? {
        return Err(ConfirmFailure::ButtonDisabled);
    }
    check_deadline()?;
    if !panel_owns_focus(ax, app, &panel, &mut check_deadline)? {
        return Err(ConfirmFailure::WrongTopLevelElement);
    }
    Ok(panel)
}

// Revalidate the lookup's own panel, host attachment, exact prompt and button
// immediately before the confirmation action. Any change fails closed.
pub(super) fn confirm_owner<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    target: &ConfirmTarget<A::Element>,
    identifier: &str,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<(), ConfirmFailure> {
    use ConfirmFailure::*;
    let button = &target.button;
    let panel = &target.panel;
    let host = &target.host;
    check_deadline()?;
    let window = ax
        .element(app, "AXFocusedWindow")
        .map_err(|_| AttributeUnavailable)?;
    check_deadline()?;
    match host {
        Some(host) => {
            // The sheet stays the one identified Open panel sheet on the same focused host.
            if window != *host
                || host_sheet(ax, host, &mut check_deadline)?.as_ref() != Some(panel)
                || !is_ok_button(ax, button)
            {
                return Err(WrongTopLevelElement);
            }
        }
        None => {
            // A standalone panel owns focus itself and never hosts an Open panel sheet.
            if window != *panel || host_sheet(ax, panel, &mut check_deadline)?.is_some() {
                return Err(WrongTopLevelElement);
            }
        }
    }
    check_deadline()?;
    // A host sheet and an unmarked focused panel prove identity only through the per-run prompt.
    // A generic or changed label never confirms.
    let marked_id = ax.string(panel, "AXIdentifier").ok().as_deref() == Some(identifier);
    let marked_title = ax.string(panel, "AXTitle").ok().as_deref() == Some(identifier);
    let marked = host.is_none() && (marked_id || marked_title);
    if !marked && ax.string(button, "AXTitle").ok().as_deref() != Some(identifier) {
        return Err(PromptNotFound);
    }
    if host.is_some() {
        // Rescan the sheet so a second matching button added after lookup fails closed.
        let buttons = prompt_buttons(
            ax,
            panel,
            identifier,
            true,
            &mut Vec::new(),
            &mut check_deadline,
        )?;
        if buttons.iter().any(|candidate| candidate != button) {
            return Err(AmbiguousButton);
        }
        if buttons.first() != Some(button) {
            return Err(WrongTopLevelElement);
        }
    }
    check_deadline()?;
    let sheet_panel = ax.string(panel, "AXRole").ok().as_deref() == Some("AXSheet");
    if !enabled_button(
        ax,
        button,
        panel,
        sheet_panel,
        &mut Vec::new(),
        &mut check_deadline,
    )? {
        return Err(ButtonDisabled);
    }
    // A Go to Folder sheet on the panel or on its original host blocks confirmation.
    if sheet_below(ax, panel, None, &mut check_deadline)? {
        return Err(NavigationSheetOpen);
    }
    if let Some(host) = host {
        if sheet_below(ax, host, Some(panel), &mut check_deadline)? {
            return Err(NavigationSheetOpen);
        }
    }
    check_deadline()?;
    if ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(&window) {
        return Err(WrongTopLevelElement);
    }
    Ok(())
}

const OPEN_PANEL_SHEET: &str = "open-panel";
const OPEN_PANEL_BUTTON: &str = "OKButton";

fn is_ok_button<A: Accessibility>(ax: &A, button: &A::Element) -> bool {
    ax.string(button, "AXIdentifier").ok().as_deref() == Some(OPEN_PANEL_BUTTON)
}

// Match only an identified Open panel sheet that the window lists among its children.
// Sheet AXParent can name the application, so the window's child list proves attachment.
fn attached_host<A: Accessibility>(
    ax: &A,
    window: &A::Element,
    sheet: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    check_deadline()?;
    if window == sheet
        || ax.string(sheet, "AXRole").ok().as_deref() != Some("AXSheet")
        || ax.string(sheet, "AXIdentifier").ok().as_deref() != Some(OPEN_PANEL_SHEET)
        || ax.string(window, "AXRole").ok().as_deref() != Some("AXWindow")
    {
        return Ok(false);
    }
    check_deadline()?;
    let children = ax.elements(window, "AXChildren");
    Ok(children.is_ok_and(|children| children.contains(sheet)))
}

// Return the single Open panel sheet attached to the focused window, if any.
fn host_sheet<A: Accessibility>(
    ax: &A,
    window: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    check_deadline()?;
    if ax.string(window, "AXRole").ok().as_deref() != Some("AXWindow") {
        return Ok(None);
    }
    let Ok(children) = ax.elements(window, "AXChildren") else {
        return Ok(None);
    };
    if children.len() > 256 {
        return Err(ConfirmFailure::ElementLimit);
    }
    let mut sheets: Vec<A::Element> = Vec::new();
    for child in children {
        if !sheets.contains(&child) && attached_host(ax, window, &child, check_deadline)? {
            sheets.push(child);
        }
    }
    if sheets.len() > 1 {
        return Err(ConfirmFailure::AmbiguousPanel);
    }
    Ok(sheets.pop())
}

// The panel owns window focus directly or through the focused window it is attached to.
fn panel_owns_focus<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    check_deadline()?;
    let Ok(window) = ax.element(app, "AXFocusedWindow") else {
        return Ok(false);
    };
    if window == *panel {
        return Ok(true);
    }
    attached_host(ax, &window, panel, check_deadline)
}

// HID events follow session focus, not the app PID. Recheck ownership before each key pair.
pub(super) fn post_navigation_key<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    field: Option<&A::Element>,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
    post_pair: impl FnOnce(),
) -> Result<(), ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let frontmost = || {
        ax.boolean(app, "AXFrontmost")
            .map_err(|_| AttributeUnavailable)
    };
    if !frontmost()? {
        return Err(KeyboardFocusLost);
    }
    if let Some(field) = field {
        if go_to_folder_field(ax, app, panel, &mut check_deadline)?.as_ref() != Some(field) {
            return Err(KeyboardFocusLost);
        }
    } else if navigation_sheet(ax, app, panel, &mut check_deadline)?
        || !panel_owns_focus(ax, app, panel, &mut check_deadline)?
    {
        return Err(KeyboardFocusLost);
    }
    // AX calls can outlast the deadline or let another app take focus.
    if !frontmost()? {
        return Err(KeyboardFocusLost);
    }
    check_deadline()?;
    // Keep key down and key up together. Never leave a key held after a focus change.
    post_pair();
    Ok(())
}

// A navigation sheet can attach to the panel or, for an Open panel sheet, to its focused host.
// The Open panel sheet itself is the panel, not an unfinished navigation sheet.
pub(super) fn navigation_sheet<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    if sheet_below(ax, panel, None, &mut check_deadline)? {
        return Ok(true);
    }
    check_deadline()?;
    let Ok(window) = ax.element(app, "AXFocusedWindow") else {
        return Ok(false);
    };
    if window == *panel || !attached_host(ax, &window, panel, &mut check_deadline)? {
        return Ok(false);
    }
    sheet_below(ax, &window, Some(panel), &mut check_deadline)
}

fn sheet_below<A: Accessibility>(
    ax: &A,
    root: &A::Element,
    skip: Option<&A::Element>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let mut pending = ax
        .elements(root, "AXChildren")
        .map_err(|_| AttributeUnavailable)?;
    let mut visited = vec![root.clone()];
    visited.extend(skip.cloned());
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
    // Use the field's owner only when this drive's panel still owns window focus,
    // directly or as the Open panel sheet attached to the focused window.
    let owns_focus = window == *panel || attached_host(ax, &window, panel, &mut check_deadline)?;
    let sheet = if owns_focus {
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
    require_ok_button: bool,
    failures: &mut Vec<ConfirmFailure>,
    candidates: &mut Vec<ConfirmCandidate>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    let mut buttons = prompt_buttons(
        ax,
        panel,
        identifier,
        require_ok_button,
        candidates,
        check_deadline,
    )?;
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

// Collect every button below the panel that carries the exact per-run prompt.
fn prompt_buttons<A: Accessibility>(
    ax: &A,
    panel: &A::Element,
    identifier: &str,
    require_ok_button: bool,
    candidates: &mut Vec<ConfirmCandidate>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Vec<A::Element>, ConfirmFailure> {
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
            // A host-attached sheet also needs the Open panel's own confirm button identifier.
            if ax.string(&element, "AXTitle").ok().as_deref() == Some(identifier)
                && (!require_ok_button || is_ok_button(ax, &element))
            {
                buttons.push(element);
            }
        } else if matches!(role.as_str(), "AXGroup" | "AXSplitGroup" | "AXScrollArea") {
            pending.extend(ax.elements(&element, "AXChildren").map_err(read)?);
        }
    }
    Ok(buttons)
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
