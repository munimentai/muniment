//! The record classes that hold secrets, extensions and blobs between three
//! in-process members: account metadata, sealed keys, MCP servers, skills
//! and plugins, artifacts and their pins.

use serde_json::{json, Value};
use sha2::Digest as _;

use super::fleet::Role;
use super::fleet_tests::{api_key, join, settle_links, Node};
use super::*;

/// Three full members: B and C joined A.
fn fleet() -> (Node, Node, Node) {
    let a = Node::new("a");
    let b = Node::new("b");
    let c = Node::new("c");
    join(&b, &a, Role::Full);
    join(&c, &a, Role::Full);
    (a, b, c)
}

fn sync(a: &Node, b: &Node, c: &Node) {
    settle_links(&[(b, a), (c, a)]);
}

fn pass_all(nodes: &[&Node]) {
    for node in nodes {
        node.pass();
    }
}

fn accounts(node: &Node) -> Vec<Value> {
    credentials::credentials_operation(node.inner(), json!({})).unwrap()["accounts"]
        .as_array()
        .unwrap()
        .clone()
}

fn account<'a>(list: &'a [Value], holder: &str, name: &str) -> Option<&'a Value> {
    list.iter()
        .find(|account| account["holder"] == holder && account["name"] == name)
}

#[test]
fn every_member_lists_every_account_with_its_holder() {
    let (a, b, c) = fleet();
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    b.write_auth(json!({"anthropic": api_key("sk-from-b")}));
    c.write_auth(json!({}));
    a.share();
    sync(&a, &b, &c);
    pass_all(&[&a, &b, &c]);
    sync(&a, &b, &c);

    for node in [&a, &b, &c] {
        let list = accounts(node);
        let from_a = account(&list, &a.id, "pi:openai").unwrap();
        assert_eq!(from_a["holder_name"], "a");
        assert_eq!(from_a["kind"], "api_key");
        assert_eq!(from_a["shared"], true);
        assert_eq!(from_a["this_machine"], node.id == a.id);
        // The key itself never enters a record.
        assert!(!from_a.to_string().contains("sk-from-a"));
        let from_b = account(&list, &b.id, "pi:anthropic").unwrap();
        assert_eq!(from_b["holder_name"], "b");
        assert_eq!(from_b["shared"], false);
    }
    // C uses A's key, which A shares, and not B's, which B keeps.
    let list = accounts(&c);
    assert_eq!(
        account(&list, &a.id, "pi:openai").unwrap()["usable_here"],
        true
    );
    assert_eq!(
        account(&list, &b.id, "pi:anthropic").unwrap()["usable_here"],
        false
    );
    // C holds A's key as a lease from A, so it lists no account of its own.
    assert!(list
        .iter()
        .all(|account| account["holder"] != c.id.as_str()));

    // A record another member writes under A's name is not A's word.
    c.swarm
        .records()
        .put(
            accounts::CLASS,
            &format!("{}/pi:forged", a.id),
            "account",
            json!({"name": "pi:forged", "holder": a.id, "kind": "api_key"}),
        )
        .unwrap();
    sync(&a, &b, &c);
    assert!(account(&accounts(&b), &a.id, "pi:forged").is_none());

    // A drops its key: the record goes on every member.
    a.write_auth(json!({}));
    a.pass();
    sync(&a, &b, &c);
    for node in [&a, &b, &c] {
        assert!(account(&accounts(node), &a.id, "pi:openai").is_none());
    }
}

fn write_connections(node: &Node, connections: Value) {
    std::fs::create_dir_all(node.agent()).unwrap();
    std::fs::write(
        node.agent().join("classifier-connections.json"),
        serde_json::to_vec(&connections).unwrap(),
    )
    .unwrap();
}

fn connections(node: &Node) -> Value {
    serde_json::from_slice(
        &std::fs::read(node.agent().join("classifier-connections.json")).unwrap(),
    )
    .unwrap()
}

fn search_key(node: &Node, provider: &str) -> Option<String> {
    crate::search_settings::load(&node.root)
        .unwrap()
        .keys
        .get(provider)
        .map(|key| key.api_key.clone())
}

fn set_search_key(node: &Node, provider: &str, key: &str) {
    let mut settings = crate::search_settings::load(&node.root).unwrap();
    settings
        .set_key(
            provider,
            crate::search_settings::ProviderKey {
                api_key: key.into(),
                ..Default::default()
            },
        )
        .unwrap();
    crate::search_settings::save(&node.root, &settings).unwrap();
}

#[test]
fn decision_model_and_search_keys_travel_sealed_and_never_replace_a_local_key() {
    let (a, b, c) = fleet();
    let remote = |key: &str| {
        json!({"id": "jev", "name": "Jev", "catalog_id": "jev",
               "classifier": {"kind": "typesafe", "api_key": key, "model": "m"}})
    };
    let local = json!({"id": "home", "name": "Home", "catalog_id": "custom",
        "classifier": {"kind": "endpoint", "base_url": "http://127.0.0.1:9000", "api_key": "loop-key", "model": "m"}});
    write_connections(&a, json!([remote("jev-key-from-a"), local]));
    // The records brought B and C the connection without its key.
    write_connections(&b, json!([remote("")]));
    write_connections(&c, json!([remote("")]));
    a.write_auth(json!({}));
    set_search_key(&a, "serper", "serper-from-a");
    set_search_key(&a, "kagi", "kagi-from-a");
    set_search_key(&c, "kagi", "kagi-own-c");
    a.share();
    sync(&a, &b, &c);
    a.pass();
    sync(&a, &b, &c);

    let sealed = a.sealed(&format!("keys:{}", a.id)).unwrap();
    assert!(!serde_json::to_string(&sealed)
        .unwrap()
        .contains("jev-key-from-a"));
    for node in [&b, &c] {
        assert_eq!(
            connections(node)[0]["classifier"]["api_key"],
            "jev-key-from-a"
        );
        assert_eq!(search_key(node, "serper").as_deref(), Some("serper-from-a"));
    }
    // C keeps the key it entered itself, and the loopback connection's key
    // never leaves A.
    assert_eq!(search_key(&c, "kagi").as_deref(), Some("kagi-own-c"));
    assert_eq!(search_key(&b, "kagi").as_deref(), Some("kagi-from-a"));
    assert!(!serde_json::to_string(&b.open(&sealed).unwrap().unwrap())
        .unwrap()
        .contains("loop-key"));

    // A key that arrived is not B's to share on.
    b.write_auth(json!({}));
    b.share();
    b.pass();
    let from_b = c
        .open(&b.sealed(&format!("keys:{}", b.id)).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(from_b["keys"], json!({"decision_models": {}, "search": {}}));

    // A rotates one key and drops another: the members follow.
    set_search_key(&a, "serper", "serper-rotated");
    let mut settings = crate::search_settings::load(&a.root).unwrap();
    settings.remove_key("kagi").unwrap();
    crate::search_settings::save(&a.root, &settings).unwrap();
    a.pass();
    sync(&a, &b, &c);
    assert_eq!(search_key(&b, "serper").as_deref(), Some("serper-rotated"));
    assert_eq!(search_key(&b, "kagi"), None);
    assert_eq!(search_key(&c, "kagi").as_deref(), Some("kagi-own-c"));
}

/// Runs the extension and artifact classes on every member around a sync.
fn classes(nodes: &[&Node]) {
    for _ in 0..3 {
        for node in nodes {
            node.inner().classes_pass(true);
        }
        settle_links(&[(nodes[1], nodes[0]), (nodes[2], nodes[0])]);
    }
}

fn inventory(node: &Node) -> Value {
    crate::extend::inventory(&node.root).unwrap()
}

fn item(node: &Node, id: &str) -> Option<Value> {
    inventory(node)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .cloned()
}

fn write_items(node: &Node, items: Value) {
    crate::extend::update_inventory(&node.root, |state| {
        state["items"] = items;
        Ok(true)
    })
    .unwrap();
}

fn edit_item(node: &Node, id: &str, edit: impl Fn(&mut Value)) {
    crate::extend::update_inventory(&node.root, |state| {
        for item in state["items"].as_array_mut().unwrap() {
            if item["id"] == id {
                edit(item);
            }
        }
        Ok(true)
    })
    .unwrap();
}

#[test]
fn mcp_servers_sync_and_a_missing_stdio_command_is_not_on_this_machine() {
    let (a, b, c) = fleet();
    let missing = a.root.join("no-such-tool");
    write_items(
        &a,
        json!([
            {"id": "ctx", "kind": "mcp", "name": "Context", "description": "Docs",
             "source": "", "enabled": true,
             "definition": {"url": "https://mcp.example.com/mcp", "bearerToken": true}},
            {"id": "tool", "kind": "mcp", "name": "Tool", "description": "", "source": "",
             "enabled": true, "definition": {"command": missing, "args": ["--stdio"]}},
        ]),
    );
    classes(&[&a, &b, &c]);

    for node in [&b, &c] {
        let ctx = item(node, "ctx").unwrap();
        assert_eq!(ctx["definition"]["url"], "https://mcp.example.com/mcp");
        assert_eq!(ctx["fleet"]["origin"], a.id.as_str());
        assert_eq!(ctx["enabled"], true);
        // The client's view says why the stdio server cannot start here.
        let read = crate::extend::command(&node.root, "read", json!({})).unwrap();
        let shown: Vec<&Value> = read["items"].as_array().unwrap().iter().collect();
        let tool = shown.iter().find(|item| item["id"] == "tool").unwrap();
        assert_eq!(tool["availability"], "not on this machine");
        assert!(shown
            .iter()
            .find(|item| item["id"] == "ctx")
            .unwrap()
            .get("availability")
            .is_none());
        // A run never starts it.
        let mut state = inventory(node);
        state["turns"]["t"] = json!({"selected": ["ctx", "tool"]});
        let servers = crate::extend::snapshot(&state, "t", json!({}))["mcpServers"].clone();
        assert_eq!(servers.as_object().unwrap().len(), 1, "{servers}");
    }

    // B turns the server off for itself; A edits it.
    edit_item(&b, "ctx", |item| item["enabled"] = json!(false));
    edit_item(&a, "ctx", |item| item["description"] = json!("Docs, newer"));
    classes(&[&a, &b, &c]);
    for node in [&a, &b, &c] {
        assert_eq!(item(node, "ctx").unwrap()["description"], "Docs, newer");
    }
    assert_eq!(item(&b, "ctx").unwrap()["enabled"], false);
    assert_eq!(item(&c, "ctx").unwrap()["enabled"], true);
    assert_eq!(item(&a, "ctx").unwrap()["enabled"], true);

    // A removes a server: it goes everywhere.
    write_items(&a, json!([item(&a, "ctx").unwrap()]));
    classes(&[&a, &b, &c]);
    for node in [&a, &b, &c] {
        assert!(item(node, "tool").is_none());
    }
}

#[test]
fn a_package_is_fetched_only_when_enabled_or_pinned_and_checked_by_its_hash() {
    let (a, b, c) = fleet();
    let packages = a.root.join("extensions/packages");
    for (name, text) in [("kit", "# Kit\nUse the kit."), ("lazy", "# Lazy\nRarely.")] {
        std::fs::create_dir_all(packages.join(name).join("scripts")).unwrap();
        std::fs::write(packages.join(name).join("SKILL.md"), text).unwrap();
        std::fs::write(packages.join(name).join("scripts/run.sh"), "echo hi\n").unwrap();
        std::fs::create_dir_all(packages.join(name).join("node_modules/dep")).unwrap();
        std::fs::write(packages.join(name).join("node_modules/dep/index.js"), "x").unwrap();
    }
    let skill = |id: &str, enabled: bool| {
        json!({"id": id, "kind": "skill", "name": id, "description": "", "source": "local",
               "version": "v1", "digest": format!("digest-{id}"), "enabled": enabled,
               "base": packages.join(id), "skills": [{"name": id, "path": "SKILL.md"}],
               "extensions": [], "servers": {}, "dependencies": {}})
    };
    write_items(&a, json!([skill("kit", true), skill("lazy", false)]));
    classes(&[&a, &b, &c]);

    let hash = item(&a, "kit").map(|_| ()).and(
        a.swarm
            .records()
            .get(extensions::PACKAGE_CLASS, "kit")
            .unwrap()
            .unwrap()
            .value("package")
            .and_then(Value::as_str)
            .map(str::to_owned),
    );
    let hash = hash.unwrap();
    for node in [&b, &c] {
        let kit = item(node, "kit").unwrap();
        let base = std::path::PathBuf::from(kit["base"].as_str().unwrap());
        assert!(base.starts_with(node.root.join("extensions/packages")));
        assert_eq!(
            std::fs::read_to_string(base.join("SKILL.md")).unwrap(),
            "# Kit\nUse the kit."
        );
        assert!(base.join("scripts/run.sh").is_file());
        assert!(!base.join("node_modules").exists());
        // A disabled item's package never travels until it is wanted.
        let lazy = item(node, "lazy").unwrap();
        assert!(lazy["base"].is_null());
        assert_eq!(
            crate::extend::availability(&lazy),
            Some("package not on this machine")
        );
    }
    assert!(pins::fetched(b.inner(), extensions::PACKAGE_CLASS)
        .unwrap()
        .contains(&hash));
    // C pins the lazy package and gets it while it stays turned off.
    pins::pin_set_operation(
        c.inner(),
        json!({"pins": [{"class": "extension", "key": "lazy", "pinned": true}]}),
    )
    .unwrap();
    classes(&[&a, &b, &c]);
    assert!(item(&c, "lazy").unwrap()["base"].is_string());
    assert_eq!(item(&c, "lazy").unwrap()["enabled"], false);
    assert!(item(&b, "lazy").unwrap()["base"].is_null());

    // A package that does not match its hash installs nothing.
    let bytes = extensions::pack(&packages.join("kit")).unwrap();
    let target = b.root.join("check-packages");
    let mut tampered = bytes.clone();
    let last = tampered.len() - 1024;
    tampered[last] ^= 1;
    assert_eq!(
        extensions::install(&target, &hash, &tampered).unwrap_err(),
        "The package does not match its hash."
    );
    assert!(!target.exists() || std::fs::read_dir(&target).unwrap().next().is_none());
    // The same folder always packs to the same hash.
    assert_eq!(hex::encode(sha2::Sha256::digest(&bytes)), hash);
    assert!(extensions::install(&target, &hash, &bytes)
        .unwrap()
        .join("SKILL.md")
        .is_file());
}

fn with_home(node: &Node) -> std::path::PathBuf {
    let home = node.root.join("home");
    crate::home::confirm_home(&node.root, &home).unwrap();
    home.join("artifacts")
}

fn blob_read(node: &Node, hash: &str) -> Value {
    node.swarm
        .operation(Operation::BlobRead, json!({"hash": hash}), None)
        .unwrap()
}

fn artifact_hash(node: &Node, path: &str) -> String {
    node.swarm
        .records()
        .get(artifacts::CLASS, path)
        .unwrap()
        .unwrap()
        .value("hash")
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn artifacts_arrive_lazily_and_unpinned_blobs_leave_past_the_budget() {
    use base64::Engine as _;
    let (a, b, c) = fleet();
    let (from_a, on_b, on_c) = (with_home(&a), with_home(&b), with_home(&c));
    for (path, size) in [
        ("one/index.html", 1000),
        ("two/data.bin", 2000),
        ("three/big.bin", 3000),
    ] {
        let file = from_a.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![path.as_bytes()[0]; size]).unwrap();
    }
    classes(&[&a, &b, &c]);

    // Every member holds the manifest and no bytes: a member without a
    // laptop or desktop form factor prefetches nothing unpinned.
    let one = artifact_hash(&b, "one/index.html");
    for node in [&b, &c] {
        let record = node
            .swarm
            .records()
            .get(artifacts::CLASS, "two/data.bin")
            .unwrap()
            .unwrap();
        assert_eq!(record.value("owner").unwrap(), a.id.as_str());
        assert_eq!(record.value("size").unwrap(), 2000);
        assert!(pins::fetched(node.inner(), artifacts::CLASS)
            .unwrap()
            .is_empty());
    }
    assert!(!on_b.join("one/index.html").exists());

    // A read fetches the bytes.
    assert_eq!(blob_read(&b, &one)["status"], "fetching");
    classes(&[&a, &b, &c]);
    let read = blob_read(&b, &one);
    assert_eq!(read["status"], "held");
    assert_eq!(read["done"], true);
    let data = base64::engine::general_purpose::STANDARD
        .decode(read["data"].as_str().unwrap())
        .unwrap();
    assert_eq!(data, vec![b'o'; 1000]);
    // A hash no fleet item names is not readable.
    assert!(b
        .swarm
        .operation(Operation::BlobRead, json!({"hash": "0".repeat(64)}), None)
        .is_err());

    // C pins one artifact: it is written into C's artifacts folder.
    let pinned = pins::pin_set_operation(
        c.inner(),
        json!({"pins": [{"class": "artifact", "key": "two/data.bin", "pinned": true}]}),
    )
    .unwrap();
    assert_eq!(pinned["pins"][0]["key"], "two/data.bin");
    classes(&[&a, &b, &c]);
    assert_eq!(
        std::fs::read(on_c.join("two/data.bin")).unwrap(),
        vec![b't'; 2000]
    );
    assert!(!on_c.join("one/index.html").exists());
    // The copy C keeps is not C's write.
    assert_eq!(
        c.swarm
            .records()
            .get(artifacts::CLASS, "two/data.bin")
            .unwrap()
            .unwrap()
            .value("owner")
            .unwrap(),
        a.id.as_str()
    );

    // B keeps 3500 bytes: reading all three leaves the newest read, and
    // the pinned one, in the content store.
    pins::pin_set_operation(
        b.inner(),
        json!({"offline": {"budget_bytes": 3500},
               "pins": [{"class": "artifact", "key": "one/index.html", "pinned": true}]}),
    )
    .unwrap();
    let two = artifact_hash(&b, "two/data.bin");
    let three = artifact_hash(&b, "three/big.bin");
    for hash in [&two, &three] {
        blob_read(&b, hash);
        classes(&[&a, &b, &c]);
        assert_eq!(blob_read(&b, hash)["status"], "held");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    pins::evict(b.inner()).unwrap();
    let held = pins::fetched(b.inner(), artifacts::CLASS).unwrap();
    assert!(held.contains(&one), "the pinned artifact left");
    assert!(held.contains(&three), "the newest read left");
    assert!(!held.contains(&two), "the budget kept {held:?}");
    // An evicted blob comes back on the next read.
    assert_eq!(blob_read(&b, &two)["status"], "fetching");
    classes(&[&a, &b, &c]);
    assert_eq!(blob_read(&b, &two)["status"], "held");

    // C unpins: its copy goes. A deletes an artifact: it leaves the manifest.
    pins::pin_set_operation(
        c.inner(),
        json!({"pins": [{"class": "artifact", "key": "two/data.bin", "pinned": false}]}),
    )
    .unwrap();
    std::fs::remove_file(from_a.join("three/big.bin")).unwrap();
    classes(&[&a, &b, &c]);
    assert!(!on_c.join("two/data.bin").exists());
    for node in [&a, &b, &c] {
        assert!(node
            .swarm
            .records()
            .get(artifacts::CLASS, "three/big.bin")
            .unwrap()
            .is_none_or(|record| !record.is_live()));
    }

    // C writes its own artifact; A edits one of its own. Both travel.
    std::fs::create_dir_all(on_c.join("four")).unwrap();
    std::fs::write(on_c.join("four/notes.md"), "from c").unwrap();
    std::fs::write(from_a.join("one/index.html"), "edited").unwrap();
    pins::pin_set_operation(
        a.inner(),
        json!({"pins": [{"class": "artifact", "key": "*", "pinned": true}]}),
    )
    .unwrap();
    classes(&[&a, &b, &c]);
    assert_eq!(
        std::fs::read_to_string(from_a.join("four/notes.md")).unwrap(),
        "from c"
    );
    // B pinned the edited artifact, so its copy follows the edit.
    assert_eq!(
        std::fs::read_to_string(on_b.join("one/index.html")).unwrap(),
        "edited"
    );
}

#[test]
fn a_secret_for_an_agent_reaches_the_worker_that_runs_it_only_in_the_recipient_set() {
    let a = Node::new("a");
    let b = Node::new("b");
    let w = Node::new("w");
    let container = Node::new("container");
    join(&b, &a, Role::Full);
    join(&w, &a, Role::Worker);
    // A container joins as a full member unless its invitation says worker.
    join(&container, &a, Role::Full);
    settle_links(&[(&b, &a), (&w, &a), (&container, &a)]);
    let agent = uuid::Uuid::new_v4().to_string();
    let entry = DirectoryEntry {
        agent_id: agent.clone(),
        name: "Builder".into(),
        label: String::new(),
        avatar: None,
        primary_runtime: w.id.clone(),
        epoch: 0,
        updated_at: String::new(),
        deleted: false,
        primary_thread: None,
        move_suggested: false,
        definition: Some(json!({"name": "Builder"})),
        activity: None,
    };
    directory::put_entry(a.inner(), &entry, None).unwrap();

    let mut full = vec![b.id.clone(), container.id.clone()];
    full.sort();
    let set = a.swarm.recipient_set(std::slice::from_ref(&agent)).unwrap();
    assert_eq!(set.full, full);
    assert_eq!(set.workers, vec![w.id.clone()]);
    // A secret for another agent, or none, reaches no worker.
    let other = uuid::Uuid::new_v4().to_string();
    assert!(a.swarm.recipient_set(&[other]).unwrap().workers.is_empty());
    assert!(a.swarm.recipient_set(&[]).unwrap().workers.is_empty());

    // Sealing still takes the full members alone, the container among them,
    // until stage S5 enforces worker scoping.
    a.write_auth(json!({"openai": api_key("sk-from-a")}));
    a.share();
    a.pass();
    let sealed = a.sealed(&format!("models:{}", a.id)).unwrap();
    let recipients: Vec<String> = sealed.recipients.keys().cloned().collect();
    assert_eq!(recipients, full);
}

/// An authorization server that rotates refresh tokens and refuses a spent
/// one. Answers its token URL and the refresh tokens it was sent.
fn token_server() -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/token", listener.local_addr().unwrap());
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&heard);
    std::thread::spawn(move || {
        let mut current = "rt0".to_owned();
        let mut issued = 0;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let (mut line, mut length) = (String::new(), 0);
            reader.read_line(&mut line).unwrap();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let token = body
                .split('&')
                .find_map(|pair| pair.strip_prefix("refresh_token="))
                .unwrap_or_default()
                .to_owned();
            assert!(body.contains("client_id=cid") && body.contains("client_secret=shh"));
            log.lock().unwrap().push(token.clone());
            let (status, answer) = if token == current {
                issued += 1;
                current = format!("rt{issued}");
                (
                    "200 OK",
                    json!({"access_token": format!("at{issued}"), "token_type": "Bearer",
                           "expires_in": 3600, "refresh_token": current}),
                )
            } else {
                ("400 Bad Request", json!({"error": "invalid_grant"}))
            };
            let text = answer.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (url, heard)
}

fn mcp_auth(node: &Node) -> serde_json::Map<String, Value> {
    muniment_models::pi_models::read_json(&node.agent().join("mcp-auth.json")).unwrap()
}

#[test]
fn mcp_sign_ins_reach_members_as_leases_and_their_owner_alone_refreshes() {
    let (a, b, c) = fleet();
    let (url, heard) = token_server();
    let key = "mcp__Context|https://mcp.example.com/mcp";
    let now = chrono::Utc::now().timestamp_millis();
    a.write_auth(json!({}));
    muniment_models::pi_models::write_json(
        &a.agent().join("mcp-auth.json"),
        json!({key: {
            "serverUrl": "https://mcp.example.com/mcp",
            "tokens": {"access_token": "at0", "token_type": "Bearer", "expires_in": 3600,
                       "refresh_token": "rt0"},
            // Ten minutes left: inside the lease margin.
            "tokensExpireAt": now + 10 * 60_000,
            "clientInformation": {"client_id": "cid", "client_secret": "shh"},
            "discovery": {"authorizationServerMetadata": {"token_endpoint": url}},
            "codeVerifier": "verifier",
        }})
        .as_object()
        .unwrap(),
    )
    .unwrap();
    a.share();
    sync(&a, &b, &c);
    pass_all(&[&a, &b, &c]);
    sync(&a, &b, &c);
    pass_all(&[&a, &b, &c]);
    sync(&a, &b, &c);

    // A refreshed once and keeps the new refresh token.
    assert_eq!(*heard.lock().unwrap(), vec!["rt0".to_owned()]);
    let own = mcp_auth(&a)[key].clone();
    assert_eq!(own["tokens"]["access_token"], "at1");
    assert_eq!(own["tokens"]["refresh_token"], "rt1");
    for node in [&b, &c] {
        let lease = mcp_auth(node)[key].clone();
        assert_eq!(lease["tokens"]["access_token"], "at1");
        assert!(lease["tokens"].get("refresh_token").is_none(), "{lease}");
        assert!(lease["clientInformation"].get("client_secret").is_none());
        assert!(lease.get("codeVerifier").is_none());
        assert_eq!(lease["tokensExpireAt"], own["tokensExpireAt"]);
    }
    // A stops sharing: the leases go.
    credentials::credentials_operation(a.inner(), json!({"share_models": false})).unwrap();
    a.pass();
    sync(&a, &b, &c);
    for node in [&b, &c] {
        assert!(!mcp_auth(node).contains_key(key));
    }
    assert_eq!(heard.lock().unwrap().len(), 1);
}
