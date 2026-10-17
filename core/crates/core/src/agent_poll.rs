//! MCP routines: a tool call on a schedule, with no model, whose JSON result
//! must pass the routine's condition before the agent runs.
//!
//! Each due routine's tool is called through [`crate::extend::mcp_call`].
//! The result and any error are kept as the routine's last poll. A failed
//! poll doubles the wait before the next one, up to six hours. A `changed`
//! test compares the value at its path with the value the previous poll
//! saw, which the routine's state keeps. When the condition holds, a run is
//! queued with the result in an untrusted block.

use crate::agent_routines::{
    self, Condition, ConditionLeaf, ConditionOp, McpTrigger, PollRecord, Routine, Trigger,
};
use crate::agent_runs::{self, NewRun, Run, RunStore, RunTrigger, TriggerKind};
use crate::agents::{self, Agent};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// How long one tool call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The wait after the first failed poll. Each later failure doubles it.
pub const BASE_BACKOFF_SECONDS: i64 = 60;
/// The longest wait after failed polls.
pub const MAX_BACKOFF_SECONDS: i64 = 6 * 60 * 60;
/// The most bytes of a result the routine's state and the run's prompt keep.
pub const MAX_RESULT_BYTES: usize = 32 * 1024;
const MAX_ERROR_CHARACTERS: usize = 300;

/// Calls one routine's tool and answers its raw result.
pub type McpCall = dyn Fn(&McpTrigger) -> Result<Value, String> + Send + Sync;

/// The real call: Pi's MCP client with the servers the user installed.
pub fn installed_call(trigger: &McpTrigger) -> Result<Value, String> {
    let root = crate::state_root::state_directory().ok_or("The Muniment folder is unavailable.")?;
    crate::extend::mcp_call(
        &root,
        &trigger.server,
        &trigger.tool,
        &trigger.args,
        CALL_TIMEOUT,
    )
}

/// The JSON a condition reads from a raw tool result: the text parsed as
/// JSON, else the server's structured content, else the text as a string.
/// A tool error is an error.
pub fn tool_value(raw: &Value) -> Result<Value, String> {
    let text: String = raw["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if raw["isError"] == true {
        let message: String = text.chars().take(MAX_ERROR_CHARACTERS).collect();
        return Err(if message.trim().is_empty() {
            "The MCP tool answered with an error.".into()
        } else {
            format!("The MCP tool answered with an error: {message}")
        });
    }
    if let Ok(value) = serde_json::from_str::<Value>(text.trim()) {
        return Ok(value);
    }
    let structured = &raw["structuredContent"];
    // Pi wraps plain content as `{content}`; that is not the server's own.
    if structured.is_object() && structured.get("content") != Some(&raw["content"]) {
        return Ok(structured.clone());
    }
    Ok(Value::String(text))
}

/// Whether `condition` holds on `value`. Every `changed` test records the
/// value at its path in `seen`, and holds when `previous` has a different
/// value there. On a routine's first poll, `changed` does not hold.
pub fn evaluate(
    condition: &Condition,
    value: &Value,
    previous: &BTreeMap<String, Value>,
    seen: &mut BTreeMap<String, Value>,
) -> bool {
    match condition {
        // Every test runs, so each `changed` path records its value.
        Condition::All(all) => {
            let held: Vec<bool> = all
                .all
                .iter()
                .map(|condition| evaluate(condition, value, previous, seen))
                .collect();
            held.into_iter().all(|one| one)
        }
        Condition::Any(any) => {
            let held: Vec<bool> = any
                .any
                .iter()
                .map(|condition| evaluate(condition, value, previous, seen))
                .collect();
            held.into_iter().any(|one| one)
        }
        Condition::Leaf(leaf) => evaluate_leaf(leaf, value, previous, seen),
    }
}

fn evaluate_leaf(
    leaf: &ConditionLeaf,
    value: &Value,
    previous: &BTreeMap<String, Value>,
    seen: &mut BTreeMap<String, Value>,
) -> bool {
    let target = value.pointer(&leaf.path);
    let expected = leaf.value.as_ref();
    let number = |value: Option<&Value>| value.and_then(Value::as_f64);
    let compare = |test: fn(f64, f64) -> bool| match (number(target), number(expected)) {
        (Some(target), Some(expected)) => test(target, expected),
        _ => false,
    };
    match leaf.op {
        ConditionOp::Eq => equal(target, expected),
        ConditionOp::Ne => !equal(target, expected),
        ConditionOp::Gt => compare(|a, b| a > b),
        ConditionOp::Gte => compare(|a, b| a >= b),
        ConditionOp::Lt => compare(|a, b| a < b),
        ConditionOp::Lte => compare(|a, b| a <= b),
        ConditionOp::Contains => match (target, expected) {
            (Some(Value::String(text)), Some(Value::String(part))) => text.contains(part.as_str()),
            (Some(Value::Array(items)), Some(expected)) => {
                items.iter().any(|item| equal(Some(item), Some(expected)))
            }
            (Some(Value::Object(fields)), Some(Value::String(key))) => fields.contains_key(key),
            _ => false,
        },
        ConditionOp::Exists => target.is_some_and(|target| !target.is_null()),
        ConditionOp::Nonempty => match target {
            None | Some(Value::Null) => false,
            Some(Value::String(text)) => !text.is_empty(),
            Some(Value::Array(items)) => !items.is_empty(),
            Some(Value::Object(fields)) => !fields.is_empty(),
            Some(_) => true,
        },
        ConditionOp::Changed => {
            let current = target.cloned().unwrap_or(Value::Null);
            let changed = previous
                .get(&leaf.path)
                .is_some_and(|before| !equal(Some(before), Some(&current)));
            seen.insert(leaf.path.clone(), current);
            changed
        }
    }
}

/// JSON equality where `1` and `1.0` are equal.
fn equal(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    }
}

/// The wait before the next poll after `failures` failed polls in a row.
pub fn backoff_seconds(failures: u32) -> i64 {
    if failures == 0 {
        return 0;
    }
    BASE_BACKOFF_SECONDS
        .saturating_mul(1_i64 << failures.saturating_sub(1).min(20))
        .min(MAX_BACKOFF_SECONDS)
}

/// A result cut to [`MAX_RESULT_BYTES`] of JSON text. A larger result is
/// kept as the start of its text.
fn bounded(value: &Value) -> Value {
    let text = value.to_string();
    if text.len() <= MAX_RESULT_BYTES {
        return value.clone();
    }
    let mut end = MAX_RESULT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Value::String(format!("{}…", &text[..end]))
}

/// One pass over the MCP routines of `agents` at `now`: each enabled routine
/// that came due, on an agent this runtime runs, is polled once. Answers
/// the runs the pass queued.
pub fn poll_due(
    profile: &Path,
    agents: &[Agent],
    runs_here: &dyn Fn(&Agent) -> bool,
    now: i64,
    call: &McpCall,
) -> Result<Vec<Run>, String> {
    let mut due: Vec<(String, Routine, McpTrigger)> = Vec::new();
    agents::with_state(profile, |state| {
        for agent in agents {
            let runs = runs_here(agent);
            let routines = state.routines.entry(agent.id.clone()).or_default();
            for routine in &agent.routines {
                let Trigger::Mcp(trigger) = &routine.trigger else {
                    continue;
                };
                let saved = routines.entry(routine.id.clone()).or_default();
                let key = agent_routines::schedule_key(routine);
                let next = || {
                    agent_routines::next_cron_after(&trigger.every, agent_routines::from_unix(now))
                        .ok()
                        .map(|time| time.timestamp())
                };
                if !routine.enabled || !runs {
                    saved.next_fire_at = None;
                    saved.key = key;
                    continue;
                }
                if saved.key != key || saved.next_fire_at.is_none() {
                    saved.key = key;
                    saved.next_fire_at = next();
                    continue;
                }
                let retry = saved
                    .last_poll
                    .as_ref()
                    .and_then(|poll| poll.retry_at.as_deref())
                    .and_then(agent_runs::seconds);
                if saved.next_fire_at.is_some_and(|at| at <= now)
                    && retry.is_none_or(|at| at <= now)
                {
                    // The next poll is set before this one runs, so a pass
                    // that starts meanwhile does not poll it twice.
                    saved.next_fire_at = next();
                    due.push((agent.id.clone(), routine.clone(), trigger.clone()));
                }
            }
        }
        Ok(())
    })?;
    let mut queued = Vec::new();
    for (agent_id, routine, trigger) in due {
        let outcome = call(&trigger).and_then(|raw| tool_value(&raw));
        if let Some(run) = record_poll(profile, &agent_id, &routine, &trigger, outcome, now)? {
            queued.push(run);
        }
    }
    Ok(queued)
}

/// Records one poll's outcome and queues a run when the condition held.
fn record_poll(
    profile: &Path,
    agent_id: &str,
    routine: &Routine,
    trigger: &McpTrigger,
    outcome: Result<Value, String>,
    now: i64,
) -> Result<Option<Run>, String> {
    let store = RunStore::open(profile)?;
    let busy = store.active()?.iter().any(|run| {
        run.agent_id == agent_id && run.trigger.routine_id.as_deref() == Some(routine.id.as_str())
    });
    let fired = agents::with_state(profile, |state| {
        let saved = state
            .routines
            .entry(agent_id.to_owned())
            .or_default()
            .entry(routine.id.clone())
            .or_default();
        let failures = saved.last_poll.as_ref().map_or(0, |poll| poll.failures);
        let mut record = PollRecord {
            at: agent_runs::timestamp(now),
            ..PollRecord::default()
        };
        let fired = match &outcome {
            Ok(value) => {
                let mut seen = BTreeMap::new();
                let held = evaluate(
                    &trigger.condition,
                    value,
                    &saved.condition_values,
                    &mut seen,
                );
                saved.condition_values = seen;
                record.result = Some(bounded(value));
                record.fired = held && !busy;
                record.fired
            }
            Err(error) => {
                record.error = Some(error.chars().take(MAX_ERROR_CHARACTERS).collect());
                record.failures = failures.saturating_add(1);
                record.retry_at = Some(agent_runs::timestamp(
                    now + backoff_seconds(record.failures),
                ));
                false
            }
        };
        if fired {
            saved.last_fired_at = Some(agent_runs::timestamp(now));
        }
        saved.last_poll = Some(record);
        Ok(fired)
    })?;
    let value = match outcome {
        Ok(value) if fired => value,
        _ => return Ok(None),
    };
    let result = serde_json::to_string_pretty(&bounded(&value)).unwrap_or_default();
    let prompt = format!(
        "{}\n\n{}",
        agent_routines::routine_prompt(routine),
        agent_routines::untrusted_block(
            "mcp_result",
            &[("server", &trigger.server), ("tool", &trigger.tool)],
            &result
        )
    );
    let run = store.enqueue(NewRun {
        agent_id: agent_id.to_owned(),
        thread_id: None,
        trigger: RunTrigger {
            kind: TriggerKind::Mcp,
            routine_id: Some(routine.id.clone()),
            source: Some(format!("{}/{}", trigger.server, trigger.tool)),
        },
        prompt,
        hop: 0,
        origin_run_id: None,
    })?;
    agents::mirror_run(profile, &run)?;
    Ok(Some(run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_routines::{ConditionAll, ConditionAny, CronSchedule};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    fn leaf(path: &str, op: ConditionOp, value: Option<Value>) -> Condition {
        Condition::Leaf(ConditionLeaf {
            path: path.into(),
            op,
            value,
        })
    }

    fn holds(condition: &Condition, value: &Value) -> bool {
        evaluate(condition, value, &BTreeMap::new(), &mut BTreeMap::new())
    }

    #[test]
    fn each_condition_op_reads_the_result_at_its_path() {
        let result = json!({"total": 3, "items": [{"status": "new", "tags": ["west"]}],
            "owner": null, "name": "Acme West", "empty": [], "meta": {"region": "west"}});
        use ConditionOp::*;
        for (condition, expected) in [
            (leaf("/total", Eq, Some(json!(3.0))), true),
            (leaf("/total", Eq, Some(json!(4))), false),
            (leaf("/items/0/status", Ne, Some(json!("old"))), true),
            (leaf("/missing", Ne, Some(json!(1))), true),
            (leaf("/total", Gt, Some(json!(2))), true),
            (leaf("/total", Gt, Some(json!(3))), false),
            (leaf("/total", Gte, Some(json!(3))), true),
            (leaf("/total", Lt, Some(json!(3))), false),
            (leaf("/total", Lte, Some(json!(3))), true),
            (leaf("/name", Gt, Some(json!(1))), false),
            (leaf("/name", Contains, Some(json!("West"))), true),
            (leaf("/items/0/tags", Contains, Some(json!("west"))), true),
            (leaf("/meta", Contains, Some(json!("region"))), true),
            (leaf("/total", Contains, Some(json!(3))), false),
            (leaf("/items", Exists, None), true),
            (leaf("/owner", Exists, None), false),
            (leaf("/missing", Exists, None), false),
            (leaf("/items", Nonempty, None), true),
            (leaf("/empty", Nonempty, None), false),
            (leaf("", Nonempty, None), true),
            (leaf("/owner", Nonempty, None), false),
        ] {
            assert_eq!(holds(&condition, &result), expected, "{condition:?}");
        }
        assert!(!holds(&leaf("", Nonempty, None), &json!("")));
        let all = Condition::All(ConditionAll {
            all: vec![
                leaf("/total", Gt, Some(json!(0))),
                leaf("/owner", Exists, None),
            ],
        });
        let any = Condition::Any(ConditionAny {
            any: vec![
                leaf("/total", Gt, Some(json!(0))),
                leaf("/owner", Exists, None),
            ],
        });
        assert!(!holds(&all, &result));
        assert!(holds(&any, &result));
    }

    #[test]
    fn changed_compares_with_the_previous_poll_and_never_holds_first() {
        let condition = Condition::Any(ConditionAny {
            any: vec![
                leaf("/total", ConditionOp::Gt, Some(json!(100))),
                leaf("/items/0/id", ConditionOp::Changed, None),
            ],
        });
        let mut previous = BTreeMap::new();
        let mut poll = |value: Value| {
            let mut seen = BTreeMap::new();
            let held = evaluate(&condition, &value, &previous, &mut seen);
            previous = seen;
            held
        };
        assert!(
            !poll(json!({"total": 1, "items": [{"id": "a"}]})),
            "first poll"
        );
        assert!(!poll(json!({"total": 1, "items": [{"id": "a"}]})));
        assert!(poll(json!({"total": 1, "items": [{"id": "b"}]})));
        // A value that goes missing is a change too.
        assert!(poll(json!({"total": 1, "items": []})));
        assert!(!poll(json!({"total": 1, "items": []})));
    }

    #[test]
    fn tool_results_read_as_json_structured_content_or_text() {
        let text = |text: &str| {
            json!({"isError": false, "content": [{"type": "text", "text": text}],
            "structuredContent": {"content": [{"type": "text", "text": text}]}})
        };
        assert_eq!(
            tool_value(&text("{\"total\": 2}")).unwrap(),
            json!({"total": 2})
        );
        assert_eq!(tool_value(&text("Two leads")).unwrap(), json!("Two leads"));
        let structured = json!({"isError": false, "content": [{"type": "text", "text": "Two leads"}],
            "structuredContent": {"total": 2}});
        assert_eq!(tool_value(&structured).unwrap(), json!({"total": 2}));
        let error = json!({"isError": true, "content": [{"type": "text", "text": "Unauthorized"}]});
        assert!(tool_value(&error).unwrap_err().contains("Unauthorized"));
    }

    #[test]
    fn failed_polls_back_off_doubling_to_six_hours() {
        assert_eq!(backoff_seconds(0), 0);
        assert_eq!(backoff_seconds(1), 60);
        assert_eq!(backoff_seconds(2), 120);
        assert_eq!(backoff_seconds(5), 960);
        assert_eq!(backoff_seconds(30), MAX_BACKOFF_SECONDS);
    }

    fn mcp_agent(profile: &Path, condition: Condition) -> Agent {
        agents::save(
            profile,
            Agent {
                name: "Poller".into(),
                instructions: "Watch the lead list.".into(),
                routines: vec![Routine {
                    id: "leads".into(),
                    name: "New leads".into(),
                    enabled: true,
                    trigger: Trigger::Mcp(McpTrigger {
                        server: "crm".into(),
                        tool: "search_leads".into(),
                        args: json!({"status": "new"}),
                        every: CronSchedule {
                            expr: "*/5 * * * *".into(),
                            tz: "UTC".into(),
                        },
                        condition,
                    }),
                    prompt: "Summarize the new leads.".into(),
                    created_from: agent_routines::CreatedFrom::User,
                    last_fired_at: None,
                    last_result: None,
                    proposed: false,
                    last_poll: None,
                    next_fire_at: None,
                }],
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn a_due_poll_fires_only_when_its_condition_holds_and_backs_off_on_errors() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&root).unwrap();
        let profile = root.join("private");
        crate::home::confirm_home(&profile, &root.join("muniment")).unwrap();
        let agent = mcp_agent(&profile, leaf("/items/0/id", ConditionOp::Changed, None));
        let answers: Arc<Mutex<Vec<Result<Value, String>>>> = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(0));
        let call = {
            let answers = Arc::clone(&answers);
            let calls = Arc::clone(&calls);
            move |trigger: &McpTrigger| {
                assert_eq!(trigger.args, json!({"status": "new"}));
                *calls.lock().unwrap() += 1;
                answers.lock().unwrap().remove(0)
            }
        };
        let raw = |id: &str| {
            Ok(json!({"isError": false, "content": [{"type": "text",
                "text": json!({"items": [{"id": id}]}).to_string()}]}))
        };
        let listing = |profile: &Path| agents::list(profile).unwrap().agents;
        let here = |_: &Agent| true;
        let start = agent_runs::now();
        let next_due = |profile: &Path| {
            agents::state(profile).unwrap().routines[&agent.id]["leads"]
                .next_fire_at
                .unwrap()
        };
        // The first pass schedules the routine; nothing polls yet.
        assert!(poll_due(&profile, &listing(&profile), &here, start, &call)
            .unwrap()
            .is_empty());
        assert_eq!(*calls.lock().unwrap(), 0);
        let mut at = next_due(&profile);
        let pass = |answer: Result<Value, String>, at: i64| {
            answers.lock().unwrap().push(answer);
            poll_due(&profile, &listing(&profile), &here, at, &call).unwrap()
        };
        assert!(
            pass(raw("a"), at).is_empty(),
            "first poll records the value"
        );
        at = next_due(&profile);
        let fired = pass(raw("b"), at);
        assert_eq!(fired.len(), 1);
        let run = &fired[0];
        assert_eq!(run.trigger.kind, TriggerKind::Mcp);
        assert_eq!(run.trigger.routine_id.as_deref(), Some("leads"));
        assert!(run
            .prompt
            .contains("<mcp_result server=\"crm\" tool=\"search_leads\">"));
        assert!(run.prompt.contains("\"id\": \"b\""));
        assert!(run.prompt.contains("untrusted data"));
        let routine = &agents::read(&profile, &agent.id).unwrap().routines[0];
        let poll = routine.last_poll.as_ref().unwrap();
        assert!(poll.fired && poll.error.is_none());
        assert_eq!(poll.result.as_ref().unwrap()["items"][0]["id"], "b");
        // The run is still queued, so the next change does not queue another.
        at = next_due(&profile);
        assert!(pass(raw("c"), at).is_empty());
        let store = RunStore::open(&profile).unwrap();
        let mut ended = store.get(&run.id).unwrap().unwrap();
        ended.status = agent_runs::RunStatus::Completed;
        store.save(&mut ended).unwrap();

        // Errors back off: the next due poll waits for the retry time.
        at = next_due(&profile);
        assert!(pass(Err("Server offline.".into()), at).is_empty());
        let poll = agents::read(&profile, &agent.id).unwrap().routines[0]
            .last_poll
            .clone()
            .unwrap();
        assert_eq!(poll.failures, 1);
        assert_eq!(poll.error.as_deref(), Some("Server offline."));
        let retry = agent_runs::seconds(poll.retry_at.as_deref().unwrap()).unwrap();
        assert_eq!(retry, at + BASE_BACKOFF_SECONDS);
        let polled = *calls.lock().unwrap();
        // A due time before the retry time does not poll.
        with_next(&profile, &agent.id, retry - 10);
        assert!(
            poll_due(&profile, &listing(&profile), &here, retry - 10, &call)
                .unwrap()
                .is_empty()
        );
        assert_eq!(*calls.lock().unwrap(), polled);
        assert!(pass(Err("Still offline.".into()), retry).is_empty());
        let poll = agents::read(&profile, &agent.id).unwrap().routines[0]
            .last_poll
            .clone()
            .unwrap();
        assert_eq!(poll.failures, 2);
        assert_eq!(
            agent_runs::seconds(poll.retry_at.as_deref().unwrap()).unwrap(),
            retry + 2 * BASE_BACKOFF_SECONDS
        );
        // A success clears the failures, and the value changed from "c".
        with_next(&profile, &agent.id, retry + 2 * BASE_BACKOFF_SECONDS);
        assert_eq!(pass(raw("d"), retry + 2 * BASE_BACKOFF_SECONDS).len(), 1);
        let poll = agents::read(&profile, &agent.id).unwrap().routines[0]
            .last_poll
            .clone()
            .unwrap();
        assert_eq!((poll.failures, poll.retry_at), (0, None));

        // A routine on another machine's agent is not polled.
        let polled = *calls.lock().unwrap();
        with_next(&profile, &agent.id, at);
        assert!(poll_due(
            &profile,
            &listing(&profile),
            &|_: &Agent| false,
            at + 3600,
            &call
        )
        .unwrap()
        .is_empty());
        assert_eq!(*calls.lock().unwrap(), polled);
        let _ = std::fs::remove_dir_all(root);
    }

    fn with_next(profile: &Path, agent_id: &str, at: i64) {
        agents::with_state(profile, |state| {
            state
                .routines
                .get_mut(agent_id)
                .unwrap()
                .get_mut("leads")
                .unwrap()
                .next_fire_at = Some(at);
            Ok(())
        })
        .unwrap();
    }
}
