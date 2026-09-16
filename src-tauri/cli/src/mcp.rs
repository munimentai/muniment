//! `muniment mcp`: the company record as one MCP server over stdio. It speaks
//! the 2026-07-28 revision and nothing older: there is no handshake, every
//! request carries its protocol version and the client's capabilities in
//! `_meta`, `server/discover` answers what the server is, every result names
//! its `resultType`, and a missing field comes back as an `input_required`
//! result the client answers by retrying the call.

use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

pub const PROTOCOL_VERSION: &str = "2026-07-28";
pub const SERVER_NAME: &str = "muniment";
const SERVER_TITLE: &str = "Muniment company record";
const WEBSITE: &str = "https://muniment.ai";
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
const TOOLS_TTL_MS: u64 = 300_000;
const DISCOVER_TTL_MS: u64 = 3_600_000;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// The twenty kinds every company starts with, and so the twenty kind views.
const KINDS: [&str; 20] = [
    "person",
    "org",
    "deal",
    "thread",
    "message",
    "ticket",
    "task",
    "project",
    "document",
    "meeting",
    "subscription",
    "invoice",
    "service",
    "incident",
    "deploy",
    "commitment",
    "decision",
    "mapping",
    "workflow",
    "view",
];

/// What the server reaches: the runtime's record operations. Each answer is
/// the attach body, `{"result": ...}` or `{"error": ...}`, and a transport
/// failure is one sentence for the agent.
pub trait RecordBackend {
    fn sql(&mut self, body: Value) -> Result<Value, String>;
    fn propose(&mut self, body: Value) -> Result<Value, String>;
    fn commit(&mut self, body: Value) -> Result<Value, String>;
}

/// Reads newline-delimited JSON-RPC from `input` until it closes and writes
/// one reply line per request. Notifications get no reply.
pub fn serve(
    backend: &mut dyn RecordBackend,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(backend, &line) {
            let mut text = serde_json::to_string(&reply)?;
            text.push('\n');
            output.write_all(text.as_bytes())?;
            output.flush()?;
        }
    }
    Ok(())
}

/// Handles one line. `None` means the line asked for no reply.
pub fn handle_line(backend: &mut dyn RecordBackend, line: &str) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(_) => return Some(error_reply(Value::Null, PARSE_ERROR, "Parse error", None)),
    };
    if message.is_array() {
        return Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: batches are not part of MCP 2026-07-28",
            None,
        ));
    }
    let Some(object) = message.as_object() else {
        return Some(error_reply(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request",
            None,
        ));
    };
    let id = object.get("id").cloned();
    let method = object.get("method").and_then(Value::as_str);
    match (id, method) {
        (None, Some(_)) => None,
        (Some(id), Some(method)) if !id.is_null() => {
            let params = object.get("params").cloned().unwrap_or(Value::Null);
            Some(handle_request(backend, id, method, &params))
        }
        (id, _) => Some(error_reply(
            id.unwrap_or(Value::Null),
            INVALID_REQUEST,
            "Invalid Request",
            None,
        )),
    }
}

fn handle_request(
    backend: &mut dyn RecordBackend,
    id: Value,
    method: &str,
    params: &Value,
) -> Value {
    if method == "initialize" {
        return error_reply(
            id,
            METHOD_NOT_FOUND,
            &format!(
                "Method not found: this server speaks MCP {PROTOCOL_VERSION}. Send server/discover, and carry {META_PROTOCOL_VERSION} and {META_CLIENT_CAPABILITIES} in _meta on every request."
            ),
            Some(json!({"supported": [PROTOCOL_VERSION]})),
        );
    }
    let meta = params.get("_meta").and_then(Value::as_object);
    let Some(version) = meta
        .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
    else {
        return error_reply(
            id,
            INVALID_PARAMS,
            &format!("Invalid params: _meta.{META_PROTOCOL_VERSION} is required"),
            None,
        );
    };
    let Some(capabilities) = meta
        .and_then(|meta| meta.get(META_CLIENT_CAPABILITIES))
        .and_then(Value::as_object)
    else {
        return error_reply(
            id,
            INVALID_PARAMS,
            &format!("Invalid params: _meta.{META_CLIENT_CAPABILITIES} is required"),
            None,
        );
    };
    if version != PROTOCOL_VERSION {
        return error_reply(
            id,
            UNSUPPORTED_PROTOCOL_VERSION,
            "Unsupported protocol version",
            Some(json!({"supported": [PROTOCOL_VERSION], "requested": version})),
        );
    }
    let client_name = meta
        .and_then(|meta| meta.get(META_CLIENT_INFO))
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    match method {
        "server/discover" => result_reply(id, discover_result()),
        "tools/list" => result_reply(id, tools_list_result()),
        "tools/call" => match tools_call(backend, params, capabilities, client_name) {
            Ok(result) => result_reply(id, result),
            Err((code, message)) => error_reply(id, code, &message, None),
        },
        _ => error_reply(
            id,
            METHOD_NOT_FOUND,
            &format!("Method not found: {method}"),
            None,
        ),
    }
}

fn server_info() -> Value {
    json!({
        "name": SERVER_NAME,
        "title": SERVER_TITLE,
        "version": env!("CARGO_PKG_VERSION"),
        "websiteUrl": WEBSITE,
    })
}

fn result_reply(id: Value, mut result: Value) -> Value {
    if let Some(object) = result.as_object_mut() {
        object
            .entry("resultType")
            .or_insert_with(|| Value::String("complete".to_owned()));
        object.insert("_meta".to_owned(), json!({META_SERVER_INFO: server_info()}));
    }
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_reply(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

fn discover_result() -> Value {
    json!({
        "resultType": "complete",
        "supportedVersions": [PROTOCOL_VERSION],
        "capabilities": {"tools": {}},
        "instructions": instructions(),
        "ttlMs": DISCOVER_TTL_MS,
        "cacheScope": "private",
    })
}

fn instructions() -> String {
    format!(
        "The company record: one SQLite graph per company, held by the Muniment desktop runtime. \
Read it with sql, a read-only query over the views {} and edges_open, entity_identities and recent_events, \
plus the tables kind, kind_extension, entity, identity, edge, event, fact_source and principal. \
Change it with propose, which validates and returns a diff, then commit, which applies that diff. \
Nothing else writes a row.",
        KINDS.iter().map(|kind| format!("v_{kind}")).collect::<Vec<_>>().join(", ")
    )
}

/// The tools in a fixed order, so a client can cache the list.
fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "commit",
            "title": "Commit a proposal",
            "description": "Applies one proposal from propose in one transaction and appends one event. A second commit of the same proposal returns the first result and writes nothing. Proposals expire after one hour.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proposal": {"type": "string", "description": "The proposal id propose returned."},
                    "company_id": {"type": "string", "description": "A company id. Omit it for the current company."}
                },
                "required": ["proposal"],
                "additionalProperties": false
            },
            "annotations": {"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }),
        json!({
            "name": "propose",
            "title": "Propose a change",
            "description": "Validates a create, update, link or merge against the kind catalogue, resolves every reference, and returns a diff with a proposal id and warnings. It writes nothing. A missing required field comes back as a question. References read entity:<id> or <identity kind>:<value>, such as email:elena@northwind.example or domain:northwind.example.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "op": {"type": "string", "enum": ["create", "update", "link", "merge"]},
                    "kind": {"type": "string", "description": "create: the kind, such as person, org or deal."},
                    "data": {"type": "object", "description": "create and update: the properties, such as {\"name\": \"Northwind\"}. On update a null removes a property."},
                    "identities": {"type": "array", "items": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["email", "domain", "phone", "handle", "external", "name_key"]}, "value": {"type": "string"}}, "required": ["kind", "value"]}, "description": "create and update: identities the entity holds."},
                    "links": {"type": "array", "items": {"type": "object", "properties": {"relation": {"type": "string"}, "target": {"type": "string"}, "props": {"type": "object"}}, "required": ["relation", "target"]}, "description": "create: edges from the new entity."},
                    "entity": {"type": "string", "description": "update: the entity reference."},
                    "src": {"type": "string", "description": "link: the source reference."},
                    "relation": {"type": "string", "description": "link: one of the seventeen relations."},
                    "dst": {"type": "string", "description": "link: the destination reference."},
                    "props": {"type": "object", "description": "link: edge properties."},
                    "loser": {"type": "string", "description": "merge: the entity that is superseded."},
                    "survivor": {"type": "string", "description": "merge: the entity that stays."},
                    "company_id": {"type": "string", "description": "A company id. Omit it for the current company."}
                },
                "required": ["op"]
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false}
        }),
        json!({
            "name": "sql",
            "title": "Query the company record",
            "description": format!("Runs one read-only SQLite statement on the company record and returns CSV. Results are cut at 500 rows or 64 KiB and a query stops after 5 seconds. Views: {} and edges_open, entity_identities, recent_events. Tables: kind, kind_extension, entity, identity, edge, event, fact_source, principal, entity_search (FTS5 over title and body_text). Entity properties live in json_extract(data, '$.name'); the views expose them as columns.", KINDS.iter().map(|kind| format!("v_{kind}")).collect::<Vec<_>>().join(", ")),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sql": {"type": "string", "description": "One SELECT or WITH statement."},
                    "company_id": {"type": "string", "description": "A company id. Omit it for the current company."}
                },
                "required": ["sql"],
                "additionalProperties": false
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "columns": {"type": "array", "items": {"type": "string"}},
                    "csv": {"type": "string"},
                    "row_count": {"type": "integer"},
                    "truncated": {"type": "boolean"},
                    "elapsed_ms": {"type": "integer"}
                },
                "required": ["columns", "csv", "row_count", "truncated", "elapsed_ms"]
            },
            "annotations": {"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false}
        }),
    ]
}

fn tools_list_result() -> Value {
    json!({
        "resultType": "complete",
        "tools": tools(),
        "ttlMs": TOOLS_TTL_MS,
        "cacheScope": "private",
    })
}

fn client_supports_form_elicitation(capabilities: &Map<String, Value>) -> bool {
    match capabilities.get("elicitation") {
        Some(Value::Object(modes)) => modes.is_empty() || modes.contains_key("form"),
        _ => false,
    }
}

fn tool_error(text: impl Into<String>) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text.into()}],
        "isError": true,
    })
}

fn tool_ok(text: String, structured: Value) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": false,
    })
}

fn tools_call(
    backend: &mut dyn RecordBackend,
    params: &Value,
    capabilities: &Map<String, Value>,
    client_name: Option<String>,
) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).ok_or_else(|| {
        (
            INVALID_PARAMS,
            "Invalid params: name is required".to_owned(),
        )
    })?;
    let mut arguments = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(arguments)) => arguments.clone(),
        Some(_) => {
            return Err((
                INVALID_PARAMS,
                "Invalid params: arguments must be an object".to_owned(),
            ))
        }
    };
    let company_id = arguments.remove("company_id");
    let mut body = Map::new();
    if let Some(company_id) = company_id.filter(|id| id.is_string()) {
        body.insert("company_id".to_owned(), company_id);
    }
    if let Some(client) = client_name {
        body.insert("client".to_owned(), Value::String(client));
    }
    match name {
        "sql" => {
            let Some(sql) = arguments.get("sql").and_then(Value::as_str) else {
                return Ok(tool_error("sql needs one statement in the sql argument."));
            };
            body.insert("sql".to_owned(), Value::String(sql.to_owned()));
            let answer = match backend.sql(Value::Object(body)) {
                Ok(answer) => answer,
                Err(message) => return Ok(tool_error(message)),
            };
            if let Some(error) = answer.get("error") {
                return Ok(tool_error(error_text(error)));
            }
            let result = answer.get("result").cloned().unwrap_or(Value::Null);
            let mut text = result
                .get("csv")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if result.get("truncated").and_then(Value::as_bool) == Some(true) {
                text.push_str("\n(cut at the row or byte cap; narrow the query)\n");
            }
            Ok(tool_ok(text, result))
        }
        "propose" => {
            let inputs = params
                .get("inputResponses")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            for (field, response) in inputs {
                match response.get("action").and_then(Value::as_str) {
                    Some("accept") => {
                        let content = response.get("content").and_then(Value::as_object);
                        let value = content
                            .and_then(|content| content.get(&field).cloned())
                            .or_else(|| {
                                content
                                    .filter(|content| content.len() == 1)
                                    .and_then(|content| content.values().next().cloned())
                            });
                        if let Some(value) = value {
                            let data = arguments
                                .entry("data".to_owned())
                                .or_insert_with(|| Value::Object(Map::new()));
                            if let Some(data) = data.as_object_mut() {
                                data.insert(field, value);
                            }
                        }
                    }
                    Some(action) => {
                        return Ok(tool_error(format!(
                            "The user chose {action} for {field}, so the proposal stands down."
                        )))
                    }
                    None => {}
                }
            }
            // A model that writes the properties under `props`, the link
            // argument's name, meant `data` on a create or an update.
            let op = arguments
                .get("op")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if matches!(op.as_str(), "create" | "update") && !arguments.contains_key("data") {
                if let Some(props) = arguments.remove("props") {
                    arguments.insert("data".to_owned(), props);
                }
            }
            body.insert("operation".to_owned(), Value::Object(arguments));
            let answer = match backend.propose(Value::Object(body)) {
                Ok(answer) => answer,
                Err(message) => return Ok(tool_error(message)),
            };
            if let Some(error) = answer.get("error") {
                let is_missing =
                    error.get("code").and_then(Value::as_str) == Some("missing_required");
                let field = error.get("field").and_then(Value::as_str);
                if let (true, Some(field)) = (is_missing, field) {
                    if client_supports_form_elicitation(capabilities) {
                        let prompt = error
                            .get("prompt")
                            .and_then(Value::as_str)
                            .unwrap_or("A required field is missing.");
                        let schema = elicitation_schema(error.get("schema"));
                        return Ok(json!({
                            "resultType": "input_required",
                            "inputRequests": {
                                field: {
                                    "method": "elicitation/create",
                                    "params": {
                                        "mode": "form",
                                        "message": prompt,
                                        "requestedSchema": {
                                            "type": "object",
                                            "properties": {field: schema},
                                            "required": [field]
                                        }
                                    }
                                }
                            }
                        }));
                    }
                }
                return Ok(tool_error(error_text(error)));
            }
            let proposal = answer.get("proposal").cloned().unwrap_or(Value::Null);
            let mut text = format!(
                "Proposal {} ready to commit.",
                proposal.get("id").and_then(Value::as_str).unwrap_or("?")
            );
            if let Some(warnings) = proposal.get("warnings").and_then(Value::as_array) {
                for warning in warnings.iter().filter_map(Value::as_str) {
                    text.push_str("\nWarning: ");
                    text.push_str(warning);
                }
            }
            if let Ok(diff) = serde_json::to_string_pretty(&proposal["diff"]) {
                text.push('\n');
                text.push_str(&diff);
            }
            Ok(tool_ok(text, proposal))
        }
        "commit" => {
            let Some(proposal) = arguments.get("proposal").and_then(Value::as_str) else {
                return Ok(tool_error(
                    "commit needs the proposal id in the proposal argument.",
                ));
            };
            body.insert("proposal".to_owned(), Value::String(proposal.to_owned()));
            let answer = match backend.commit(Value::Object(body)) {
                Ok(answer) => answer,
                Err(message) => return Ok(tool_error(message)),
            };
            if let Some(error) = answer.get("error") {
                return Ok(tool_error(error_text(error)));
            }
            let result = answer.get("result").cloned().unwrap_or(Value::Null);
            let text = format!(
                "Committed as event {} on {}.",
                result
                    .get("event_seq")
                    .and_then(Value::as_i64)
                    .unwrap_or_default(),
                result
                    .get("entity_ids")
                    .and_then(Value::as_array)
                    .map(|ids| ids
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", "))
                    .unwrap_or_default()
            );
            Ok(tool_ok(text, result))
        }
        other => Err((INVALID_PARAMS, format!("Unknown tool: {other}"))),
    }
}

fn error_text(error: &Value) -> String {
    match error.get("message").and_then(Value::as_str) {
        Some(message) => message.to_owned(),
        None => error.to_string(),
    }
}

/// Form elicitation takes a flat object of primitives, so a kind property
/// becomes one primitive schema. Arrays and objects become a string.
fn elicitation_schema(property: Option<&Value>) -> Value {
    let mut schema = Map::new();
    let kind = property
        .and_then(|property| property.get("type"))
        .and_then(Value::as_str)
        .unwrap_or("string");
    match kind {
        "number" | "integer" | "boolean" => {
            schema.insert("type".to_owned(), Value::String(kind.to_owned()));
        }
        _ => {
            schema.insert("type".to_owned(), Value::String("string".to_owned()));
        }
    }
    if let Some(values) = property.and_then(|property| property.get("enum")) {
        schema.insert("enum".to_owned(), values.clone());
    }
    if let Some(format) = property
        .and_then(|property| property.get("format"))
        .and_then(Value::as_str)
        .filter(|format| ["email", "uri", "date", "date-time"].contains(format))
    {
        schema.insert("format".to_owned(), Value::String(format.to_owned()));
    }
    if let Some(description) = property.and_then(|property| property.get("description")) {
        schema.insert("description".to_owned(), description.clone());
    }
    Value::Object(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeBackend {
        calls: Vec<(String, Value)>,
        sql_answer: Option<Value>,
        propose_answer: Option<Value>,
        commit_answer: Option<Value>,
        fail: Option<String>,
    }

    impl RecordBackend for FakeBackend {
        fn sql(&mut self, body: Value) -> Result<Value, String> {
            self.calls.push(("sql".into(), body));
            self.fail.clone().map_or_else(
                || Ok(self.sql_answer.clone().unwrap_or(json!({"result": {"columns": ["n"], "csv": "n\n1\n", "row_count": 1, "truncated": false, "elapsed_ms": 1}}))),
                Err,
            )
        }
        fn propose(&mut self, body: Value) -> Result<Value, String> {
            self.calls.push(("propose".into(), body));
            Ok(self.propose_answer.clone().unwrap_or(
                json!({"proposal": {"id": "p1", "warnings": [], "diff": {"op": "create"}}}),
            ))
        }
        fn commit(&mut self, body: Value) -> Result<Value, String> {
            self.calls.push(("commit".into(), body));
            Ok(self
                .commit_answer
                .clone()
                .unwrap_or(json!({"result": {"event_seq": 7, "entity_ids": ["e1"]}})))
        }
    }

    fn meta() -> Value {
        json!({
            META_PROTOCOL_VERSION: PROTOCOL_VERSION,
            META_CLIENT_INFO: {"name": "claude-code", "version": "2.0"},
            META_CLIENT_CAPABILITIES: {"elicitation": {"form": {}}},
        })
    }

    fn request(id: u64, method: &str, mut params: Value) -> String {
        params["_meta"] = meta();
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
    }

    #[test]
    fn discover_names_the_version_the_tools_and_the_server() {
        let mut backend = FakeBackend::default();
        let reply = handle_line(&mut backend, &request(1, "server/discover", json!({}))).unwrap();
        assert_eq!(reply["id"], 1);
        let result = &reply["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["supportedVersions"], json!([PROTOCOL_VERSION]));
        assert_eq!(result["capabilities"]["tools"], json!({}));
        assert_eq!(result["_meta"][META_SERVER_INFO]["name"], SERVER_NAME);
        assert_eq!(result["cacheScope"], "private");
        assert!(result["ttlMs"].as_u64().unwrap() > 0);
        assert!(result["instructions"].as_str().unwrap().contains("v_deal"));
    }

    #[test]
    fn every_request_needs_the_meta_fields_and_the_one_version() {
        let mut backend = FakeBackend::default();
        let bare =
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}).to_string();
        let reply = handle_line(&mut backend, &bare).unwrap();
        assert_eq!(reply["error"]["code"], INVALID_PARAMS);
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains(META_PROTOCOL_VERSION));

        let no_capabilities = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list",
            "params": {"_meta": {META_PROTOCOL_VERSION: PROTOCOL_VERSION}}})
        .to_string();
        let reply = handle_line(&mut backend, &no_capabilities).unwrap();
        assert_eq!(reply["error"]["code"], INVALID_PARAMS);

        let old = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list",
            "params": {"_meta": {META_PROTOCOL_VERSION: "2025-11-25", META_CLIENT_CAPABILITIES: {}}}}).to_string();
        let reply = handle_line(&mut backend, &old).unwrap();
        assert_eq!(reply["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
        assert_eq!(
            reply["error"]["data"]["supported"],
            json!([PROTOCOL_VERSION])
        );
        assert_eq!(reply["error"]["data"]["requested"], "2025-11-25");
    }

    #[test]
    fn a_legacy_initialize_is_refused_and_names_the_version() {
        let mut backend = FakeBackend::default();
        let legacy = json!({"jsonrpc": "2.0", "id": 5, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "x", "version": "1"}}}).to_string();
        let reply = handle_line(&mut backend, &legacy).unwrap();
        assert_eq!(reply["error"]["code"], METHOD_NOT_FOUND);
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains(PROTOCOL_VERSION));
        assert_eq!(
            reply["error"]["data"]["supported"],
            json!([PROTOCOL_VERSION])
        );
        let reply = handle_line(&mut backend, &request(6, "ping", json!({}))).unwrap();
        assert_eq!(reply["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn notifications_batches_and_broken_lines_answer_as_the_spec_says() {
        let mut backend = FakeBackend::default();
        assert!(handle_line(
            &mut backend,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#
        )
        .is_none());
        let reply = handle_line(&mut backend, "{not json").unwrap();
        assert_eq!(reply["error"]["code"], PARSE_ERROR);
        assert!(reply["id"].is_null());
        let reply = handle_line(&mut backend, "[]").unwrap();
        assert_eq!(reply["error"]["code"], INVALID_REQUEST);
        let reply = handle_line(
            &mut backend,
            r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#,
        )
        .unwrap();
        assert_eq!(reply["error"]["code"], INVALID_REQUEST);
        assert!(backend.calls.is_empty());
    }

    #[test]
    fn tools_list_is_ordered_cacheable_and_complete() {
        let mut backend = FakeBackend::default();
        let reply = handle_line(&mut backend, &request(7, "tools/list", json!({}))).unwrap();
        let result = &reply["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["ttlMs"], TOOLS_TTL_MS);
        assert_eq!(result["cacheScope"], "private");
        let names: Vec<&str> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["commit", "propose", "sql"]);
        for tool in result["tools"].as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert!(tool["description"].as_str().unwrap().len() > 20);
        }
        assert_eq!(result["tools"][2]["annotations"]["readOnlyHint"], true);
        assert_eq!(result["tools"][2]["outputSchema"]["required"][1], "csv");
        let again = handle_line(&mut backend, &request(8, "tools/list", json!({}))).unwrap();
        assert_eq!(again["result"]["tools"], result["tools"]);
    }

    #[test]
    fn sql_returns_csv_text_and_structured_content_and_names_the_client() {
        let mut backend = FakeBackend::default();
        let reply = handle_line(
            &mut backend,
            &request(
                9,
                "tools/call",
                json!({"name": "sql", "arguments": {"sql": "select 1 as n", "company_id": "c1"}}),
            ),
        )
        .unwrap();
        let result = &reply["result"];
        assert_eq!(result["resultType"], "complete");
        assert_eq!(result["isError"], false);
        assert_eq!(result["content"][0]["text"], "n\n1\n");
        assert_eq!(result["structuredContent"]["row_count"], 1);
        assert_eq!(
            backend.calls[0].1,
            json!({"sql": "select 1 as n", "company_id": "c1", "client": "claude-code"})
        );

        backend.sql_answer =
            Some(json!({"error": {"code": "sql", "message": "no such table: nope"}}));
        let reply = handle_line(
            &mut backend,
            &request(
                10,
                "tools/call",
                json!({"name": "sql", "arguments": {"sql": "select * from nope"}}),
            ),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(reply["result"]["content"][0]["text"], "no such table: nope");

        let reply = handle_line(
            &mut backend,
            &request(11, "tools/call", json!({"name": "sql", "arguments": {}})),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], true);

        backend.fail = Some("the Muniment desktop attach service is unavailable".into());
        let reply = handle_line(
            &mut backend,
            &request(
                12,
                "tools/call",
                json!({"name": "sql", "arguments": {"sql": "select 1"}}),
            ),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unavailable"));

        let reply = handle_line(
            &mut backend,
            &request(13, "tools/call", json!({"name": "nope", "arguments": {}})),
        )
        .unwrap();
        assert_eq!(reply["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn a_missing_field_becomes_an_input_required_result_and_the_retry_carries_the_answer() {
        let mut backend = FakeBackend {
            propose_answer: Some(
                json!({"error": {"code": "missing_required", "message": "stage is required", "field": "stage",
                "prompt": "stage is required for kind deal. What is it?", "schema": {"type": "string", "enum": ["discovery", "won"]}}}),
            ),
            ..FakeBackend::default()
        };
        let call = json!({"name": "propose", "arguments": {"op": "create", "kind": "deal", "data": {"name": "Renewal"}}});
        let reply = handle_line(&mut backend, &request(14, "tools/call", call.clone())).unwrap();
        let result = &reply["result"];
        assert_eq!(result["resultType"], "input_required");
        let ask = &result["inputRequests"]["stage"];
        assert_eq!(ask["method"], "elicitation/create");
        assert_eq!(ask["params"]["mode"], "form");
        assert_eq!(
            ask["params"]["message"],
            "stage is required for kind deal. What is it?"
        );
        assert_eq!(
            ask["params"]["requestedSchema"]["properties"]["stage"]["enum"],
            json!(["discovery", "won"])
        );
        assert_eq!(
            ask["params"]["requestedSchema"]["required"],
            json!(["stage"])
        );
        assert!(result.get("requestState").is_none());

        backend.propose_answer = None;
        let mut retry = call.clone();
        retry["inputResponses"] =
            json!({"stage": {"action": "accept", "content": {"stage": "won"}}});
        let reply = handle_line(&mut backend, &request(15, "tools/call", retry)).unwrap();
        assert_eq!(reply["result"]["resultType"], "complete");
        assert_eq!(reply["result"]["structuredContent"]["id"], "p1");
        let sent = &backend.calls[1].1;
        assert_eq!(sent["operation"]["data"]["stage"], "won");
        assert_eq!(sent["operation"]["op"], "create");
        assert_eq!(sent["client"], "claude-code");

        let props = json!({"name": "propose", "arguments": {"op": "create", "kind": "org", "props": {"name": "Northwind"}}});
        handle_line(&mut backend, &request(16, "tools/call", props)).unwrap();
        let sent = &backend.calls[2].1;
        assert_eq!(sent["operation"]["data"]["name"], "Northwind");
        assert!(sent["operation"].get("props").is_none());

        let mut declined = call.clone();
        declined["inputResponses"] = json!({"stage": {"action": "decline"}});
        let reply = handle_line(&mut backend, &request(17, "tools/call", declined)).unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(backend.calls.len(), 3);
    }

    #[test]
    fn without_elicitation_a_missing_field_is_a_tool_error() {
        let mut backend = FakeBackend {
            propose_answer: Some(
                json!({"error": {"code": "missing_required", "message": "stage is required for kind deal. What is it?", "field": "stage", "prompt": "stage is required for kind deal. What is it?"}}),
            ),
            ..FakeBackend::default()
        };
        let mut params = json!({"name": "propose", "arguments": {"op": "create", "kind": "deal"}});
        params["_meta"] =
            json!({META_PROTOCOL_VERSION: PROTOCOL_VERSION, META_CLIENT_CAPABILITIES: {}});
        let line = json!({"jsonrpc": "2.0", "id": 17, "method": "tools/call", "params": params})
            .to_string();
        let reply = handle_line(&mut backend, &line).unwrap();
        assert_eq!(reply["result"]["resultType"], "complete");
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("stage"));
        assert!(backend.calls[0].1.get("client").is_none());
    }

    #[test]
    fn commit_forwards_the_proposal_and_reports_the_event() {
        let mut backend = FakeBackend::default();
        let reply = handle_line(
            &mut backend,
            &request(
                18,
                "tools/call",
                json!({"name": "commit", "arguments": {"proposal": "p1"}}),
            ),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(
            reply["result"]["content"][0]["text"],
            "Committed as event 7 on e1."
        );
        assert_eq!(
            backend.calls[0].1,
            json!({"proposal": "p1", "client": "claude-code"})
        );
        backend.commit_answer =
            Some(json!({"error": {"code": "proposal_expired", "message": "proposal p1 expired"}}));
        let reply = handle_line(
            &mut backend,
            &request(
                19,
                "tools/call",
                json!({"name": "commit", "arguments": {"proposal": "p1"}}),
            ),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(reply["result"]["content"][0]["text"], "proposal p1 expired");
    }

    #[test]
    fn serve_writes_one_line_per_request_and_skips_notifications() {
        let mut backend = FakeBackend::default();
        let input = format!(
            "{}\n{}\n\n{}\n",
            request(20, "server/discover", json!({})),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            request(21, "tools/list", json!({}))
        );
        let mut output = Vec::new();
        serve(&mut backend, input.as_bytes(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(text.ends_with('\n'));
        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["id"], 20);
        let second: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["id"], 21);
        assert!(lines.iter().all(|line| !line.contains('\r')));
    }

    #[test]
    fn elicitation_schemas_stay_primitive() {
        assert_eq!(
            elicitation_schema(Some(&json!({"type": "array", "items": {"type": "string"}}))),
            json!({"type": "string"})
        );
        assert_eq!(
            elicitation_schema(Some(&json!({"type": "string", "format": "date"}))),
            json!({"type": "string", "format": "date"})
        );
        assert_eq!(
            elicitation_schema(Some(&json!({"type": "number", "description": "USD"}))),
            json!({"type": "number", "description": "USD"})
        );
        assert_eq!(elicitation_schema(None), json!({"type": "string"}));
    }
}
