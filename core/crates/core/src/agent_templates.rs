//! Agent templates: Grok Bot share links, Grok Bot JSON and Markdown, and
//! Muniment agent files, read into an import preview and written back out.
//!
//! Share pages are read as inert data, bounded, with no account cookies or
//! app activation, and their scripts never run. A template's routines arrive
//! as disabled proposals, its skills match installed skills by name, its
//! plugins and tools match installed extensions or become setup
//! requirements, and its memories go to the agent's memory folder. Exports
//! carry no secrets: no webhook secret or hook id, no machine binding and no
//! private memory.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
pub fn public_url(input: &str) -> Result<String, String> {
    let url = url::Url::parse(input.trim()).map_err(|_| "Paste a public x.ai/bot share link.")?;
    let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
    if url.scheme() != "https"
        || url.host_str() != Some("x.ai")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || parts.len() < 2
        || parts.len() > 3
        || parts[0] != "bot"
        || parts[1].len() != 21
        || !parts[1]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("Paste a public x.ai/bot share link.".into());
    }
    Ok(format!("https://x.ai/bot/{}", parts[1]))
}
pub fn fetch_public_template(input: &str) -> Result<serde_json::Value, String> {
    let url = public_url(input)?;
    let response = crate::http::agent_builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirects(0)
        .build()
        .get(&url)
        .set("Accept", "text/html")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => {
                "This template was not found. It may be private or deleted.".to_string()
            }
            ureq::Error::Status(401 | 403, _) => {
                "This template is not public. Export its configuration from Grok Bot instead."
                    .to_string()
            }
            _ => "The Grok template could not be loaded. Check the link and try again.".to_string(),
        })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The template response could not be read.")?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("The template page is too large.".into());
    }
    let html = String::from_utf8(bytes).map_err(|_| "The template page is not valid text.")?;
    Ok(serde_json::json!({ "url": url, "html": html }))
}
/// The largest template file or pasted text.
pub const MAX_TEMPLATE_BYTES: usize = 128 * 1024;
/// The Muniment agent file version this core writes. It reads 1 to 3.
pub const MUNIMENT_FORMAT_VERSION: u64 = 3;
const MUNIMENT_FORMAT: &str = "muniment-agent";
const SECTIONS: [&str; 5] = ["skills", "routines", "plugins", "memories", "settings"];
const MAX_IMPORTED_MEMORIES: usize = 64;
const MAX_SKILL_TEXT_BYTES: usize = 16 * 1024;
/// The prefix of the ids of routines a Grok template proposes.
pub const IMPORT_PREFIX: &str = "import-";

/// The skills, plugins and MCP servers installed on this computer, as an
/// import matches a template's skills and plugins against them.
#[derive(Clone, Debug, Default)]
pub struct Installed {
    /// `(id, name, file)` of each enabled skill. The id is what an agent's
    /// `skills` list names.
    pub skills: Vec<(String, String, Option<std::path::PathBuf>)>,
    /// `(id, name, kind)` of each enabled plugin and MCP server.
    pub extensions: Vec<(String, String, String)>,
}

impl Installed {
    /// The enabled extensions in the extension inventory under `root`.
    pub fn read(root: &std::path::Path) -> Self {
        let Ok(state) = crate::extend::inventory(root) else {
            return Self::default();
        };
        let mut installed = Self::default();
        for item in state["items"].as_array().into_iter().flatten() {
            let id = item["id"].as_str().unwrap_or("");
            if id.is_empty() || item["enabled"] == false {
                continue;
            }
            let kind = item["kind"].as_str().unwrap_or("");
            let name = item["name"].as_str().unwrap_or("").to_owned();
            if matches!(kind, "plugin" | "mcp") && !name.is_empty() {
                installed
                    .extensions
                    .push((id.to_owned(), name, kind.to_owned()));
            }
            let base = std::path::Path::new(item["base"].as_str().unwrap_or(""));
            for skill in item["skills"].as_array().into_iter().flatten() {
                let (Some(path), Some(skill_name)) =
                    (skill["path"].as_str(), skill["name"].as_str())
                else {
                    continue;
                };
                installed.skills.push((
                    format!("{id}:{path}"),
                    skill_name.to_owned(),
                    (!base.as_os_str().is_empty()).then(|| base.join(path)),
                ));
            }
        }
        installed
    }

    /// The extensions installed on this computer.
    pub fn local() -> Self {
        crate::state_root::state_directory()
            .map(|root| Self::read(&root))
            .unwrap_or_default()
    }
}

/// What an import found and how it maps onto this computer, before the user
/// saves it. `agent` is a draft for `agent.import_commit`.
#[derive(Clone, Debug, Serialize)]
pub struct ImportPreview {
    pub agent: crate::agents::Agent,
    pub memories: Vec<ImportedMemory>,
    pub skills: SkillMapping,
    pub tools: ToolMapping,
    /// Template routines no trigger fits. They stay in the template as notes.
    pub routine_notes: Vec<Value>,
    /// `grok_link`, `grok_json`, `markdown` or `muniment`.
    pub source: &'static str,
    /// `public-profile` for a share page, which carries only the profile;
    /// `provided` for a supplied file.
    pub coverage: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedMemory {
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SkillMapping {
    /// Template skills that name an installed skill: `{name, id}`.
    pub matched: Vec<Value>,
    /// Template skills with no installed match: `{name, instructions}`. They
    /// stay in the template as reference text, not installs.
    pub proposals: Vec<Value>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ToolMapping {
    /// Template plugins and tools that name an installed extension:
    /// `{name, id, kind}`.
    pub matched: Vec<Value>,
    /// Template plugins and tools to set up before use: `{name}`.
    pub setup: Vec<Value>,
}

fn bounded_text(content: &str) -> Result<(), String> {
    if content.len() > MAX_TEMPLATE_BYTES {
        return Err("Choose a template under 128 KB.".into());
    }
    Ok(())
}

/// The JSON in the first fenced `json` block of `text`.
fn fenced_json(text: &str) -> Option<&str> {
    let start = text.find("```json")?;
    let rest = &text[start + 7..];
    let newline = rest.find('\n')?;
    if !rest[..newline].trim().is_empty() {
        return None;
    }
    let body = &rest[newline + 1..];
    let end = body.find("\n```")?;
    Some(&body[..end])
}

/// Reads pasted text or a file's content: a share link, Grok JSON, a
/// Muniment agent file, or Markdown.
pub fn import(input: &str) -> Result<ImportPreview, String> {
    let installed = Installed::local();
    let tz = crate::agent_routines::local_time_zone();
    if input.trim().starts_with("https://") {
        let page = fetch_public_template(input)?;
        return import_grok_page(
            page["url"].as_str().unwrap_or(""),
            page["html"].as_str().unwrap_or(""),
            &installed,
            &tz,
        );
    }
    import_text(input, &installed, &tz)
}

/// Reads a template's text as `import` does, against `installed`.
pub fn import_text(
    content: &str,
    installed: &Installed,
    tz: &str,
) -> Result<ImportPreview, String> {
    bounded_text(content)?;
    let text = content.trim();
    let config = fenced_json(text);
    let (raw, source) = if text.starts_with('{') || config.is_some() {
        let raw: Value = serde_json::from_str(config.unwrap_or(text))
            .map_err(|_| "The template is not valid JSON.")?;
        let kind = if config.is_some() {
            "markdown"
        } else {
            "grok_json"
        };
        (raw, kind)
    } else {
        let Some(first) = text.lines().next().and_then(|line| line.strip_prefix("# ")) else {
            return Err(
                "Choose a JSON template or Markdown with a name heading and description.".into(),
            );
        };
        let description = text[text.find('\n').unwrap_or(text.len())..].trim();
        (
            json!({"name": first.trim(), "description": description}),
            "markdown",
        )
    };
    let source = if raw["format"] == MUNIMENT_FORMAT {
        "muniment"
    } else {
        source
    };
    map_template(raw, source, "provided", None, installed, tz)
}

/// Reads the profile in a public share page's serialized page data. The
/// page's scripts never run and its HTML is never kept.
pub fn import_grok_page(
    url: &str,
    html: &str,
    installed: &Installed,
    tz: &str,
) -> Result<ImportPreview, String> {
    let url = public_url(url)?;
    let id = url.rsplit('/').next().unwrap_or("");
    let found = page_profile(html, id).ok_or(
        "The public profile could not be read. The template may be private, deleted, or use a new format.",
    )?;
    let text = serde_json::to_string(&found).map_err(|_| "The template could not be read.")?;
    bounded_text(&text)?;
    map_template(
        found,
        "grok_link",
        "public-profile",
        Some(&url),
        installed,
        tz,
    )
}

/// The share page's bot profile: the object with the bot's id, a
/// `botName` and a `description`, in the page's serialized data chunks.
fn page_profile(html: &str, id: &str) -> Option<Value> {
    let lower = html.to_ascii_lowercase();
    let mut chunks = String::new();
    let mut from = 0;
    while let Some(open) = lower[from..].find("<script") {
        let open = from + open;
        let Some(body) = lower[open..].find('>').map(|end| open + end + 1) else {
            break;
        };
        let close = lower[body..]
            .find("</script>")
            .map_or(html.len(), |end| body + end);
        let script = &html[body..close];
        let marker = "self.__next_f.push(";
        let mut at = 0;
        while let Some(found) = script[at..].find(marker) {
            let start = at + found + marker.len();
            at = start;
            // The row is the shortest `[...]` before `)` that parses.
            let mut search = start;
            for _ in 0..64 {
                let Some(end) = script[search..].find("])") else {
                    break;
                };
                let end = search + end + 1;
                if let Ok(row) = serde_json::from_str::<Value>(&script[start..end]) {
                    if let Some(chunk) = row.get(1).and_then(Value::as_str) {
                        chunks.push_str(chunk);
                    }
                    break;
                }
                search = end;
            }
        }
        from = close;
    }
    let mut found = None;
    for line in chunks.split('\n') {
        let Some(colon) = line.find(':') else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(&line[colon + 1..]) {
            walk_profile(&value, id, &mut found);
        }
    }
    found
}

fn walk_profile(value: &Value, id: &str, found: &mut Option<Value>) {
    match value {
        Value::Object(fields) => {
            if fields.get("id").and_then(Value::as_str) == Some(id)
                && fields.get("botName").is_some_and(Value::is_string)
                && fields.get("description").is_some_and(Value::is_string)
            {
                *found = Some(value.clone());
            }
            fields
                .values()
                .for_each(|value| walk_profile(value, id, found));
        }
        Value::Array(items) => items
            .iter()
            .for_each(|value| walk_profile(value, id, found)),
        _ => {}
    }
}

fn text_field<'a>(source: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
}

/// Maps a template's JSON onto a draft agent and the mappings the preview
/// shows.
fn map_template(
    raw: Value,
    source_kind: &'static str,
    coverage: &'static str,
    url: Option<&str>,
    installed: &Installed,
    tz: &str,
) -> Result<ImportPreview, String> {
    if !raw.is_object() {
        return Err("The template must contain an agent profile.".into());
    }
    let muniment = raw["format"] == MUNIMENT_FORMAT;
    if muniment
        && !raw["version"]
            .as_u64()
            .is_some_and(|version| (1..=MUNIMENT_FORMAT_VERSION).contains(&version))
    {
        return Err("This template version is not supported.".into());
    }
    let source = if raw.get("name").is_none() && raw.get("botName").is_none() {
        raw.get("bot")
            .or_else(|| raw.get("template"))
            .filter(|inner| inner.is_object())
            .unwrap_or(&raw)
    } else {
        &raw
    };
    let name = text_field(source, &["name", "botName"])
        .unwrap_or("")
        .trim();
    let instructions = text_field(source, &["instructions", "description"])
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("");
    let label = text_field(source, &["label", "jobTitle"])
        .unwrap_or("")
        .trim();
    if name.is_empty() || name.len() > 100 || instructions.trim().is_empty() {
        return Err("The template needs a name and description.".into());
    }
    if label.len() > 120 {
        return Err("Keep the job title under 120 bytes.".into());
    }
    if instructions.len() > 64 * 1024 {
        return Err("The description exceeds 64 KB.".into());
    }
    let avatar = serde_json::from_value::<crate::agents::Avatar>(source["avatar"].clone())
        .ok()
        .filter(|avatar| {
            matches!(avatar.style.as_str(), "muniment-v1" | "muniment-v2")
                && !avatar.seed.is_empty()
                && avatar.seed.len() <= 128
        });
    let mut agent = crate::agents::Agent {
        name: name.to_owned(),
        label: label.to_owned(),
        instructions: instructions.to_owned(),
        avatar,
        ..Default::default()
    };
    let mut preview = ImportPreview {
        agent: crate::agents::Agent::default(),
        memories: Vec::new(),
        skills: SkillMapping::default(),
        tools: ToolMapping::default(),
        routine_notes: Vec::new(),
        source: source_kind,
        coverage,
    };
    if muniment {
        map_muniment(source, &mut agent, &mut preview, tz)?;
    } else {
        let mut template = json!({"source": "imported", "coverage": coverage, "original": raw});
        for key in SECTIONS {
            if let Some(section) = source.get(key).filter(|value| !value.is_null()) {
                template[key] = section.clone();
            }
        }
        if let Some(url) = url {
            template["source"] = json!("grok");
            template["sourceUrl"] = json!(url);
        }
        map_skills(source, &mut agent, &mut preview, installed);
        map_tools(source, &mut preview, installed);
        map_routines(source, &mut agent, &mut preview, tz);
        preview.memories = memories(source);
        template["routine_notes"] = json!(preview.routine_notes);
        if preview.routine_notes.is_empty() {
            template
                .as_object_mut()
                .map(|fields| fields.remove("routine_notes"));
        }
        agent.template = Some(template);
    }
    preview.agent = agent;
    Ok(preview)
}

/// A Muniment agent file: v1 and v2 carry the profile, a schedule and the
/// template; v3 adds the agent's settings, routines and memories.
fn map_muniment(
    source: &Value,
    agent: &mut crate::agents::Agent,
    preview: &mut ImportPreview,
    tz: &str,
) -> Result<(), String> {
    let template = source
        .get("template")
        .cloned()
        .filter(|value| !value.is_null());
    if template.as_ref().is_some_and(|value| !value.is_object()) {
        return Err("The template context must be an object.".into());
    }
    agent.template = template;
    if let Some(schedule) = source.get("schedule").filter(|value| !value.is_null()) {
        let schedule: crate::agents::Schedule = serde_json::from_value(schedule.clone())
            .map_err(|_| "The template schedule is invalid.")?;
        let routine = crate::agent_routines::schedule_routine(&schedule, tz)
            .map_err(|_| "The template schedule is invalid.")?;
        agent.routines.push(routine);
    }
    if let Some(settings) = source.get("agent").filter(|value| value.is_object()) {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Settings {
            #[serde(default)]
            model: Option<crate::agents::AgentModel>,
            #[serde(default)]
            tools: Option<crate::agents::AgentTools>,
            #[serde(default)]
            skills: Option<Vec<String>>,
            #[serde(default)]
            permissions: Option<crate::agents::AgentPermissions>,
            #[serde(default)]
            messaging: Option<crate::agents::AgentMessaging>,
            #[serde(default)]
            routines: Vec<crate::agent_routines::Routine>,
        }
        let settings: Settings = serde_json::from_value(settings.clone())
            .map_err(|_| "The agent settings in the template are invalid.")?;
        agent.model = settings.model;
        agent.tools = settings.tools;
        agent.skills = settings.skills;
        agent.permissions = settings.permissions;
        agent.messaging = settings.messaging;
        agent.routines.extend(settings.routines);
    }
    for routine in &mut agent.routines {
        portable_routine(routine);
        routine.proposed = true;
    }
    crate::agent_routines::validate_routines(&agent.routines)?;
    preview.memories = memories(source);
    Ok(())
}

/// A routine as it leaves or enters a machine: disabled, with no runtime
/// state, no hook id and no secret hash. A proposal mark is set by import.
fn portable_routine(routine: &mut crate::agent_routines::Routine) {
    routine.enabled = false;
    routine.proposed = false;
    routine.last_fired_at = None;
    routine.last_result = None;
    routine.last_poll = None;
    if let crate::agent_routines::Trigger::Webhook(hook) = &mut routine.trigger {
        hook.hook_id.clear();
        hook.secret_sha256 = None;
    }
}

/// The name and instruction text of a template skill: a string, or an
/// object with a name and instructions, content or a description.
fn skill_entry(skill: &Value) -> Option<(String, String)> {
    match skill {
        Value::String(name) => Some((name.trim().to_owned(), String::new())),
        Value::Object(_) => {
            let name = text_field(skill, &["name", "title"])?.trim().to_owned();
            let text = text_field(skill, &["instructions", "content", "prompt", "description"])
                .unwrap_or("")
                .to_owned();
            Some((name, text))
        }
        _ => None,
    }
    .filter(|(name, _)| !name.is_empty())
}

fn map_skills(
    source: &Value,
    agent: &mut crate::agents::Agent,
    preview: &mut ImportPreview,
    installed: &Installed,
) {
    let mut ids = Vec::new();
    for skill in source["skills"].as_array().into_iter().flatten() {
        let Some((name, instructions)) = skill_entry(skill) else {
            continue;
        };
        match installed
            .skills
            .iter()
            .find(|(_, installed, _)| installed.eq_ignore_ascii_case(&name))
        {
            Some((id, _, _)) => {
                preview.skills.matched.push(json!({"name": name, "id": id}));
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
            None => preview
                .skills
                .proposals
                .push(json!({"name": name, "instructions": instructions})),
        }
    }
    if !ids.is_empty() {
        agent.skills = Some(ids);
    }
}

fn map_tools(source: &Value, preview: &mut ImportPreview, installed: &Installed) {
    for key in ["plugins", "tools"] {
        for entry in source[key].as_array().into_iter().flatten() {
            let name = match entry {
                Value::String(name) => name.trim(),
                _ => text_field(entry, &["name", "id", "title"])
                    .unwrap_or("")
                    .trim(),
            };
            if name.is_empty() {
                continue;
            }
            let wanted = name.to_lowercase();
            match installed.extensions.iter().find(|(id, installed, _)| {
                installed.to_lowercase() == wanted || id.to_lowercase() == wanted
            }) {
                Some((id, _, kind)) => preview
                    .tools
                    .matched
                    .push(json!({"name": name, "id": id, "kind": kind})),
                None => preview.tools.setup.push(json!({"name": name})),
            }
        }
    }
}

/// A template routine's schedule as a cron schedule: a cron string, an
/// object with `cron` (and `timezone`), or a Muniment v1 schedule.
fn routine_schedule(entry: &Value, tz: &str) -> Option<crate::agent_routines::CronSchedule> {
    let schedule = entry.get("schedule").unwrap_or(&Value::Null);
    let zone = text_field(schedule, &["timezone", "tz"])
        .or_else(|| text_field(entry, &["timezone", "tz"]))
        .unwrap_or(tz);
    let expr = text_field(entry, &["cron"])
        .or_else(|| schedule.as_str())
        .or_else(|| text_field(schedule, &["cron", "expr"]));
    let schedule = match expr {
        Some(expr) => crate::agent_routines::CronSchedule {
            expr: expr.trim().to_owned(),
            tz: zone.to_owned(),
        },
        None => {
            let v1: crate::agents::Schedule = serde_json::from_value(schedule.clone()).ok()?;
            match crate::agent_routines::schedule_routine(&v1, zone)
                .ok()?
                .trigger
            {
                crate::agent_routines::Trigger::Cron(cron) => cron,
                _ => return None,
            }
        }
    };
    crate::agent_routines::validate_cron(&schedule).ok()?;
    Some(schedule)
}

/// The trigger an event routine's description fits: a webhook from the
/// service it names. Answers `None` when no trigger fits.
fn event_trigger(description: &str) -> Option<crate::agent_routines::Trigger> {
    use crate::agent_routines::{Trigger, WebhookPreset, WebhookTrigger};
    let lower = description.to_lowercase();
    let preset = if lower.contains("github") {
        WebhookPreset::Github
    } else if lower.contains("forgejo") || lower.contains("gitea") {
        WebhookPreset::Forgejo
    } else if lower.contains("webhook") {
        WebhookPreset::Generic
    } else {
        return None;
    };
    Some(Trigger::Webhook(WebhookTrigger {
        hook_id: String::new(),
        preset,
        secret_sha256: None,
    }))
}

fn map_routines(
    source: &Value,
    agent: &mut crate::agents::Agent,
    preview: &mut ImportPreview,
    tz: &str,
) {
    use crate::agent_routines::{CreatedFrom, Routine, Trigger};
    let entries: Vec<&Value> = source["routines"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    for (index, entry) in entries.into_iter().enumerate() {
        let text = match entry {
            Value::String(text) => text.trim().to_owned(),
            _ => text_field(entry, &["prompt", "instructions", "task", "description"])
                .unwrap_or("")
                .trim()
                .to_owned(),
        };
        let name = text_field(entry, &["name", "title"])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        let explicit = entry
            .get("trigger")
            .and_then(|trigger| serde_json::from_value::<Trigger>(trigger.clone()).ok());
        let event = text_field(entry, &["event", "when"]).unwrap_or("");
        let found = match (explicit, routine_schedule(entry, tz)) {
            (Some(trigger), _) => Some((trigger, text.clone())),
            (None, Some(schedule)) => Some((Trigger::Cron(schedule), text.clone())),
            (None, None) if !event.is_empty() => {
                event_trigger(event).map(|trigger| (trigger, text.clone()))
            }
            // A routine written as a sentence reads like instructions.
            (None, None) => crate::agent_extract::propose(&text, tz, &[])
                .into_iter()
                .next()
                .map(|routine| (routine.trigger, routine.prompt)),
        };
        let Some((trigger, prompt)) = found else {
            preview.routine_notes.push(json!({
                "name": name.clone().unwrap_or_else(|| format!("Routine {}", index + 1)),
                "note": "No schedule, webhook or MCP trigger fits this routine. Set one up by hand.",
                "original": entry,
            }));
            continue;
        };
        let mut routine = Routine {
            id: String::new(),
            name: name.unwrap_or_else(|| {
                let words: String = prompt.chars().take(60).collect();
                if words.trim().is_empty() {
                    format!("Routine {}", index + 1)
                } else {
                    words
                }
            }),
            enabled: false,
            trigger,
            prompt,
            created_from: CreatedFrom::Instructions,
            last_fired_at: None,
            last_result: None,
            proposed: true,
            last_poll: None,
        };
        portable_routine(&mut routine);
        routine.proposed = true;
        {
            use sha2::{Digest, Sha256};
            let key =
                serde_json::to_string(&(&routine.trigger, &routine.prompt)).unwrap_or_default();
            routine.id = format!(
                "{IMPORT_PREFIX}{}",
                &hex::encode(Sha256::digest(key.as_bytes()))[..16]
            );
        }
        let duplicate = agent.routines.iter().any(|other| other.id == routine.id);
        if crate::agent_routines::validate_routine(&routine).is_ok()
            && !duplicate
            && agent.routines.len() < crate::agent_routines::MAX_ROUTINES
        {
            agent.routines.push(routine);
        } else if !duplicate {
            preview.routine_notes.push(json!({
                "name": routine.name,
                "note": "This routine's schedule or trigger is not valid here. Set one up by hand.",
                "original": entry,
            }));
        }
    }
}

/// A template's memories: strings, or objects with content or text and an
/// optional title. A memory that holds a credential is left out.
fn memories(source: &Value) -> Vec<ImportedMemory> {
    let mut out = Vec::new();
    for entry in source["memories"].as_array().into_iter().flatten() {
        let (title, content) = match entry {
            Value::String(text) => (None, text.trim().to_owned()),
            _ => (
                text_field(entry, &["title", "name"]).map(str::trim),
                text_field(entry, &["content", "text", "memory"])
                    .unwrap_or("")
                    .trim()
                    .to_owned(),
            ),
        };
        if content.is_empty() || crate::memory_secret::reject_memory_secret(&content).is_err() {
            continue;
        }
        let title = title
            .filter(|title| !title.is_empty() && !title.contains(['\r', '\n']))
            .map(|title| title.chars().take(100).collect())
            .unwrap_or_else(|| {
                let line = content.lines().next().unwrap_or("Imported memory");
                line.chars().take(60).collect::<String>()
            });
        out.push(ImportedMemory { title, content });
        if out.len() == MAX_IMPORTED_MEMORIES {
            break;
        }
    }
    out
}

/// Saves an import: the agent, then its memories in the agent's memory
/// folder as facts from outside the conversation.
pub fn commit(
    profile: &std::path::Path,
    agent: crate::agents::Agent,
    memories: &[ImportedMemory],
) -> Result<crate::agents::Agent, String> {
    if memories.len() > MAX_IMPORTED_MEMORIES {
        return Err(format!("Import at most {MAX_IMPORTED_MEMORIES} memories."));
    }
    for memory in memories {
        crate::memory_secret::reject_memory_secret(&memory.content)
            .map_err(|_| "Remove credentials from the imported memories.")?;
    }
    let mut agent = agent;
    agent.id.clear();
    agent.project_id = None;
    agent.primary_runtime = None;
    agent.paused_reason = None;
    let agent = crate::agents::save(profile, agent)?;
    let (home, _) = crate::agents::memory_paths(profile, &agent.id)?;
    for memory in memories {
        crate::memory_files::fact_save_in(
            profile,
            &home,
            crate::memory_files::Fact {
                id: String::new(),
                title: memory.title.clone(),
                content: memory.content.clone(),
                source: "Imported agent template".into(),
                trust: crate::memory_files::Trust::External,
            },
        )?;
    }
    Ok(agent)
}

/// The facts in an agent's own memory folder. The private folder that
/// automatic capture writes is never exported.
fn exported_memories(profile: &std::path::Path, agent_id: &str) -> Vec<ImportedMemory> {
    let Ok((home, _)) = crate::agents::memory_paths(profile, agent_id) else {
        return Vec::new();
    };
    let mut memories: Vec<ImportedMemory> = crate::memory_files::facts_in(&home)
        .unwrap_or_default()
        .into_iter()
        .filter(|fact| crate::memory_secret::reject_memory_secret(&fact.content).is_ok())
        .map(|fact| ImportedMemory {
            title: fact.title,
            content: fact.content,
        })
        .collect();
    memories.sort_by(|a, b| (&a.title, &a.content).cmp(&(&b.title, &b.content)));
    memories
}

/// A file name for an export, from the agent's name.
fn file_name(name: &str, suffix: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    let stem = stem.trim_matches('-');
    format!("{}{suffix}", if stem.is_empty() { "agent" } else { stem })
}

/// Exports an agent: `muniment` as a Muniment agent file another Muniment
/// imports without loss, `grok_bot` as Markdown setup notes with the
/// configuration in Grok Bot's terms. Answers `{format, file_name, content}`.
pub fn export(
    profile: &std::path::Path,
    agent_id: &str,
    format: &str,
    installed: &Installed,
) -> Result<Value, String> {
    let agent = crate::agents::read(profile, agent_id)?;
    let memories = exported_memories(profile, agent_id);
    let content = match format {
        "muniment" => export_muniment(&agent, &memories)?,
        "grok_bot" => export_grok(&agent, &memories, installed)?,
        _ => return Err("Choose the grok_bot or muniment format.".into()),
    };
    let file_name = file_name(
        &agent.name,
        if format == "muniment" {
            ".muniment-agent.json"
        } else {
            ".grok-bot.md"
        },
    );
    Ok(json!({"format": format, "file_name": file_name, "content": content}))
}

fn export_muniment(
    agent: &crate::agents::Agent,
    memories: &[ImportedMemory],
) -> Result<String, String> {
    let mut routines = agent.routines.clone();
    routines.iter_mut().for_each(portable_routine);
    let file = json!({
        "format": MUNIMENT_FORMAT,
        "version": MUNIMENT_FORMAT_VERSION,
        "name": agent.name,
        "label": agent.label,
        "description": agent.instructions,
        "avatar": agent.avatar,
        "template": agent.template,
        "agent": {
            "model": agent.model,
            "tools": agent.tools,
            "skills": agent.skills,
            "permissions": agent.permissions,
            "messaging": agent.messaging,
            "routines": routines,
        },
        "memories": memories,
    });
    let text =
        serde_json::to_string_pretty(&file).map_err(|_| "The agent could not be exported.")? + "\n";
    checked_export(text)
}

fn checked_export(text: String) -> Result<String, String> {
    bounded_text(&text)
        .map_err(|_| "The export is over 128 KB. Remove memories or template text.".to_string())?;
    crate::memory_secret::reject_memory_secret(&text)
        .map_err(|_| "The export would include a credential. Remove it from the agent first.")?;
    Ok(text)
}

/// The instruction text of an installed skill, bounded.
fn skill_text(path: &std::path::Path) -> String {
    use std::io::Read;
    let mut text = String::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file
            .take(MAX_SKILL_TEXT_BYTES as u64)
            .read_to_string(&mut text);
    }
    text
}

fn export_grok(
    agent: &crate::agents::Agent,
    memories: &[ImportedMemory],
    installed: &Installed,
) -> Result<String, String> {
    use crate::agent_routines::Trigger;
    let template = agent.template.clone().unwrap_or(Value::Null);
    let mut skills: Vec<Value> = Vec::new();
    for id in agent.skills.iter().flatten() {
        if let Some((_, name, path)) = installed.skills.iter().find(|(skill, _, _)| skill == id) {
            let instructions = path.as_deref().map(skill_text).unwrap_or_default();
            skills.push(json!({"name": name, "instructions": instructions}));
        }
    }
    for skill in template["skills"].as_array().into_iter().flatten() {
        if let Some((name, instructions)) = skill_entry(skill) {
            if !skills.iter().any(|entry| entry["name"] == name.as_str()) {
                skills.push(json!({"name": name, "instructions": instructions}));
            }
        }
    }
    let routines: Vec<Value> = agent
        .routines
        .iter()
        .map(|routine| {
            let mut portable = routine.clone();
            portable_routine(&mut portable);
            match &portable.trigger {
                Trigger::Cron(cron) => json!({"name": portable.name, "prompt": portable.prompt,
                    "schedule": {"cron": cron.expr, "timezone": cron.tz}}),
                Trigger::Webhook(hook) => json!({"name": portable.name, "prompt": portable.prompt,
                    "event": format!("When a {} webhook delivery arrives", match hook.preset {
                        crate::agent_routines::WebhookPreset::Github => "GitHub",
                        crate::agent_routines::WebhookPreset::Forgejo => "Forgejo",
                        crate::agent_routines::WebhookPreset::Generic => "generic",
                    }),
                    "trigger": portable.trigger}),
                Trigger::Mcp(mcp) => json!({"name": portable.name, "prompt": portable.prompt,
                    "event": format!("When the {} tool {} answers a result that passes the condition, checked on the schedule {} ({})",
                        mcp.server, mcp.tool, mcp.every.expr, mcp.every.tz),
                    "trigger": portable.trigger}),
            }
        })
        .collect();
    let mut config = json!({
        "botName": agent.name,
        "jobTitle": agent.label,
        "description": agent.instructions,
        "skills": skills,
        "routines": routines,
        "memories": memories.iter().map(|memory| json!({"title": memory.title, "content": memory.content})).collect::<Vec<_>>(),
    });
    for key in ["plugins", "settings"] {
        if let Some(section) = template.get(key).filter(|value| !value.is_null()) {
            config[key] = section.clone();
        }
    }
    if let Some(avatar) = &agent.avatar {
        config["avatar"] = json!(avatar);
    }
    let text = format!(
        "# Set up {} in Grok Bot\n\nCreate a new Bot from the configuration below. Set its name, job title and description. Treat the description as lasting instructions. Add each skill with its instructions, and keep the memories as reference. Review and configure the plugins with the user. Keep routines paused until the user turns them on. Account credentials, webhook secrets, conversation history and private memory are not included.\n\n## Configuration\n\n```json\n{}\n```\n",
        agent.name,
        serde_json::to_string_pretty(&config).map_err(|_| "The agent could not be exported.")?
    );
    checked_export(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confines_reads_to_public_share_pages() {
        assert_eq!(
            public_url("https://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5/dewey?tracking=1").unwrap(),
            "https://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5"
        );
        for url in [
            "http://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5",
            "https://x.ai.evil.test/bot/rfAHsaFrz6xHBMtUpxDi5",
            "https://user@x.ai/bot/rfAHsaFrz6xHBMtUpxDi5",
            "https://127.0.0.1/bot/rfAHsaFrz6xHBMtUpxDi5",
            "https://x.ai/bot/marketplace",
        ] {
            assert!(public_url(url).is_err(), "{url}");
        }
    }

    const URL: &str = "https://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5";
    const TZ: &str = "America/New_York";

    fn page(data: Value) -> String {
        let row = json!([
            1,
            format!(
                "2b:{}\n",
                json!({"state": {"queries": [{"state": {"data": data}}]}})
            )
        ]);
        format!("<script>self.__next_f.push({row})</script>")
    }

    fn installed() -> Installed {
        Installed {
            skills: vec![(
                "pack:skills/check/SKILL.md".into(),
                "Source check".into(),
                None,
            )],
            extensions: vec![("gh".into(), "GitHub".into(), "mcp".into())],
        }
    }

    struct Profile(std::path::PathBuf);
    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn profile() -> (Profile, std::path::PathBuf) {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        (Profile(root), profile)
    }

    #[test]
    fn the_live_share_page_fixture_reads_as_a_public_profile_without_running_scripts() {
        let html = include_str!("../tests/fixtures/grok-share-page.html");
        let preview = import_grok_page(
            &format!("{URL}/dewey?tracking=1"),
            &format!("{html}<script>throw new Error(\"must not execute\")</script>"),
            &installed(),
            TZ,
        )
        .unwrap();
        assert_eq!(preview.agent.name, "Dewey");
        assert_eq!(
            preview.agent.instructions,
            "Example mail assistant. Draft replies for review."
        );
        assert_eq!(
            (preview.source, preview.coverage),
            ("grok_link", "public-profile")
        );
        let template = preview.agent.template.as_ref().unwrap();
        assert_eq!(template["source"], "grok");
        assert_eq!(template["sourceUrl"], URL);
        assert_eq!(template["original"]["shape"], "wedge");
        let data = json!({"id": "wrong-id-wrong-id-wro", "botName": "Dewey", "description": "x"});
        assert!(import_grok_page(URL, &page(data), &installed(), TZ)
            .unwrap_err()
            .contains("could not be read"));
        assert!(import_grok_page(URL, "<h1>Not found</h1>", &installed(), TZ).is_err());
        assert!(import_grok_page("https://x.ai/bot/marketplace", html, &installed(), TZ).is_err());
    }

    #[test]
    fn grok_sections_map_to_proposals_skills_tools_and_memories() {
        let source = json!({
            "botName": "Scout", "jobTitle": "Researcher", "description": "Find sources.",
            "skills": [{"name": "Source check", "instructions": "Check dates."},
                       {"name": "Cite", "instructions": "Cite every claim."}],
            "plugins": ["github", {"name": "Notion"}],
            "routines": [
                {"name": "Morning digest", "prompt": "Send a digest.", "schedule": {"cron": "0 8 * * 1-5", "timezone": "Europe/Paris"}},
                {"cron": "0 9 * * *"},
                "Check the news every evening at 6pm",
                {"name": "Push review", "prompt": "Review the push.", "event": "When a GitHub push arrives"},
                {"name": "Vague", "prompt": "Do something sometimes."}
            ],
            "memories": ["Use original sources.", {"title": "Tone", "content": "Write plainly."},
                         "token: ghp_0123456789abcdefghijklmnopqrstuvwxyzAB"],
            "customField": "preserve this"
        });
        let preview = import_text(&source.to_string(), &installed(), TZ).unwrap();
        let agent = &preview.agent;
        assert_eq!(
            (agent.name.as_str(), agent.label.as_str()),
            ("Scout", "Researcher")
        );
        assert_eq!(
            agent.skills,
            Some(vec!["pack:skills/check/SKILL.md".to_owned()])
        );
        assert_eq!(
            preview.skills.proposals,
            vec![json!({"name": "Cite", "instructions": "Cite every claim."})]
        );
        assert_eq!(
            preview.tools.matched,
            vec![json!({"name": "github", "id": "gh", "kind": "mcp"})]
        );
        assert_eq!(preview.tools.setup, vec![json!({"name": "Notion"})]);
        assert_eq!(agent.routines.len(), 4, "{:?}", agent.routines);
        assert!(agent.routines.iter().all(|routine| routine.proposed
            && !routine.enabled
            && routine.id.starts_with(IMPORT_PREFIX)));
        use crate::agent_routines::Trigger;
        assert!(matches!(&agent.routines[0].trigger,
            Trigger::Cron(cron) if cron.expr == "0 8 * * 1-5" && cron.tz == "Europe/Paris"));
        assert!(matches!(&agent.routines[1].trigger, Trigger::Cron(cron) if cron.tz == TZ));
        assert!(
            matches!(&agent.routines[2].trigger, Trigger::Cron(cron) if cron.expr == "0 18 * * *")
        );
        assert!(matches!(&agent.routines[3].trigger,
            Trigger::Webhook(hook) if hook.preset == crate::agent_routines::WebhookPreset::Github));
        assert_eq!(preview.routine_notes.len(), 1);
        assert_eq!(preview.routine_notes[0]["name"], "Vague");
        assert_eq!(
            preview.memories,
            vec![
                ImportedMemory {
                    title: "Use original sources.".into(),
                    content: "Use original sources.".into()
                },
                ImportedMemory {
                    title: "Tone".into(),
                    content: "Write plainly.".into()
                },
            ]
        );
        let template = agent.template.as_ref().unwrap();
        assert_eq!(template["original"]["customField"], "preserve this");
        assert_eq!(template["skills"], source["skills"]);
        assert_eq!(template["routine_notes"][0]["name"], "Vague");
    }

    #[test]
    fn markdown_legacy_and_bound_errors_match_the_desktop_reader() {
        let markdown = import_text(
            "# Researcher\n\n## Skills\nCheck sources.",
            &installed(),
            TZ,
        )
        .unwrap();
        assert_eq!(markdown.agent.name, "Researcher");
        assert!(markdown
            .agent
            .instructions
            .contains("## Skills\nCheck sources."));
        let legacy = import_text(
            r#"{"format":"muniment-agent","version":1,"name":"Old","instructions":"Keep this."}"#,
            &installed(),
            TZ,
        )
        .unwrap();
        assert_eq!(legacy.agent.instructions, "Keep this.");
        assert_eq!(legacy.source, "muniment");
        let v2 = import_text(
            r#"{"format":"muniment-agent","version":2,"name":"Two","description":"Work.","schedule":{"enabled":true,"cadence":"weekly","weekday":2,"time":"09:30"}}"#,
            &installed(),
            TZ,
        )
        .unwrap();
        assert_eq!(v2.agent.routines.len(), 1);
        assert!(!v2.agent.routines[0].enabled && v2.agent.routines[0].proposed);
        assert!(import_text(
            r#"{"format":"muniment-agent","version":99}"#,
            &installed(),
            TZ
        )
        .unwrap_err()
        .contains("version"));
        assert!(import_text("# Empty\n", &installed(), TZ).is_err());
        assert!(
            import_text(&"x".repeat(MAX_TEMPLATE_BYTES + 1), &installed(), TZ)
                .unwrap_err()
                .contains("128 KB")
        );
        let nested = import_text(
            r#"{"bot":{"botName":"Nested","description":"Inside."}}"#,
            &installed(),
            TZ,
        )
        .unwrap();
        assert_eq!(nested.agent.name, "Nested");
    }

    #[test]
    fn a_grok_template_survives_import_commit_export_and_import_again() {
        let (_root, profile) = profile();
        let source = json!({
            "botName": "Scout", "label": "Researcher", "description": "Find sources.",
            "skills": [{"name": "Cite", "instructions": "Cite every claim."}],
            "plugins": ["Notion"],
            "routines": [{"cron": "0 9 * * *"}, {"name": "Digest", "prompt": "Send a digest.", "schedule": "30 7 * * 1"}],
            "memories": ["Use original sources."]
        });
        let first = import_text(&source.to_string(), &Installed::default(), TZ).unwrap();
        let saved = commit(&profile, first.agent.clone(), &first.memories).unwrap();
        let exported = export(&profile, &saved.id, "grok_bot", &Installed::default()).unwrap();
        assert_eq!(exported["file_name"], "Scout.grok-bot.md");
        let text = exported["content"].as_str().unwrap();
        assert!(text.contains("Keep routines paused"));
        let again = import_text(text, &Installed::default(), TZ).unwrap();
        assert_eq!(again.source, "markdown");
        assert_eq!(again.agent.name, first.agent.name);
        assert_eq!(again.agent.label, first.agent.label);
        assert_eq!(again.agent.instructions, first.agent.instructions);
        assert_eq!(again.agent.routines, first.agent.routines);
        assert_eq!(again.skills.proposals, first.skills.proposals);
        assert_eq!(again.tools.setup, first.tools.setup);
        assert_eq!(again.memories, first.memories);
    }

    #[test]
    fn a_muniment_export_imports_without_loss_and_carries_no_secrets() {
        let (_root, profile) = profile();
        let agent: crate::agents::Agent = serde_json::from_value(json!({
            "name": "Closer", "label": "Sales", "instructions": "Follow up on deals.",
            "avatar": {"style": "muniment-v2", "seed": "closer"},
            "template": {"custom": "keep"},
            "model": {"provider": "local", "id": "example-model", "thinking": "high"},
            "tools": {"disabled": ["bash"]},
            "skills": ["review"],
            "permissions": {"mode": "strict", "always_allow": [], "ask_first": []},
            "messaging": {"allow": "any"},
            "primary_runtime": "018f0000-0000-7000-8000-000000000095",
            "routines": [
                {"id": "hook", "name": "Hook", "enabled": true, "trigger": {"kind": "webhook", "preset": "github",
                    "secret_sha256": "ab".repeat(32)}},
                {"id": "poll", "name": "Poll", "enabled": true, "prompt": "Summarize.", "trigger": {"kind": "mcp",
                    "server": "crm", "tool": "search", "args": {"q": 1}, "every": {"expr": "0 * * * *", "tz": "UTC"},
                    "condition": {"path": "/total", "op": "changed"}}}
            ]
        }))
        .unwrap();
        let saved = crate::agents::save(&profile, agent).unwrap();
        let (home, private) = crate::agents::memory_paths(&profile, &saved.id).unwrap();
        crate::memory_files::fact_save_in(
            &profile,
            &home,
            crate::memory_files::Fact {
                id: String::new(),
                title: "Pipeline".into(),
                content: "Deals close on Fridays.".into(),
                source: "test".into(),
                trust: crate::memory_files::Trust::User,
            },
        )
        .unwrap();
        std::fs::create_dir_all(&private).unwrap();
        std::fs::write(private.join("secret-note.md"), "private memory").unwrap();
        let first = export(&profile, &saved.id, "muniment", &Installed::default()).unwrap();
        let text = first["content"].as_str().unwrap().to_owned();
        let Trigger::Webhook(hook) = &saved.routines[0].trigger else {
            panic!("webhook")
        };
        for absent in [
            hook.hook_id.as_str(),
            &"ab".repeat(32),
            "018f0000-0000-7000-8000-000000000095",
            "private memory",
            "lastFired",
        ] {
            assert!(!text.contains(absent), "{absent}");
        }
        let preview = import_text(&text, &Installed::default(), TZ).unwrap();
        assert!(preview
            .agent
            .routines
            .iter()
            .all(|routine| routine.proposed && !routine.enabled));
        let copy = commit(&profile, preview.agent, &preview.memories).unwrap();
        assert_ne!(copy.id, saved.id);
        let second = export(&profile, &copy.id, "muniment", &Installed::default()).unwrap();
        assert_eq!(second["content"], first["content"]);
        let read = crate::agents::read(&profile, &copy.id).unwrap();
        assert_eq!(read.model, saved.model);
        assert_eq!(read.permissions, saved.permissions);
        assert_eq!(read.template, saved.template);
        assert!(read.primary_runtime.is_none());
        use crate::agent_routines::Trigger;
        let Trigger::Webhook(copied) = &read.routines[0].trigger else {
            panic!("webhook")
        };
        assert!(!copied.hook_id.is_empty() && copied.hook_id != hook.hook_id);
        assert!(copied.secret_sha256.is_none());
        assert!(export(&profile, &copy.id, "zip", &Installed::default()).is_err());
    }
}
