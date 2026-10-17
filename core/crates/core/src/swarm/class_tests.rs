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
