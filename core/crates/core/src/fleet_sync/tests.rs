//! Three in-process members per class: they converge, settle a conflict by
//! the class's rule, carry removals, never write back what they applied, and
//! keep machine-local values on their own machine.

use std::path::Path;

use serde_json::{json, Value};

use super::*;
use crate::swarm::test_members::{exchange, TestMember};

/// Three linked members, each with a Home, after a first exchange.
fn fleet() -> [TestMember; 3] {
    let members = ["Studio", "Laptop", "Box"].map(TestMember::new);
    for member in &members {
        crate::home::initialize_state_home(&member.profile).unwrap();
    }
    settle(&members);
    members
}

/// Passes and exchanges until every change reached every member. B links A
/// and C, so C hears from A through B.
fn settle(members: &[TestMember; 3]) {
    for _ in 0..4 {
        for member in members {
            sync_all(&member.records());
        }
        exchange(&members[1], &members[0]);
        exchange(&members[2], &members[1]);
    }
    for member in members {
        sync_all(&member.records());
    }
}

/// How many rows each member wrote itself.
fn writes(members: &[TestMember; 3]) -> Vec<u64> {
    members.iter().map(TestMember::own_writes).collect()
}

/// Two more rounds change nothing: no member writes back what it applied.
fn assert_quiet(members: &[TestMember; 3]) {
    let before = writes(members);
    settle(members);
    settle(members);
    assert_eq!(
        writes(members),
        before,
        "a member wrote back what it applied"
    );
}

/// Makes `member`'s next writes later than every other member's.
fn later(member: &TestMember, seconds: i64) {
    member
        .swarm
        .advance_clock(chrono::Duration::seconds(seconds));
}

/// Every record value a member holds for a class, as text.
fn record_text(member: &TestMember, class: &str) -> String {
    serde_json::to_string(&member.records().scan(class, None, 1000).unwrap()).unwrap()
}

fn preference(profile: &Path, key: &str) -> Option<Value> {
    crate::preferences::values(profile).unwrap().remove(key)
}

fn set_preferences(member: &TestMember, values: Value) {
    crate::preferences::operation(
        &member.profile,
        "preferences.set",
        json!({ "values": values }),
    )
    .unwrap();
}

#[test]
fn preferences_converge_take_the_later_write_and_keep_local_keys_home() {
    let members = fleet();
    let [a, b, c] = &members;
    let events = b.swarm.subscribe();
    set_preferences(
        a,
        json!({"theme": "dark", "sidebar-width": 280, "frame": "frameless"}),
    );
    settle(&members);
    for member in &members {
        assert_eq!(preference(&member.profile, "theme"), Some(json!("dark")));
    }
    // Machine-local keys never reach the records or another machine.
    for member in &members {
        let text = record_text(member, PREFERENCES_CLASS);
        assert!(!text.contains("sidebar-width") && !text.contains("frameless"));
    }
    assert!(preference(&b.profile, "sidebar-width").is_none());
    assert!(preference(&c.profile, "frame").is_none());
    let changed: Vec<_> = events
        .try_iter()
        .filter(|event| event.name == muniment_attach::EventName::PreferencesChanged)
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].body["source"], "fleet");
    assert_eq!(changed[0].body["values"]["theme"], "dark");

    // Two members change the same key before they hear from each other.
    set_preferences(a, json!({"theme": "light"}));
    later(c, 5);
    set_preferences(c, json!({"theme": "blue"}));
    settle(&members);
    for member in &members {
        assert_eq!(preference(&member.profile, "theme"), Some(json!("blue")));
    }

    // A removal reaches every member.
    set_preferences(b, json!({"theme": null}));
    settle(&members);
    for member in &members {
        assert!(preference(&member.profile, "theme").is_none());
    }
    assert_quiet(&members);
}

#[test]
fn a_joining_member_takes_the_fleets_preferences_and_adds_its_own() {
    let members = ["Studio", "Laptop", "Box"].map(TestMember::new);
    let [a, b, c] = &members;
    set_preferences(a, json!({"theme": "dark"}));
    exchange(b, a);
    sync_all(&a.records());
    exchange(b, a);
    // C kept its own choices before it ever linked.
    set_preferences(c, json!({"theme": "green", "agent-avatars": "beam"}));
    exchange(c, b);
    settle(&members);
    for member in &members {
        assert_eq!(preference(&member.profile, "theme"), Some(json!("dark")));
        assert_eq!(
            preference(&member.profile, "agent-avatars"),
            Some(json!("beam"))
        );
    }
}

fn request(path: &str) -> Value {
    json!({"request_kind": "path", "resource": {"type": "path", "path": path, "operation": "read"}})
}

fn save_permissions(member: &TestMember, body: Value) {
    crate::permission_gate::settings::operation(&member.profile, "permission.settings_save", body)
        .unwrap();
}

fn permissions(member: &TestMember) -> Value {
    json!(crate::permission_gate::settings::load(&member.profile))
}

/// The settings scope keeps on each machine exactly the permission settings
/// the sync adapter leaves out: a change to any other one moves its slots.
#[test]
fn the_settings_scope_names_each_permission_setting_sync_leaves_home() {
    use crate::permission_gate::settings::{self, PermissionSettings};
    let member = TestMember::new("Studio");
    let records = member.records();
    let context = Context {
        profile: &member.profile,
        records: &records,
        origin: "studio".into(),
        name: "Studio".into(),
    };
    // Defaults that name no bypass, so turning bypass off moves nothing else.
    let base = json!(PermissionSettings {
        default_thread_mode: crate::permission_gate::PermissionMode::Manual,
        default_agent_mode: crate::permission_gate::PermissionMode::Manual,
        ..PermissionSettings::default()
    });
    let store = |value: &Value| {
        settings::save(
            &member.profile,
            serde_json::from_value(value.clone()).unwrap(),
        )
        .unwrap();
    };
    store(&base);
    let before = permissions::Permissions.read(&context).unwrap();
    let mut home = BTreeSet::new();
    for (field, value) in base.as_object().unwrap() {
        let changed = match value {
            Value::Bool(on) => json!(!on),
            Value::Number(number) => json!(number.as_u64().unwrap() + 60),
            Value::String(_) => json!("plan"),
            Value::Array(_) => json!([request("/scope")]),
            other => panic!("classify the permission setting {field}: {other}"),
        };
        let mut settings = base.clone();
        settings[field] = changed;
        store(&settings);
        if permissions::Permissions.read(&context).unwrap() == before {
            home.insert(field.as_str());
        }
        store(&base);
    }
    let part = crate::settings_scope::part("permissions").unwrap();
    assert_eq!(part.classes(), [permissions::CLASS]);
    let named: BTreeSet<&str> = part.machine_local.iter().map(|local| local.field).collect();
    assert_eq!(home, named);
}

#[test]
fn permission_rules_keep_a_concurrent_add_and_bypass_stays_local() {
    let members = fleet();
    let [a, b, c] = &members;
    save_permissions(
        a,
        json!({
            "bypass_enabled": false,
            "default_thread_mode": "edit_auto",
            "unattended_timeout_seconds": 7200,
            "deny": [request("/secret")],
            "ask_first": [request("/ask")],
            "always_allow": [request("/notes")]
        }),
    );
    settle(&members);
    for member in [b, c] {
        let settings = permissions(member);
        assert_eq!(settings["default_thread_mode"], "edit_auto");
        assert_eq!(settings["unattended_timeout_seconds"], 7200);
        assert_eq!(settings["deny"], json!([request("/secret")]));
        assert_eq!(settings["ask_first"], json!([request("/ask")]));
        assert_eq!(settings["always_allow"], json!([request("/notes")]));
        // Bypass stays as each machine set it.
        assert_eq!(settings["bypass_enabled"], true);
    }
    assert_eq!(permissions(a)["bypass_enabled"], false);
    for member in &members {
        assert!(!record_text(member, permissions::CLASS).contains("bypass"));
    }

    // A and B add the same deny rule before they hear from each other, then
    // A removes it: B's add keeps it.
    save_permissions(a, json!({"deny": [request("/secret"), request("/keys")]}));
    sync_all(&a.records());
    save_permissions(b, json!({"deny": [request("/secret"), request("/keys")]}));
    sync_all(&b.records());
    save_permissions(a, json!({"deny": [request("/secret")]}));
    settle(&members);
    for member in &members {
        assert_eq!(
            permissions(member)["deny"],
            json!([request("/secret"), request("/keys")])
        );
    }

    // Removals of an ask-first and an always-allow rule reach every member.
    save_permissions(c, json!({"ask_first": [], "always_allow": []}));
    settle(&members);
    for member in &members {
        assert_eq!(permissions(member)["ask_first"], json!([]));
        assert_eq!(permissions(member)["always_allow"], json!([]));
    }
    assert_quiet(&members);
}

#[test]
fn settings_that_arrive_in_parts_never_publish_the_defaults_a_write_filled_in() {
    let members = fleet();
    let [a, b, c] = &members;
    // A's fields reach B in two exchanges, as a cut exchange delivers them.
    // B has no settings file, so its write of the first part fills in the
    // default wait.
    let records = a.records();
    records
        .put(
            permissions::CLASS,
            "settings",
            "default_thread_mode",
            json!("edit_auto"),
        )
        .unwrap();
    exchange(b, a);
    sync_all(&b.records());
    assert_eq!(permissions(b)["default_thread_mode"], "edit_auto");
    records
        .put(
            permissions::CLASS,
            "settings",
            "unattended_timeout_seconds",
            json!(7200),
        )
        .unwrap();
    exchange(b, a);
    settle(&members);
    for member in [b, c] {
        assert_eq!(permissions(member)["unattended_timeout_seconds"], 7200);
    }
    let held = b
        .records()
        .get(permissions::CLASS, "settings")
        .unwrap()
        .unwrap();
    assert_eq!(
        held.fields["unattended_timeout_seconds"].origin,
        a.records().origin().unwrap()
    );
    assert_quiet(&members);
}

fn retention(member: &TestMember) -> Option<crate::retention_record::RetentionChoice> {
    crate::retention_record::read_retention_choice(&member.profile)
}

#[test]
fn the_retention_choice_converges_and_the_later_one_wins() {
    use crate::retention_record::{write_retention_choice, RetentionChoice, RECORD_FILE};
    let members = fleet();
    let [a, b, c] = &members;
    write_retention_choice(&a.profile, RetentionChoice::DeleteAfter30Days).unwrap();
    settle(&members);
    for member in &members {
        assert_eq!(retention(member), Some(RetentionChoice::DeleteAfter30Days));
    }
    write_retention_choice(&b.profile, RetentionChoice::DeleteAfter90Days).unwrap();
    later(c, 5);
    write_retention_choice(&c.profile, RetentionChoice::DeleteAfter1Year).unwrap();
    settle(&members);
    for member in &members {
        assert_eq!(retention(member), Some(RetentionChoice::DeleteAfter1Year));
    }
    // A removed choice keeps every thread everywhere.
    std::fs::remove_file(a.profile.join(RECORD_FILE)).unwrap();
    later(a, 10);
    settle(&members);
    for member in &members {
        assert_eq!(retention(member), None);
    }
    assert_quiet(&members);
}

fn agent(member: &TestMember) -> std::path::PathBuf {
    crate::state_root::agent_directory(&member.profile)
}

fn save_routes(member: &TestMember, routes: &[(&str, &str, &str)]) {
    use crate::model_router::config;
    let agent = agent(member);
    let mut router = config::load(&agent).unwrap();
    router.routes = routes
        .iter()
        .map(|(key, description, model)| config::Route {
            key: (*key).into(),
            description: (*description).into(),
            family: "anthropic".into(),
            model: (*model).into(),
        })
        .collect();
    config::save(&agent, &router).unwrap();
}

fn routes(member: &TestMember) -> Vec<(String, String)> {
    crate::model_router::config::load(&agent(member))
        .unwrap()
        .routes
        .into_iter()
        .map(|route| (route.key, route.description))
        .collect()
}

fn connections(member: &TestMember) -> Vec<Value> {
    std::fs::read(agent(member).join("classifier-connections.json"))
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
        .unwrap_or_default()
}

#[test]
fn routes_and_decision_models_sync_without_keys_or_loopback_connections() {
    let members = fleet();
    let [a, b, c] = &members;
    save_routes(
        a,
        &[
            ("fast", "Quick answers", "haiku"),
            ("deep", "Hard problems", "opus"),
        ],
    );
    std::fs::create_dir_all(agent(a)).unwrap();
    std::fs::write(
        agent(a).join("classifier-connections.json"),
        serde_json::to_vec(&json!([
            {"id": "hosted", "name": "Hosted", "catalog_id": "jev",
             "classifier": {"kind": "typesafe", "api_key": "ts-secret-key-1234", "model": "jev-latest"}},
            {"id": "local", "name": "Local", "catalog_id": "custom",
             "classifier": {"kind": "endpoint", "base_url": "http://127.0.0.1:8080/v1", "model": "small"}}
        ]))
        .unwrap(),
    )
    .unwrap();
    settle(&members);
    for member in [b, c] {
        assert_eq!(
            routes(member),
            vec![
                ("fast".into(), "Quick answers".into()),
                ("deep".into(), "Hard problems".into())
            ]
        );
        let held = connections(member);
        assert_eq!(held.len(), 1, "only the hosted connection travels");
        assert_eq!(held[0]["id"], "hosted");
        assert_eq!(held[0]["classifier"]["api_key"], "");
    }
    for member in &members {
        let text = record_text(member, routing::CONNECTION_CLASS);
        assert!(!text.contains("ts-secret-key") && !text.contains("127.0.0.1"));
    }
    // A's own key and loopback connection stay as they were.
    assert_eq!(connections(a).len(), 2);
    assert_eq!(
        connections(a)[0]["classifier"]["api_key"],
        "ts-secret-key-1234"
    );

    // B and C rewrite one statement at once: the later one wins.
    save_routes(
        b,
        &[
            ("fast", "Short replies", "haiku"),
            ("deep", "Hard problems", "opus"),
        ],
    );
    later(c, 5);
    save_routes(
        c,
        &[
            ("fast", "Small tasks", "haiku"),
            ("deep", "Hard problems", "opus"),
        ],
    );
    settle(&members);
    for member in &members {
        assert_eq!(routes(member)[0], ("fast".into(), "Small tasks".into()));
    }
    // A removed route and connection leave every member.
    later(a, 10);
    save_routes(a, &[("fast", "Small tasks", "haiku")]);
    let mut held = connections(a);
    held.retain(|connection| connection["id"] != "hosted");
    std::fs::write(
        agent(a).join("classifier-connections.json"),
        serde_json::to_vec(&held).unwrap(),
    )
    .unwrap();
    settle(&members);
    for member in &members {
        assert_eq!(routes(member).len(), 1);
        assert!(connections(member)
            .iter()
            .all(|connection| connection["id"] != "hosted"));
    }
    assert_eq!(connections(a).len(), 1);
    assert_quiet(&members);
}

#[test]
fn search_order_and_engines_sync_and_keys_stay_home() {
    use crate::search_settings::{self, ProviderKey};
    let members = fleet();
    let [a, b, c] = &members;
    let mut settings = search_settings::load(&a.profile).unwrap();
    settings
        .set_key(
            "brave",
            ProviderKey {
                api_key: "brave-secret-0001".into(),
                ..ProviderKey::default()
            },
        )
        .unwrap();
    settings
        .set_engines(vec!["bing".into(), "wikipedia".into()])
        .unwrap();
    search_settings::save(&a.profile, &settings).unwrap();
    settle(&members);
    for member in [b, c] {
        let held = search_settings::load(&member.profile).unwrap();
        assert_eq!(held.engines, vec!["bing", "wikipedia"]);
        // Without Brave's key here, Brave stays out of the order.
        assert!(held.keys.is_empty());
        assert!(held.order.iter().all(|entry| entry.id != "brave"));
    }
    for member in &members {
        assert!(!record_text(member, search::CLASS).contains("brave-secret"));
    }
    // Concurrent engine choices: the later one wins.
    let mut on_b = search_settings::load(&b.profile).unwrap();
    on_b.set_engines(vec!["yahoo".into()]).unwrap();
    search_settings::save(&b.profile, &on_b).unwrap();
    later(c, 5);
    let mut on_c = search_settings::load(&c.profile).unwrap();
    on_c.set_engines(vec!["duckduckgo".into()]).unwrap();
    search_settings::save(&c.profile, &on_c).unwrap();
    settle(&members);
    for member in &members {
        assert_eq!(
            search_settings::load(&member.profile).unwrap().engines,
            vec!["duckduckgo"]
        );
    }
    // A keeps its key and Brave's place in its own order.
    let on_a = search_settings::load(&a.profile).unwrap();
    assert!(on_a.keys.contains_key("brave"));
    assert!(on_a.order.iter().any(|entry| entry.id == "brave"));
    assert_quiet(&members);
}

#[test]
fn projects_show_on_every_member_with_files_on_the_owner() {
    let members = fleet();
    let [a, b, c] = &members;
    let project = crate::projects::create(&a.profile, "Research").unwrap();
    let folder = crate::projects::folder(&a.profile, &project).unwrap();
    std::fs::write(folder.join("notes.md"), "draft").unwrap();
    crate::projects::assign(&a.profile, "thread-1", &project).unwrap();
    settle(&members);
    for member in [b, c] {
        let catalog = crate::projects::list(&member.profile).unwrap();
        let remote = &catalog.remote[&project];
        assert_eq!(remote.name, "Research");
        assert_eq!(remote.owner_runtime, a.id);
        assert_eq!(remote.path, folder.to_string_lossy());
        assert!(!catalog.projects.contains_key(&project));
        assert_eq!(catalog.threads["thread-1"], project);
        // The files stay on the owner.
        let root = crate::projects::root(&member.profile).unwrap();
        assert!(!root.join("Research").exists());
        // A thread in another member's project works in its own folder.
        let workspace = crate::projects::workspace(&member.profile, "thread-1").unwrap();
        assert!(!workspace.starts_with(&root));
    }

    // B and C rename the project at once: the later name wins, and the owner
    // renames its folder.
    let rename = |member: &TestMember, name: &str| {
        crate::projects::update(&member.profile, |catalog| {
            catalog.remote.get_mut(&project).unwrap().name = name.into();
            true
        })
        .unwrap();
    };
    rename(b, "Notes");
    later(c, 5);
    rename(c, "Papers");
    // Two members put one thread in different projects: the later wins.
    let other = crate::projects::create(&b.profile, "Other").unwrap();
    crate::projects::assign(&b.profile, "thread-2", &other).unwrap();
    later(a, 10);
    crate::projects::assign(&a.profile, "thread-2", &project).unwrap();
    settle(&members);
    for member in &members {
        let catalog = crate::projects::saved(&member.profile).unwrap();
        let name = catalog
            .projects
            .get(&project)
            .cloned()
            .or_else(|| catalog.remote.get(&project).map(|held| held.name.clone()));
        assert_eq!(name.as_deref(), Some("Papers"));
        assert_eq!(catalog.threads["thread-2"], project);
    }
    let renamed = crate::projects::folder(&a.profile, &project).unwrap();
    let folder_name = renamed.file_name().unwrap().to_string_lossy().into_owned();
    assert!(folder_name.starts_with("Papers") && renamed.join("notes.md").is_file());

    // Removals: a thread leaves its project, and the owner drops a project.
    crate::projects::unassign(&a.profile, "thread-1").unwrap();
    let other_folder = crate::projects::folder(&b.profile, &other).unwrap();
    crate::projects::update(&b.profile, |catalog| {
        catalog.projects.remove(&other).is_some()
    })
    .unwrap();
    std::fs::remove_dir(other_folder).unwrap();
    settle(&members);
    for member in &members {
        let catalog = crate::projects::saved(&member.profile).unwrap();
        assert!(!catalog.threads.contains_key("thread-1"));
        assert!(!catalog.remote.contains_key(&other));
    }
    assert_quiet(&members);
}

fn memory_file(member: &TestMember, relative: &str) -> Option<String> {
    let home = crate::memory_files::home(&member.profile).unwrap();
    std::fs::read_to_string(home.join("memory").join(relative)).ok()
}

fn memory_names(member: &TestMember) -> Vec<String> {
    crate::memory_files::sync_files(&member.profile)
        .unwrap()
        .into_keys()
        .collect()
}

#[test]
fn memory_files_converge_and_removals_reach_every_member() {
    use crate::memory_files::{self, Fact, Trust};
    let members = fleet();
    let [a, b, c] = &members;
    memory_files::profile_save(&a.profile, "# Profile\n\nCall me Alex.\n").unwrap();
    let fact = memory_files::fact_save(
        &a.profile,
        Fact {
            id: String::new(),
            title: "Coffee".into(),
            content: "Takes it black.".into(),
            source: "Settings".into(),
            trust: Trust::User,
        },
    )
    .unwrap();
    memory_files::recall_settings_save(
        &a.profile,
        memory_files::RecallSettings {
            learn: false,
            use_memories: true,
            ask: true,
        },
    )
    .unwrap();
    settle(&members);
    for member in [b, c] {
        assert_eq!(
            memory_file(member, "profile.md").as_deref(),
            Some("# Profile\n\nCall me Alex.\n")
        );
        let facts = memory_files::facts(&member.profile).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Takes it black.");
    }
    // Recall, usage, pending and the index stay on each machine.
    for member in &members {
        let text = record_text(member, memory::CLASS);
        assert!(!text.contains("memory-recall") && !text.contains("memory-index"));
        assert!(member
            .records()
            .scan(memory::CLASS, None, 100)
            .unwrap()
            .iter()
            .all(|record| record.key.ends_with(".md")));
    }

    // An edit made after the last one arrived replaces it, with no copy.
    let mut edited = memory_files::facts(&b.profile).unwrap().remove(0);
    edited.content = "Takes it black, no sugar.".into();
    memory_files::fact_save(&b.profile, edited).unwrap();
    settle(&members);
    for member in &members {
        let facts = memory_files::facts(&member.profile).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Takes it black, no sugar.");
    }

    // Removing the fact moves it to the deleted memories everywhere.
    memory_files::fact_delete(&c.profile, &fact.id).unwrap();
    settle(&members);
    for member in &members {
        assert!(memory_files::facts(&member.profile).unwrap().is_empty());
    }
    assert_eq!(memory_files::deleted_facts(&a.profile).unwrap().len(), 1);
    assert_quiet(&members);
}

#[test]
fn concurrent_memory_edits_keep_both_as_a_conflict_copy() {
    use crate::memory_files::{self, Fact, Trust};
    let members = fleet();
    let [a, b, _] = &members;
    memory_files::profile_save(&a.profile, "# Profile\n\nStart.\n").unwrap();
    let fact = memory_files::fact_save(
        &a.profile,
        Fact {
            id: String::new(),
            title: "Tea".into(),
            content: "Green tea.".into(),
            source: "Settings".into(),
            trust: Trust::User,
        },
    )
    .unwrap();
    settle(&members);

    // A and B edit the same files before they hear from each other; B's
    // edits are later, so A's become the copies.
    memory_files::profile_save(&a.profile, "# Profile\n\nFrom the studio.\n").unwrap();
    let mut on_a = fact.clone();
    on_a.content = "Oolong.".into();
    memory_files::fact_save(&a.profile, on_a).unwrap();
    sync_all(&a.records());
    later(b, 5);
    memory_files::profile_save(&b.profile, "# Profile\n\nFrom the laptop.\n").unwrap();
    let mut on_b = fact.clone();
    on_b.content = "Matcha.".into();
    memory_files::fact_save(&b.profile, on_b).unwrap();
    sync_all(&b.records());
    settle(&members);

    for member in &members {
        assert_eq!(
            memory_file(member, "profile.md").as_deref(),
            Some("# Profile\n\nFrom the laptop.\n")
        );
        assert_eq!(
            memory_file(member, "profile (from Studio).md").as_deref(),
            Some("# Profile\n\nFrom the studio.\n")
        );
        let mut facts = memory_files::facts(&member.profile).unwrap();
        facts.sort_by(|x, y| x.title.cmp(&y.title));
        let summary: Vec<(String, String)> = facts
            .into_iter()
            .map(|fact| (fact.title, fact.content))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Tea".to_owned(), "Matcha.".to_owned()),
                ("Tea (from Studio)".to_owned(), "Oolong.".to_owned()),
            ]
        );
        assert_eq!(memory_names(member).len(), 4);
    }
    assert_quiet(&members);
}

#[test]
fn a_joining_member_keeps_its_own_memory_file_as_a_copy() {
    let members = ["Studio", "Laptop", "Box"].map(TestMember::new);
    for member in &members {
        crate::home::initialize_state_home(&member.profile).unwrap();
    }
    let [a, b, c] = &members;
    crate::memory_files::profile_save(&a.profile, "# Profile\n\nFleet.\n").unwrap();
    exchange(b, a);
    sync_all(&a.records());
    exchange(b, a);
    // C wrote its own profile before it ever linked.
    crate::memory_files::profile_save(&c.profile, "# Profile\n\nMine.\n").unwrap();
    exchange(c, b);
    settle(&members);
    for member in &members {
        assert_eq!(
            memory_file(member, "profile.md").as_deref(),
            Some("# Profile\n\nFleet.\n")
        );
        assert_eq!(
            memory_file(member, "profile (from Box).md").as_deref(),
            Some("# Profile\n\nMine.\n")
        );
    }
}
