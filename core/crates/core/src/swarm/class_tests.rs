//! The record classes that hold secrets, extensions and blobs between three
//! in-process members: account metadata, sealed keys, MCP servers, skills
//! and plugins, artifacts and their pins.

use serde_json::{json, Value};

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
