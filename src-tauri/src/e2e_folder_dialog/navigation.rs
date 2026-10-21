// Go to Folder entry and commit for the marked Open panel.
// The drive writes the path through AXValue and presses the sheet's own Go button or,
// on a buttonless sheet, confirms the path field. When AXConfirm leaves that sheet open,
// one Return pair goes to the process that owns the revalidated, focused path field.
use super::accessibility::{
    attached_host, focused_panel, navigation_sheet, Accessibility, ConfirmFailure, NavigationEvent,
};

// Action names and mutating Accessibility calls. Each mutation runs once and is never retried.
pub(super) trait AccessibilityActions: Accessibility {
    fn action_names(&self, element: &Self::Element) -> Result<Vec<String>, String>;
    fn set_value(&self, element: &Self::Element, value: &str) -> Result<(), String>;
    fn perform(&self, element: &Self::Element, action: &str) -> Result<(), String>;
    // Write AXFocused on the element.
    fn set_focused(&self, element: &Self::Element, focused: bool) -> Result<(), String>;
    // The process that owns the element, as Accessibility reports it.
    fn pid(&self, element: &Self::Element) -> Result<i32, String>;
    // Post one Return key event, down or up, to that process alone.
    fn post_return_key(&self, pid: i32, key_down: bool) -> Result<(), String>;
}

// Post one Return pair. The release is attempted once even when the press reports an
// error: a timed-out reply can still mean the press arrived, and a held Return would
// otherwise stay down in the field owner. Neither event is retried.
fn post_return<A: AccessibilityActions>(ax: &A, pid: i32) -> Result<(), String> {
    let down = ax.post_return_key(pid, true);
    let up = ax.post_return_key(pid, false);
    down.and(up)
}

const GO_TO_WINDOW: &str = "GoToWindow";
const PATH_FIELD: &str = "PathTextField";
// Identifiers AppKit gives the Go to Folder commit button. Labels are localized and never match.
const COMMIT_BUTTONS: [&str; 2] = ["OKButton", "GoButton"];
const SCAN_LIMIT: usize = 256;
// Settle ticks an open sheet gets after AXConfirm before the one Return fallback.
const CONFIRM_SETTLE: usize = 3;
const TRACE_LIMIT: usize = 16;

// The validated Go to Folder sheet, its editable path control and its commit control.
// The commit presses the sheet's Go button or, without one, confirms the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GoToFolder<E> {
    pub(super) sheet: E,
    pub(super) field: E,
    pub(super) commit: E,
    pub(super) action: &'static str,
}

fn is_container(role: &str) -> bool {
    matches!(role, "AXGroup" | "AXSplitGroup" | "AXScrollArea")
}

fn is_editable(role: &str) -> bool {
    matches!(role, "AXTextField" | "AXComboBox")
}

fn is_go_to_window<A: Accessibility>(ax: &A, element: &A::Element) -> bool {
    ax.string(element, "AXIdentifier").ok().as_deref() == Some(GO_TO_WINDOW)
        && matches!(
            ax.string(element, "AXRole").ok().as_deref(),
            Some("AXSheet" | "AXWindow")
        )
}

fn label_event<A: Accessibility>(
    ax: &A,
    element: &A::Element,
    stage: &'static str,
) -> NavigationEvent {
    // Read the role and identifier alone, never the editable value or a title.
    let label = |name| {
        ax.string(element, name)
            .ok()
            .map(|text| text.chars().take(128).collect())
    };
    NavigationEvent {
        stage,
        role: label("AXRole"),
        identifier: label("AXIdentifier"),
    }
}

// Collect GoToWindow sheets below root through layout containers.
// Skip the Open panel sheet itself when root is the panel's host window.
fn sheets_below<A: Accessibility>(
    ax: &A,
    root: &A::Element,
    skip: Option<&A::Element>,
    sheets: &mut Vec<A::Element>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<(), ConfirmFailure> {
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
        if visited.len() == SCAN_LIMIT {
            return Err(ElementLimit);
        }
        visited.push(element.clone());
        if is_go_to_window(ax, &element) {
            if !sheets.contains(&element) {
                sheets.push(element);
            }
            continue;
        }
        let role = ax
            .string(&element, "AXRole")
            .map_err(|_| AttributeUnavailable)?;
        if is_container(&role) {
            pending.extend(
                ax.elements(&element, "AXChildren")
                    .map_err(|_| AttributeUnavailable)?,
            );
        }
    }
    Ok(())
}

// Walk AXParent from a control to the sheet through layout containers and combo boxes.
fn owned_by<A: Accessibility>(
    ax: &A,
    control: &A::Element,
    sheet: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<(), ConfirmFailure> {
    use ConfirmFailure::*;
    let mut current = control.clone();
    let mut visited = vec![current.clone()];
    for _ in 0..SCAN_LIMIT {
        check_deadline()?;
        let parent = ax
            .element(&current, "AXParent")
            .map_err(|_| AttributeUnavailable)?;
        if parent == *sheet {
            return Ok(());
        }
        if visited.contains(&parent) {
            return Err(WrongTopLevelElement);
        }
        let role = ax
            .string(&parent, "AXRole")
            .map_err(|_| AttributeUnavailable)?;
        if !is_container(&role) && role != "AXComboBox" {
            return Err(WrongTopLevelElement);
        }
        visited.push(parent.clone());
        current = parent;
    }
    Err(ElementLimit)
}

struct SheetControls<E> {
    visited: Vec<E>,
    identified: Vec<E>,
    nested_identified: Vec<E>,
    editable: Vec<E>,
    buttons: Vec<E>,
}

// Scan one GoToWindow sheet. A control nested in a combo box counts only by identifier,
// and only when the sheet has no outer identified control.
fn sheet_controls<A: Accessibility>(
    ax: &A,
    sheet: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
    record: &mut impl FnMut(NavigationEvent),
) -> Result<SheetControls<A::Element>, ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let mut pending: Vec<_> = ax
        .elements(sheet, "AXChildren")
        .map_err(|_| AttributeUnavailable)?
        .into_iter()
        .map(|child| (child, false))
        .collect();
    let mut controls = SheetControls {
        visited: vec![sheet.clone()],
        identified: Vec::new(),
        nested_identified: Vec::new(),
        editable: Vec::new(),
        buttons: Vec::new(),
    };
    let mut traced = 0;
    while let Some((element, nested)) = pending.pop() {
        check_deadline()?;
        if controls.visited.contains(&element) {
            continue;
        }
        if controls.visited.len() == SCAN_LIMIT {
            return Err(ElementLimit);
        }
        controls.visited.push(element.clone());
        let role = ax
            .string(&element, "AXRole")
            .map_err(|_| AttributeUnavailable)?;
        let identified = ax.string(&element, "AXIdentifier").ok().as_deref() == Some(PATH_FIELD);
        if (is_editable(&role) || role == "AXButton") && traced < TRACE_LIMIT {
            traced += 1;
            record(label_event(ax, &element, "sheet_control"));
        }
        if identified {
            if !is_editable(&role) {
                return Err(WrongTopLevelElement);
            }
            if nested {
                controls.nested_identified.push(element.clone());
            } else {
                controls.identified.push(element.clone());
            }
        } else if is_editable(&role) && !nested {
            controls.editable.push(element.clone());
        }
        if role == "AXButton" {
            controls.buttons.push(element);
        } else if is_container(&role) || role == "AXComboBox" {
            check_deadline()?;
            let nested = nested || role == "AXComboBox";
            pending.extend(
                ax.elements(&element, "AXChildren")
                    .map_err(|_| AttributeUnavailable)?
                    .into_iter()
                    .map(|child| (child, nested)),
            );
        }
    }
    Ok(controls)
}

// The GoToWindow that contains the focused control, if any. Bounded by the scan limit.
fn focused_sheet<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<Option<A::Element>, ConfirmFailure> {
    check_deadline()?;
    let Ok(mut current) = ax.element(app, "AXFocusedUIElement") else {
        return Ok(None);
    };
    let mut visited = vec![current.clone()];
    for _ in 0..SCAN_LIMIT {
        check_deadline()?;
        let Ok(parent) = ax.element(&current, "AXParent") else {
            return Ok(None);
        };
        if visited.contains(&parent) {
            return Err(ConfirmFailure::WrongTopLevelElement);
        }
        if is_go_to_window(ax, &parent) {
            return Ok(Some(parent));
        }
        let Ok(role) = ax.string(&parent, "AXRole") else {
            return Ok(None);
        };
        if !is_container(&role) && !is_editable(&role) {
            return Ok(None);
        }
        visited.push(parent.clone());
        current = parent;
    }
    Err(ConfirmFailure::ElementLimit)
}

fn single<E>(mut items: Vec<E>) -> Result<Option<E>, ConfirmFailure> {
    if items.len() > 1 {
        return Err(ConfirmFailure::AmbiguousNavigation);
    }
    Ok(items.pop())
}

// Resolve the focused GoToWindow sheet of the marked panel, its single editable path
// control and its Go button. The sheet is focused itself, nested in the panel, or
// attached to the focused window that hosts the panel. A role or label alone never matches.
pub(super) fn go_to_folder<A: AccessibilityActions>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
    mut record: impl FnMut(NavigationEvent),
) -> Result<Option<GoToFolder<A::Element>>, ConfirmFailure> {
    use ConfirmFailure::*;
    let check_deadline = &mut check_deadline;
    let record = &mut record;
    check_deadline()?;
    let Ok(window) = ax.element(app, "AXFocusedWindow") else {
        return Ok(None);
    };
    let mut sheets = Vec::new();
    check_deadline()?;
    if window != *panel && is_go_to_window(ax, &window) {
        // A focused GoToWindow belongs to the panel directly, through the panel's host,
        // or as a listed child of the panel.
        let parent = ax.element(&window, "AXParent").ok();
        let owned = match &parent {
            Some(parent) if parent == panel => true,
            Some(parent) => attached_host(ax, parent, panel, check_deadline)?,
            None => false,
        };
        if !owned {
            sheets_below(ax, panel, None, &mut sheets, check_deadline)?;
            if !sheets.contains(&window) {
                // Record the foreign owner's role and identifier for the failure report.
                if let Some(parent) = &parent {
                    record(label_event(ax, parent, "sheet_owner"));
                }
                return Err(WrongTopLevelElement);
            }
        }
        sheets = vec![window.clone()];
    } else {
        let host = if window == *panel {
            None
        } else if attached_host(ax, &window, panel, check_deadline)? {
            Some(window.clone())
        } else {
            return Ok(None);
        };
        sheets_below(ax, panel, None, &mut sheets, check_deadline)?;
        if let Some(host) = &host {
            sheets_below(ax, host, Some(panel), &mut sheets, check_deadline)?;
        }
        if sheets.is_empty() {
            // A view bridge can omit the sheet from the panel's children. Follow the focused
            // control up to its GoToWindow, which must name the panel or its host as parent.
            if let Some(sheet) = focused_sheet(ax, app, check_deadline)? {
                check_deadline()?;
                let parent = ax
                    .element(&sheet, "AXParent")
                    .map_err(|_| AttributeUnavailable)?;
                if parent != *panel && host.as_ref() != Some(&parent) {
                    return Err(WrongTopLevelElement);
                }
                sheets.push(sheet);
            }
        }
    }
    let Some(sheet) = single(sheets)? else {
        return Ok(None);
    };
    let controls = sheet_controls(ax, &sheet, check_deadline, record)?;
    let field = match single(controls.identified)? {
        Some(field) => field,
        None => match single(controls.nested_identified)? {
            Some(field) => field,
            None => match single(controls.editable)? {
                Some(field) => field,
                None => return Ok(None),
            },
        },
    };
    owned_by(ax, &field, &sheet, check_deadline)?;
    check_deadline()?;
    if !ax
        .boolean(&field, "AXEnabled")
        .map_err(|_| AttributeUnavailable)?
    {
        return Ok(None);
    }
    // Only a button identified as a commit button counts, default or not. A default button
    // with another identifier, such as CloseButton, is never pressed. A sheet without a
    // commit button commits through the field's own confirm action.
    check_deadline()?;
    let is_commit = |button: &A::Element| {
        ax.string(button, "AXIdentifier")
            .ok()
            .is_some_and(|id| COMMIT_BUTTONS.contains(&id.as_str()))
    };
    let default = match ax.element(&sheet, "AXDefaultButton") {
        Ok(button) if !controls.buttons.contains(&button) => return Err(WrongTopLevelElement),
        Ok(button) if is_commit(&button) => Some(button),
        _ => None,
    };
    let button = match default {
        Some(button) => Some(button),
        None => single(
            controls
                .buttons
                .iter()
                .filter(|button| is_commit(button))
                .cloned()
                .collect(),
        )?,
    };
    let (commit, action) = match button {
        Some(button) => {
            owned_by(ax, &button, &sheet, check_deadline)?;
            check_deadline()?;
            if ax.string(&button, "AXRole").ok().as_deref() != Some("AXButton") {
                return Err(WrongButtonRole);
            }
            if !ax
                .boolean(&button, "AXEnabled")
                .map_err(|_| AttributeUnavailable)?
            {
                return Ok(None);
            }
            (button, "AXPress")
        }
        None => {
            check_deadline()?;
            let actions = ax.action_names(&field).map_err(|_| AttributeUnavailable)?;
            if !actions.iter().any(|action| action == "AXConfirm") {
                return Ok(None);
            }
            (field.clone(), "AXConfirm")
        }
    };
    // Keyboard focus must be positively inside this sheet: a scanned control of the sheet
    // or an element whose AXParent chain reaches it. Missing focus attributes, focus on
    // the panel or its host, and focus in another app all withhold the target.
    check_deadline()?;
    let Ok(focused) = ax.element(app, "AXFocusedUIElement") else {
        return Ok(None);
    };
    if !controls.visited.contains(&focused)
        && focused_sheet(ax, app, check_deadline)?.as_ref() != Some(&sheet)
    {
        return Ok(None);
    }
    check_deadline()?;
    if ax.element(app, "AXFocusedWindow").ok().as_ref() != Some(&window) {
        return Ok(None);
    }
    // Read AXFrontmost last so it is the freshest check before a mutation.
    check_deadline()?;
    if ax.boolean(app, "AXFrontmost").ok() != Some(true) {
        return Ok(None);
    }
    Ok(Some(GoToFolder {
        sheet,
        field,
        commit,
        action,
    }))
}

// Deliver the one Return fallback for a sheet that AXConfirm left open. The sheet, field
// ownership, focus and foreground state must still match the committed target, the field
// must accept AXFocused and hold keyboard focus, and its owner must report a positive pid.
// Returns false when any check withholds delivery. The caller never calls this twice.
fn return_fallback<A: AccessibilityActions>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    target: &GoToFolder<A::Element>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
    record: &mut impl FnMut(NavigationEvent),
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    if !still_target(ax, app, panel, target, check_deadline, record)? {
        return Ok(false);
    }
    check_deadline()?;
    if ax.set_focused(&target.field, true).is_err()
        || !field_focused(ax, app, &target.field, check_deadline)?
    {
        return Ok(false);
    }
    check_deadline()?;
    let pid = match ax.pid(&target.field) {
        Ok(pid) if pid > 0 => pid,
        _ => return Ok(false),
    };
    // The focus write can move ownership or foreground state. Recheck both, then focus.
    if !still_target(ax, app, panel, target, check_deadline, record)?
        || !field_focused(ax, app, &target.field, check_deadline)?
    {
        return Ok(false);
    }
    check_deadline()?;
    post_return(ax, pid).map_err(|_| CommitFailed)?;
    Ok(true)
}

// The live Go to Folder resolution still names the committed sheet, field and control.
fn still_target<A: AccessibilityActions>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    target: &GoToFolder<A::Element>,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
    record: &mut impl FnMut(NavigationEvent),
) -> Result<bool, ConfirmFailure> {
    match go_to_folder(ax, app, panel, &mut *check_deadline, &mut *record) {
        Ok(live) => Ok(live.as_ref() == Some(target)),
        Err(ConfirmFailure::AttributeUnavailable) => Ok(false),
        Err(reason) => Err(reason),
    }
}

fn field_focused<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    field: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    check_deadline()?;
    Ok(ax.element(app, "AXFocusedUIElement").ok().as_ref() == Some(field))
}

// Enter the path into the validated Go to Folder control and press its Go button once.
// A buttonless sheet that AXConfirm leaves open gets one owner-targeted Return.
// Success requires the sheet to close and the marked panel to own focus again.
// The caller still gates confirmation on the panel's canonical directory.
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_and_commit<A: AccessibilityActions>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    panel_button: &A::Element,
    path: &str,
    mut check_deadline: impl FnMut() -> Result<(), ConfirmFailure>,
    mut settle: impl FnMut(),
    mut record: impl FnMut(NavigationEvent),
) -> Result<(), ConfirmFailure> {
    use ConfirmFailure::*;
    let target = loop {
        check_deadline()?;
        record(NavigationEvent::focused_window(ax, app));
        match go_to_folder(ax, app, panel, &mut check_deadline, &mut record) {
            Ok(Some(target)) => break target,
            Ok(None) | Err(AttributeUnavailable) => {}
            Err(reason) => return Err(reason),
        }
        settle();
    };
    record(label_event(ax, &target.sheet, "sheet_found"));
    record(NavigationEvent::stage("field_found"));
    // Recheck the live sheet, focus and ownership right before the write.
    let live = go_to_folder(ax, app, panel, &mut check_deadline, &mut record)?;
    if live.as_ref() != Some(&target) {
        return Err(KeyboardFocusLost);
    }
    check_deadline()?;
    ax.set_value(&target.field, path)
        .map_err(|_| ValueRejected)?;
    record(NavigationEvent::stage("value_set"));
    // Let the remote field process its edit before the value check and the commit.
    settle();
    check_deadline()?;
    // Compare the value without recording it. Diagnostics never carry editable contents.
    if ax.string(&target.field, "AXValue").ok().as_deref() != Some(path) {
        return Err(ValueRejected);
    }
    let live = go_to_folder(ax, app, panel, &mut check_deadline, &mut record)?;
    if live.as_ref() != Some(&target) {
        return Err(KeyboardFocusLost);
    }
    check_deadline()?;
    // One commit. A repeated commit could act on the panel after the sheet closes.
    ax.perform(&target.commit, target.action)
        .map_err(|_| CommitFailed)?;
    record(NavigationEvent::stage("go_committed"));
    let mut fallback = target.action == "AXConfirm";
    let mut waited = 0;
    loop {
        check_deadline()?;
        record(NavigationEvent::focused_window(ax, app));
        if sheet_closed(ax, app, panel, &target.sheet, &mut check_deadline)? {
            record(NavigationEvent::stage("sheet_closed"));
            if focused_panel(ax, app, panel_button, &mut check_deadline)
                .ok()
                .as_ref()
                == Some(panel)
            {
                record(NavigationEvent::stage("panel_refocused"));
                return Ok(());
            }
        } else if fallback && waited >= CONFIRM_SETTLE {
            // One attempt only. A withheld or ignored Return leaves the deadline in charge.
            fallback = false;
            let posted =
                return_fallback(ax, app, panel, &target, &mut check_deadline, &mut record)?;
            let stage = if posted {
                "return_posted"
            } else {
                "return_withheld"
            };
            record(NavigationEvent::stage(stage));
        }
        waited += 1;
        settle();
    }
}

// Closure needs positive evidence for the resolved sheet itself. Focus returning to the
// panel or its host is not enough: an unlisted view-bridge sheet can stay open while
// window and keyboard focus sit on the host, and no child list shows it. The resolved
// sheet must fail to read as a GoToWindow because AppKit destroyed it, and any focused
// element must not lead through AXParent into a GoToWindow.
fn sheet_closed<A: Accessibility>(
    ax: &A,
    app: &A::Element,
    panel: &A::Element,
    sheet: &A::Element,
    check_deadline: &mut impl FnMut() -> Result<(), ConfirmFailure>,
) -> Result<bool, ConfirmFailure> {
    use ConfirmFailure::*;
    check_deadline()?;
    let Ok(window) = ax.element(app, "AXFocusedWindow") else {
        return Ok(false);
    };
    if window == *sheet || is_go_to_window(ax, &window) {
        return Ok(false);
    }
    match navigation_sheet(ax, app, panel, &mut *check_deadline) {
        Ok(false) => {}
        Ok(true) | Err(AttributeUnavailable) => return Ok(false),
        Err(reason) => return Err(reason),
    }
    let host = window != *panel && attached_host(ax, &window, panel, check_deadline)?;
    check_deadline()?;
    if let Ok(mut current) = ax.element(app, "AXFocusedUIElement") {
        let mut visited = Vec::new();
        loop {
            check_deadline()?;
            if current == *sheet || is_go_to_window(ax, &current) || visited.contains(&current) {
                return Ok(false);
            }
            if current == *panel || (host && current == window) {
                break;
            }
            if visited.len() == SCAN_LIMIT {
                return Err(ElementLimit);
            }
            let Ok(parent) = ax.element(&current, "AXParent") else {
                break;
            };
            visited.push(current);
            current = parent;
        }
    }
    check_deadline()?;
    Ok(!is_go_to_window(ax, sheet))
}
