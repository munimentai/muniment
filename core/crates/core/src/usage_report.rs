//! Usage: what one machine's runs used, and what each subscription window
//! has left.
//!
//! `usage.summary` reads the receipts of finished runs from the run journal,
//! where each receipt sums Pi's usage on every assistant message of its run,
//! the router's last quota probe of each account, and the plan limits Claude
//! Code last reported for each Anthropic account. It answers totals, a
//! series of hourly or daily buckets by provider, a breakdown by model, agent
//! and project, one limit card per account with a row per window, and each
//! provider's pool of a window, which the composer warns about. [`merge`]
//! joins the answers of several machines into one view and adds the machine breakdown. A cost is the model catalog's estimate, never
//! a bill. Runs that fail or stop keep no receipt, so their tokens are not
//! counted.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::journal::{CompletedReceipt, RunJournal};
use crate::model_router::{config, family, quota};

/// Percent left under which a window shows as low.
pub const LOW_PERCENT: f64 = 20.0;
/// Points between spent and elapsed share that still count as on pace.
const PACE_SLACK: f64 = 5.0;
const ROUTER_PROVIDER: &str = "muniment-router";

/// The span a summary covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Range {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
    #[serde(rename = "90d")]
    Quarter,
}

impl Range {
    /// How many buckets the series holds: hours for a day, else days.
    fn buckets(self) -> i64 {
        match self {
            Self::Day => 24,
            Self::Week => 7,
            Self::Month => 30,
            Self::Quarter => 90,
        }
    }

    fn hourly(self) -> bool {
        self == Self::Day
    }
}

/// The body of `usage.summary`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub range: Range,
    /// A provider family id such as `anthropic`. None is every provider.
    #[serde(default)]
    pub provider: Option<String>,
    /// The client's offset from UTC, so days start at its midnight.
    #[serde(default)]
    pub utc_offset_minutes: i32,
}

/// Token counts, summed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl Tokens {
    fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.reasoning += other.reasoning;
    }

    /// Every token a provider read or wrote.
    pub fn processed(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
}

/// One amount: tokens, runs and the estimated cost.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Amount {
    pub tokens: Tokens,
    pub processed: u64,
    pub runs: u64,
    pub cost_usd: f64,
    /// Runs whose receipt carried no cost, so `cost_usd` leaves them out.
    pub unpriced_runs: u64,
}

impl Amount {
    fn add(&mut self, other: &Amount) {
        self.tokens.add(&other.tokens);
        self.processed = self.tokens.processed();
        self.runs += other.runs;
        self.cost_usd += other.cost_usd;
        self.unpriced_runs += other.unpriced_runs;
    }

    fn record(&mut self, run: &RunUsage) {
        self.add(&Amount {
            tokens: run.tokens,
            processed: run.tokens.processed(),
            runs: 1,
            cost_usd: run.cost_usd.unwrap_or(0.0),
            unpriced_runs: u64::from(run.cost_usd.is_none()),
        });
    }
}

/// The five numbers the page leads with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub processed: u64,
    pub cached_input: u64,
    pub uncached_input: u64,
    pub output: u64,
    /// Cached input as a percent of all input. None without input.
    pub cache_hit_percent: Option<f64>,
    pub runs: u64,
    pub cost_usd: f64,
    pub unpriced_runs: u64,
}

impl Totals {
    fn of(amount: &Amount) -> Self {
        let tokens = amount.tokens;
        let input = tokens.input + tokens.cache_read + tokens.cache_write;
        Self {
            processed: tokens.processed(),
            cached_input: tokens.cache_read,
            uncached_input: tokens.input + tokens.cache_write,
            output: tokens.output,
            cache_hit_percent: (input > 0)
                .then(|| (tokens.cache_read as f64 / input as f64 * 1000.0).round() / 10.0),
            runs: amount.runs,
            cost_usd: amount.cost_usd,
            unpriced_runs: amount.unpriced_runs,
        }
    }
}

/// One bucket of the series: its start and each provider's amount.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    pub start_ms: i64,
    pub providers: BTreeMap<String, Amount>,
}

/// One row of a breakdown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    /// The model, agent, project or machine id. Empty for runs no agent or
    /// project holds.
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(flatten)]
    pub amount: Amount,
}

/// The breakdowns, each largest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Breakdown {
    pub model: Vec<Row>,
    pub agent: Vec<Row>,
    pub project: Vec<Row>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machine: Vec<Row>,
}

/// One account's reading of one window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountWindow {
    pub account_id: String,
    pub label: String,
    pub provider: String,
    pub kind: quota::WindowKind,
    #[serde(default)]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub remaining_percent: f64,
    pub limit_reached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
    pub observed_at_ms: i64,
    /// Who the account is upstream, the same on every machine that holds it:
    /// the provider's account id, else the email. Empty when neither is
    /// known, and the account's own id stands in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// Another machine shared the account here.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
}

/// A window's provider, account identity, kind order and scope.
type WindowKey = (String, String, u8, String);

impl AccountWindow {
    /// Who the account is on every machine: its identity, else the id of
    /// the account a shared copy copies.
    fn upstream(&self) -> String {
        if self.identity.is_empty() {
            // A shared copy's id is the sender's id behind the shared prefix.
            self.account_id
                .strip_prefix(config::SHARED_ACCOUNT_PREFIX)
                .and_then(|rest| rest.split_once(':'))
                .map_or(self.account_id.as_str(), |(_, id)| id)
                .to_owned()
        } else {
            self.identity.clone()
        }
    }

    /// The key one account's window has on every machine.
    fn key(&self) -> WindowKey {
        (
            self.provider.clone(),
            self.upstream(),
            kind_order(self.kind),
            self.scope.clone(),
        )
    }
}

/// One reading per account and window, however many machines or shared
/// copies hold it: the freshest reading, under the name and id of a copy
/// from the machine where the account signed in when one is present.
pub fn distinct(windows: Vec<AccountWindow>) -> Vec<AccountWindow> {
    let mut held: BTreeMap<WindowKey, (AccountWindow, Option<AccountWindow>)> = BTreeMap::new();
    for window in windows {
        let own = (!window.shared).then(|| window.clone());
        match held.entry(window.key()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((window, own));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (freshest, signed_in) = entry.get_mut();
                if window.observed_at_ms > freshest.observed_at_ms {
                    *freshest = window;
                }
                if signed_in.is_none() {
                    *signed_in = own;
                }
            }
        }
    }
    held.into_values()
        .map(|(mut reading, signed_in)| {
            if let Some(signed_in) = signed_in {
                reading.account_id = signed_in.account_id;
                reading.label = signed_in.label;
                reading.plan = reading.plan.or(signed_in.plan);
                reading.shared = false;
            }
            reading
        })
        .collect()
}

/// The identity of a subscription sign-in, as
/// [`config::Credential::same_subscription`] matches two of them.
fn subscription_identity(credential: &config::Credential) -> String {
    let config::Credential::Subscription {
        provider,
        account_id,
        email,
        ..
    } = credential
    else {
        return String::new();
    };
    let nonempty = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    match (nonempty(account_id), nonempty(email)) {
        (Some(id), _) => format!("{provider}:account:{id}"),
        (None, Some(email)) => format!("{provider}:email:{}", email.to_lowercase()),
        (None, None) => String::new(),
    }
}

/// One subscription window of one account, or of a provider's pool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub provider: String,
    /// The account the card reads. Empty for a pool.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    /// The account's name. Empty for a pool.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub kind: quota::WindowKind,
    #[serde(default)]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<i64>,
    /// The share left: the account's, or the mean of a pool's accounts.
    pub remaining_percent: f64,
    /// The soonest reset among the accounts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
    /// The share of the window already past, where even spending would be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_percent: Option<f64>,
    /// `ahead` spends faster than the window passes, `under` slower.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace: Option<String>,
    /// `ok`, `low` under [`LOW_PERCENT`] left, or `limit` when the account,
    /// or every account of a pool, is spent.
    pub status: String,
}

/// One account's card: who it is and a row per window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountLimits {
    /// Who the account is on every machine, as [`distinct`] joins it.
    pub identity: String,
    pub provider: String,
    /// The account's id where it signed in, when a machine holds that copy.
    pub account_id: String,
    /// The provider's name, which heads the card.
    pub name: String,
    /// The account's name and plan, such as `user@example.com · max`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtitle: String,
    /// The pace of the window furthest ahead: `ahead`, `on` or `under`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pace: Option<String>,
    /// What the card says under its rows, such as a weekly window whose
    /// upstream reports no 5-hour window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// One row per window kind and scope, the shortest window first.
    pub windows: Vec<Limit>,
}

/// A provider no window meters, with the reason.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Unmetered {
    pub provider: String,
    pub label: String,
    /// `local` for a server on this machine, `key` for an API key billed by
    /// use, `pending` for a subscription whose first probe has not answered,
    /// `unread` for a subscription whose upstream reports no quota.
    pub reason: String,
}

/// One machine's answer to `usage.summary`, or the joined view.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub range: Option<Range>,
    pub start_ms: i64,
    pub end_ms: i64,
    /// `hour` or `day`.
    pub bucket: String,
    pub totals: Totals,
    pub series: Vec<Bucket>,
    pub breakdown: Breakdown,
    /// Every provider with usage in the range, before the provider filter.
    pub providers: Vec<String>,
    /// One card per account, with a row per window.
    #[serde(default)]
    pub accounts: Vec<AccountLimits>,
    /// One card per provider and window with its accounts pooled. A pool is
    /// spent only once every account in it is.
    #[serde(default)]
    pub pools: Vec<Limit>,
    pub unmetered: Vec<Unmetered>,
    /// Each account's reading, which [`merge`] pools across machines.
    pub windows: Vec<AccountWindow>,
}

/// What one finished run used.
#[derive(Clone, Debug, PartialEq)]
pub struct RunUsage {
    pub run_id: String,
    pub thread_id: Option<String>,
    pub at_ms: i64,
    pub provider: String,
    pub model: String,
    pub tokens: Tokens,
    pub cost_usd: Option<f64>,
}

/// The provider family and model a receipt's `provider/model` names. A
/// router turn names the family after the router's own id.
pub fn split_model(model: &str) -> (String, String) {
    let (provider, rest) = model.split_once('/').unwrap_or(("", model));
    let (provider, rest) = match (provider, rest.split_once('/')) {
        (ROUTER_PROVIDER, Some((family, model))) => (family, model),
        _ => (provider, rest),
    };
    let provider = family::family_for_pi_provider(provider)
        .map(|family| family.id)
        .unwrap_or(provider);
    let provider = if provider.is_empty() {
        "unknown"
    } else {
        provider
    };
    (provider.to_owned(), rest.to_owned())
}

fn count(value: &Value, field: &str) -> u64 {
    value
        .get(field)
        .and_then(Value::as_f64)
        .map_or(0, |n| n.max(0.0) as u64)
}

/// The usage a receipt records. A receipt without tokens records none.
pub fn run_usage(completed: &CompletedReceipt) -> Option<RunUsage> {
    let receipt = &completed.receipt;
    let tokens = receipt.get("tokens").filter(|tokens| tokens.is_object())?;
    let at = DateTime::parse_from_rfc3339(&completed.recorded_at).ok()?;
    let (provider, model) = split_model(receipt.get("model").and_then(Value::as_str)?);
    let cost_usd = receipt
        .get("cost_usd")
        .and_then(Value::as_f64)
        .or_else(|| receipt.get("cost").and_then(Value::as_str).and_then(cost));
    Some(RunUsage {
        run_id: completed.run_id.clone(),
        thread_id: completed.thread_id.clone(),
        at_ms: at.timestamp_millis(),
        provider,
        model,
        tokens: Tokens {
            input: count(tokens, "input"),
            output: count(tokens, "output"),
            cache_read: count(tokens, "cacheRead"),
            cache_write: count(tokens, "cacheWrite"),
            reasoning: count(tokens, "reasoning"),
        },
        cost_usd,
    })
}

/// The dollars in a receipt's cost line, such as `$0.012 est.`.
fn cost(text: &str) -> Option<f64> {
    let number: String = text
        .trim()
        .trim_start_matches('$')
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok()
}

/// The start of each bucket, oldest first, and the end of the last.
fn bucket_starts(range: Range, now_ms: i64, offset_minutes: i32) -> (Vec<i64>, i64) {
    let offset = Duration::minutes(i64::from(offset_minutes));
    let now = Utc
        .timestamp_millis_opt(now_ms)
        .single()
        .unwrap_or_else(Utc::now);
    let local = now + offset;
    let floor = if range.hourly() {
        local
            .with_minute(0)
            .and_then(|at| at.with_second(0))
            .and_then(|at| at.with_nanosecond(0))
    } else {
        local
            .with_hour(0)
            .and_then(|at| at.with_minute(0))
            .and_then(|at| at.with_second(0))
            .and_then(|at| at.with_nanosecond(0))
    }
    .unwrap_or(local);
    let step = if range.hourly() {
        Duration::hours(1)
    } else {
        Duration::days(1)
    };
    let last = floor - offset;
    let starts = (0..range.buckets())
        .rev()
        .map(|back| (last - step * back as i32).timestamp_millis())
        .collect();
    (starts, (last + step).timestamp_millis())
}

/// Who each run and thread belongs to, for the agent and project rows.
#[derive(Clone, Debug, Default)]
pub struct Owners {
    /// Run id to agent id.
    pub run_agents: HashMap<String, String>,
    /// Thread id to agent id.
    pub thread_agents: HashMap<String, String>,
    pub agent_names: HashMap<String, String>,
    /// Thread id to project id.
    pub thread_projects: HashMap<String, String>,
    pub project_names: HashMap<String, String>,
}

impl Owners {
    /// Reads the agents, their runs and the projects of `profile`. A store
    /// that cannot be read leaves its rows unattributed.
    pub fn read(profile: &Path) -> Self {
        let mut owners = Self::default();
        if let Ok(listing) = crate::agents::list(profile) {
            for agent in listing.agents {
                owners.agent_names.insert(agent.id, agent.name);
            }
            for (agent, thread) in listing.state.primary_threads {
                owners.thread_agents.insert(thread, agent);
            }
        }
        if let Ok(store) = crate::agent_runs::RunStore::open(profile) {
            if let Ok(runs) = store.agents_by_run() {
                for (run, agent, thread) in runs {
                    if let Some(thread) = thread {
                        owners.thread_agents.entry(thread).or_insert(agent.clone());
                    }
                    owners.run_agents.insert(run, agent);
                }
            }
        }
        if let Ok(catalog) = crate::projects::saved(profile) {
            owners.thread_projects = catalog.threads.into_iter().collect();
            owners.project_names = catalog.projects.into_iter().collect();
        }
        owners
    }

    fn agent(&self, run: &RunUsage) -> Option<&String> {
        self.run_agents.get(&run.run_id).or_else(|| {
            run.thread_id
                .as_ref()
                .and_then(|thread| self.thread_agents.get(thread))
        })
    }
}

/// Sums `runs` into a summary for `request`, as of `now_ms`.
pub fn summarize(
    runs: &[RunUsage],
    owners: &Owners,
    windows: Vec<AccountWindow>,
    unmetered: Vec<Unmetered>,
    request: &Request,
    now_ms: i64,
) -> Summary {
    let (starts, end_ms) = bucket_starts(request.range, now_ms, request.utc_offset_minutes);
    let start_ms = starts.first().copied().unwrap_or(end_ms);
    let in_range: Vec<&RunUsage> = runs
        .iter()
        .filter(|run| run.at_ms >= start_ms && run.at_ms < end_ms)
        .collect();
    let providers: BTreeSet<String> = in_range.iter().map(|run| run.provider.clone()).collect();
    let wanted = |provider: &str| {
        request
            .provider
            .as_deref()
            .is_none_or(|wanted| wanted == provider)
    };
    let mut total = Amount::default();
    let mut series: Vec<Bucket> = starts
        .iter()
        .map(|start| Bucket {
            start_ms: *start,
            providers: BTreeMap::new(),
        })
        .collect();
    let mut models: BTreeMap<String, Row> = BTreeMap::new();
    let mut agents: BTreeMap<String, Row> = BTreeMap::new();
    let mut projects: BTreeMap<String, Row> = BTreeMap::new();
    for run in in_range.into_iter().filter(|run| wanted(&run.provider)) {
        total.record(run);
        let index = starts.partition_point(|start| *start <= run.at_ms);
        if let Some(bucket) = index.checked_sub(1).and_then(|index| series.get_mut(index)) {
            bucket
                .providers
                .entry(run.provider.clone())
                .or_default()
                .record(run);
        }
        let model = format!("{}/{}", run.provider, run.model);
        models
            .entry(model.clone())
            .or_insert_with(|| Row {
                key: model,
                label: run.model.clone(),
                provider: Some(run.provider.clone()),
                ..Row::default()
            })
            .amount
            .record(run);
        let agent = owners.agent(run).cloned().unwrap_or_default();
        let agent_label = owners.agent_names.get(&agent).cloned().unwrap_or_default();
        agents
            .entry(agent.clone())
            .or_insert_with(|| Row {
                key: agent,
                label: agent_label,
                ..Row::default()
            })
            .amount
            .record(run);
        let project = run
            .thread_id
            .as_ref()
            .and_then(|thread| owners.thread_projects.get(thread))
            .cloned()
            .unwrap_or_default();
        let project_label = owners
            .project_names
            .get(&project)
            .cloned()
            .unwrap_or_default();
        projects
            .entry(project.clone())
            .or_insert_with(|| Row {
                key: project,
                label: project_label,
                ..Row::default()
            })
            .amount
            .record(run);
    }
    let windows = distinct(
        windows
            .into_iter()
            .filter(|window| wanted(&window.provider))
            .collect(),
    );
    Summary {
        range: Some(request.range),
        start_ms,
        end_ms,
        bucket: if request.range.hourly() {
            "hour"
        } else {
            "day"
        }
        .into(),
        totals: Totals::of(&total),
        series,
        breakdown: Breakdown {
            model: ranked(models),
            agent: ranked(agents),
            project: ranked(projects),
            machine: Vec::new(),
        },
        providers: providers.into_iter().collect(),
        accounts: accounts(&windows, now_ms),
        pools: pools(&windows, now_ms),
        unmetered: unmetered
            .into_iter()
            .filter(|entry| wanted(&entry.provider))
            .collect(),
        windows,
    }
}

fn ranked(rows: BTreeMap<String, Row>) -> Vec<Row> {
    let mut rows: Vec<Row> = rows.into_values().collect();
    rows.sort_by(|a, b| {
        b.amount
            .processed
            .cmp(&a.amount.processed)
            .then_with(|| a.key.cmp(&b.key))
    });
    rows
}

/// The length of a window kind, when it has a fixed one.
fn window_seconds(kind: quota::WindowKind) -> Option<i64> {
    match kind {
        quota::WindowKind::FiveHour => Some(5 * 60 * 60),
        quota::WindowKind::Daily => Some(24 * 60 * 60),
        quota::WindowKind::Weekly => Some(7 * 24 * 60 * 60),
        quota::WindowKind::Monthly => Some(30 * 24 * 60 * 60),
        quota::WindowKind::Other => None,
    }
}

fn kind_order(kind: quota::WindowKind) -> u8 {
    match kind {
        quota::WindowKind::FiveHour => 0,
        quota::WindowKind::Daily => 1,
        quota::WindowKind::Weekly => 2,
        quota::WindowKind::Monthly => 3,
        quota::WindowKind::Other => 4,
    }
}

/// One card per account, however many machines or shared copies hold it,
/// with a row per window kind and scope. A window whose reset has passed
/// reads as full until the next probe.
pub fn accounts(windows: &[AccountWindow], now_ms: i64) -> Vec<AccountLimits> {
    let mut held: BTreeMap<(String, String), Vec<&AccountWindow>> = BTreeMap::new();
    for window in windows {
        held.entry((window.provider.clone(), window.upstream()))
            .or_default()
            .push(window);
    }
    let mut cards: Vec<AccountLimits> = held
        .into_iter()
        .map(|((provider, identity), group)| {
            // The copy from the machine where the account signed in names it.
            let named = group
                .iter()
                .find(|window| !window.shared)
                .unwrap_or(&group[0]);
            let plan = named
                .plan
                .clone()
                .or_else(|| group.iter().find_map(|window| window.plan.clone()));
            let rows: Vec<AccountWindow> = group
                .iter()
                .map(|window| AccountWindow {
                    account_id: named.account_id.clone(),
                    label: named.label.clone(),
                    plan: plan.clone(),
                    ..(*window).clone()
                })
                .collect();
            let windows = grouped(&rows, now_ms, true);
            let pace = ["ahead", "on", "under"]
                .into_iter()
                .find(|pace| {
                    windows
                        .iter()
                        .any(|limit| limit.pace.as_deref() == Some(*pace))
                })
                .map(str::to_owned);
            let unscoped = |kind| {
                windows
                    .iter()
                    .any(|limit| limit.kind == kind && limit.scope.is_empty())
            };
            // A weekly window with no 5-hour window beside it says so, so the
            // card never reads as a 5-hour window missing from the screen.
            let note = (unscoped(quota::WindowKind::Weekly)
                && !unscoped(quota::WindowKind::FiveHour))
            .then(|| "The plan reports no 5-hour window.".to_owned());
            AccountLimits {
                name: family::family(&provider)
                    .map_or_else(|| provider.clone(), |family| family.name.to_owned()),
                subtitle: [Some(named.label.trim()), plan.as_deref().map(str::trim)]
                    .into_iter()
                    .flatten()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · "),
                identity,
                provider,
                account_id: named.account_id.clone(),
                pace,
                note,
                windows,
            }
        })
        .collect();
    cards.sort_by(|a, b| {
        (&a.provider, &a.subtitle, &a.identity).cmp(&(&b.provider, &b.subtitle, &b.identity))
    });
    cards
}

/// One card per provider, window kind and scope, with its accounts pooled.
pub fn pools(windows: &[AccountWindow], now_ms: i64) -> Vec<Limit> {
    grouped(windows, now_ms, false)
}

/// What one account's window reads as of `now_ms`: its share left, whether
/// it is spent, and its reset. A passed reset reads as a full window.
fn reading(window: &AccountWindow, now_ms: i64) -> (f64, bool, Option<i64>) {
    let reset = window.resets_at_ms.is_some_and(|at| at <= now_ms);
    if reset {
        (100.0, false, None)
    } else {
        (
            window.remaining_percent.clamp(0.0, 100.0),
            window.limit_reached,
            window.resets_at_ms,
        )
    }
}

fn grouped(windows: &[AccountWindow], now_ms: i64, by_account: bool) -> Vec<Limit> {
    let mut groups: BTreeMap<(String, String, u8, String), Vec<&AccountWindow>> = BTreeMap::new();
    for window in windows {
        groups
            .entry((
                window.provider.clone(),
                if by_account {
                    window.account_id.clone()
                } else {
                    String::new()
                },
                kind_order(window.kind),
                window.scope.clone(),
            ))
            .or_default()
            .push(window);
    }
    groups
        .into_values()
        .map(|group| {
            let readings: Vec<(f64, bool, Option<i64>)> =
                group.iter().map(|window| reading(window, now_ms)).collect();
            let first = group[0];
            let remaining =
                readings.iter().map(|(left, _, _)| left).sum::<f64>() / readings.len() as f64;
            let resets_at_ms = readings.iter().filter_map(|(_, _, at)| *at).min();
            let seconds = window_seconds(first.kind);
            let elapsed = seconds.zip(resets_at_ms).map(|(seconds, at)| {
                let left = (at - now_ms) as f64 / (seconds as f64 * 1000.0) * 100.0;
                (100.0 - left).clamp(0.0, 100.0)
            });
            let used = 100.0 - remaining;
            let pace = elapsed.map(|elapsed| {
                if used > elapsed + PACE_SLACK {
                    "ahead"
                } else if used < elapsed - PACE_SLACK {
                    "under"
                } else {
                    "on"
                }
                .to_owned()
            });
            let spent = readings.iter().all(|(_, reached, _)| *reached) || remaining <= 0.0;
            Limit {
                provider: first.provider.clone(),
                account_id: if by_account {
                    first.account_id.clone()
                } else {
                    String::new()
                },
                label: if by_account {
                    first.label.clone()
                } else {
                    String::new()
                },
                plan: if by_account { first.plan.clone() } else { None },
                kind: first.kind,
                scope: first.scope.clone(),
                window_seconds: seconds,
                remaining_percent: (remaining * 10.0).round() / 10.0,
                resets_at_ms,
                elapsed_percent: elapsed.map(|elapsed| (elapsed * 10.0).round() / 10.0),
                pace,
                status: if spent {
                    "limit"
                } else if remaining < LOW_PERCENT {
                    "low"
                } else {
                    "ok"
                }
                .into(),
            }
        })
        .collect()
}

/// Each enabled subscription's windows from the router's last probe, and the
/// providers no window meters: API keys, subscriptions whose upstream
/// reports no quota, and servers on this machine.
pub fn account_windows(agent: &Path) -> (Vec<AccountWindow>, Vec<Unmetered>) {
    let mut windows = Vec::new();
    let mut unmetered = BTreeSet::new();
    let store = quota::load(agent);
    if let Ok(router) = config::load(agent) {
        for account in router.accounts.iter().filter(|account| account.enabled) {
            let family = account.family.clone();
            let name = account
                .family()
                .map_or_else(|| family.clone(), |family| family.name.to_owned());
            if account.is_key() {
                unmetered.insert(Unmetered {
                    provider: family,
                    label: name,
                    reason: "key".into(),
                });
                continue;
            }
            let probe = store.accounts.get(&account.id);
            let Some(probe) = probe.filter(|probe| !probe.windows.is_empty()) else {
                // A reader that has not answered yet is still on its way.
                let pending = probe.is_none()
                    && account
                        .credential
                        .pi_provider()
                        .is_some_and(quota::has_reader);
                unmetered.insert(Unmetered {
                    provider: family,
                    label: name,
                    reason: if pending { "pending" } else { "unread" }.into(),
                });
                continue;
            };
            for window in &probe.windows {
                windows.push(AccountWindow {
                    account_id: account.id.clone(),
                    label: account.label.clone(),
                    provider: family.clone(),
                    kind: window.kind,
                    scope: window.scope.clone(),
                    plan: probe.plan.clone(),
                    remaining_percent: window.remaining_percent(),
                    limit_reached: window.limit_reached,
                    resets_at_ms: window.resets_at_ms,
                    observed_at_ms: probe.observed_at_ms,
                    identity: subscription_identity(&account.credential),
                    shared: account.id.starts_with(config::SHARED_ACCOUNT_PREFIX),
                });
            }
        }
    }
    claude_windows(agent, &mut windows, &mut unmetered);
    unmetered.extend(local_servers(agent));
    (windows, unmetered.into_iter().collect())
}

/// The windows Claude Code last reported for each Anthropic account it
/// signed in to.
fn claude_windows(
    agent: &Path,
    windows: &mut Vec<AccountWindow>,
    unmetered: &mut BTreeSet<Unmetered>,
) {
    for account in crate::models::claude_accounts::load(agent).accounts {
        let label = account.label();
        // One Claude account is one email in one organization.
        let identity = account
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty())
            .map_or_else(String::new, |email| {
                format!(
                    "anthropic:email:{}:{}",
                    email.to_lowercase(),
                    account.org_id.as_deref().unwrap_or_default()
                )
            });
        let Some(usage) = account
            .usage
            .as_ref()
            .filter(|usage| !usage.windows.is_empty())
        else {
            unmetered.insert(Unmetered {
                provider: "anthropic".into(),
                label: "Anthropic".into(),
                reason: match &account.usage {
                    None => "pending",
                    Some(_) => "unread",
                }
                .into(),
            });
            continue;
        };
        for window in &usage.windows {
            windows.push(AccountWindow {
                account_id: account.id.clone(),
                label: label.clone(),
                provider: "anthropic".into(),
                kind: serde_json::from_value(Value::String(window.kind.clone()))
                    .unwrap_or(quota::WindowKind::Other),
                scope: window.scope.clone(),
                plan: account.plan.clone(),
                remaining_percent: window.remaining_percent,
                limit_reached: window.limit_reached,
                resets_at_ms: window.resets_at_ms,
                observed_at_ms: usage.observed_at_ms,
                identity: identity.clone(),
                shared: false,
            });
        }
    }
}

/// The providers in Pi's `models.json` served from this machine, each under
/// the name the Models page shows: the name the user gave it, else the
/// registry's name, else its id.
fn local_servers(agent: &Path) -> Vec<Unmetered> {
    let record = crate::models::pi_models::read_record(agent).unwrap_or_default();
    let given = |id: &str| {
        record
            .get("names")
            .and_then(|names| names.get(id))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let Ok(bytes) = std::fs::read(agent.join("models.json")) else {
        return Vec::new();
    };
    let Ok(models) = serde_json::from_slice::<Value>(&bytes) else {
        return Vec::new();
    };
    let Some(providers) = models.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    providers
        .iter()
        .filter(|(id, _)| id.as_str() != ROUTER_PROVIDER)
        .filter(|(_, provider)| {
            provider
                .get("baseUrl")
                .and_then(Value::as_str)
                .and_then(|url| url::Url::parse(url).ok())
                .and_then(|url| url.host_str().map(str::to_owned))
                .is_some_and(|host| {
                    host == "localhost"
                        || host
                            .trim_matches(['[', ']'])
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())
                })
        })
        .map(|(id, provider)| Unmetered {
            provider: id.clone(),
            label: given(id)
                .or_else(|| {
                    provider
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .or_else(|| crate::models::registry::display_name(id).map(str::to_owned))
                .unwrap_or_else(|| id.clone()),
            reason: "local".into(),
        })
        .collect()
}

/// The finished runs `journal` recorded at or after `since`, for [`operation`].
pub fn receipts(journal: &mut RunJournal, since: &str) -> Result<Vec<CompletedReceipt>, String> {
    journal
        .completed_receipts(since)
        .map_err(|_| "Usage could not be read from the run history.".to_string())
}

/// Answers `usage.summary` for the runtime whose state is `profile`.
/// `receipts` reads the finished runs recorded at or after an RFC 3339 time,
/// so the caller holds its journal only while they are read.
pub fn operation(
    profile: &Path,
    receipts: impl FnOnce(&str) -> Result<Vec<CompletedReceipt>, String>,
    body: Value,
    now_ms: i64,
) -> Result<Value, String> {
    let request: Request =
        serde_json::from_value(body).map_err(|_| "The request is invalid.".to_string())?;
    let (starts, _) = bucket_starts(request.range, now_ms, request.utc_offset_minutes);
    let since = Utc
        .timestamp_millis_opt(starts.first().copied().unwrap_or(now_ms))
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let runs: Vec<RunUsage> = receipts(&since)?.iter().filter_map(run_usage).collect();
    let (windows, unmetered) = account_windows(&crate::state_root::agent_directory(profile));
    let summary = summarize(
        &runs,
        &Owners::read(profile),
        windows,
        unmetered,
        &request,
        now_ms,
    );
    serde_json::to_value(summary).map_err(|_| "Usage could not be read.".to_string())
}

/// One machine's answer for [`merge`].
#[derive(Clone, Debug)]
pub struct MachineAnswer {
    pub id: String,
    pub name: String,
    /// The machine's summary, or why it gave none.
    pub summary: Result<Summary, String>,
}

/// How each machine answered, in the joined view.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MachineState {
    pub id: String,
    pub name: String,
    /// `ok` or `error`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The joined view of several machines.
#[derive(Clone, Debug, Serialize)]
pub struct FleetSummary {
    #[serde(flatten)]
    pub summary: Summary,
    pub machines: Vec<MachineState>,
    /// Some machine gave no answer, so the totals leave it out.
    pub partial: bool,
}

fn add_rows(into: &mut BTreeMap<String, Row>, rows: Vec<Row>) {
    for row in rows {
        match into.get_mut(&row.key) {
            Some(held) => {
                held.amount.add(&row.amount);
                if held.label.is_empty() {
                    held.label = row.label;
                }
            }
            None => {
                into.insert(row.key.clone(), row);
            }
        }
    }
}

/// Joins the answers of several machines: sums, the series by bucket, the
/// breakdowns by key, a machine row each, and each account's newest reading
/// pooled once however many machines hold it.
pub fn merge(answers: Vec<MachineAnswer>, now_ms: i64) -> FleetSummary {
    let mut joined = Summary::default();
    let mut total = Amount::default();
    let mut series: BTreeMap<i64, BTreeMap<String, Amount>> = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut agents = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut machines_rows = Vec::new();
    let mut providers = BTreeSet::new();
    let mut unmetered = BTreeSet::new();
    let mut windows: Vec<AccountWindow> = Vec::new();
    let mut machines = Vec::new();
    for answer in answers {
        let summary = match answer.summary {
            Ok(summary) => summary,
            Err(error) => {
                machines.push(MachineState {
                    id: answer.id,
                    name: answer.name,
                    state: "error".into(),
                    error: Some(error),
                });
                continue;
            }
        };
        machines.push(MachineState {
            id: answer.id.clone(),
            name: answer.name.clone(),
            state: "ok".into(),
            error: None,
        });
        if joined.range.is_none() {
            joined.range = summary.range;
            joined.start_ms = summary.start_ms;
            joined.end_ms = summary.end_ms;
            joined.bucket = summary.bucket.clone();
        }
        let mut machine = Amount::default();
        for bucket in summary.series {
            let held = series.entry(bucket.start_ms).or_default();
            for (provider, amount) in bucket.providers {
                machine.add(&amount);
                held.entry(provider).or_default().add(&amount);
            }
        }
        total.add(&machine);
        machines_rows.push(Row {
            key: answer.id,
            label: answer.name,
            provider: None,
            amount: machine,
        });
        add_rows(&mut models, summary.breakdown.model);
        add_rows(&mut agents, summary.breakdown.agent);
        add_rows(&mut projects, summary.breakdown.project);
        providers.extend(summary.providers);
        unmetered.extend(summary.unmetered);
        windows.extend(summary.windows);
    }
    let partial = machines.iter().any(|machine| machine.state != "ok");
    let windows = distinct(windows);
    joined.totals = Totals::of(&total);
    joined.series = series
        .into_iter()
        .map(|(start_ms, providers)| Bucket {
            start_ms,
            providers,
        })
        .collect();
    joined.breakdown = Breakdown {
        model: ranked(models),
        agent: ranked(agents),
        project: ranked(projects),
        machine: {
            let mut rows = machines_rows;
            rows.sort_by_key(|row| std::cmp::Reverse(row.amount.processed));
            rows
        },
    };
    joined.providers = providers.into_iter().collect();
    joined.accounts = accounts(&windows, now_ms);
    joined.pools = pools(&windows, now_ms);
    joined.unmetered = unmetered.into_iter().collect();
    joined.windows = windows;
    FleetSummary {
        summary: joined,
        machines,
        partial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;
    const HOUR: i64 = 60 * 60 * 1000;

    fn completed(run_id: &str, at_ms: i64, receipt: Value) -> CompletedReceipt {
        CompletedReceipt {
            run_id: run_id.into(),
            thread_id: Some(format!("thread-{run_id}")),
            recorded_at: Utc
                .timestamp_millis_opt(at_ms)
                .unwrap()
                .to_rfc3339_opts(SecondsFormat::Millis, true),
            receipt,
        }
    }

    fn run(run_id: &str, at_ms: i64, model: &str, input: u64, cost: Option<f64>) -> RunUsage {
        run_usage(&completed(
            run_id,
            at_ms,
            json!({
                "model": model,
                "cost_usd": cost,
                "tokens": {"input": input, "output": 10, "cacheRead": 100, "cacheWrite": 0, "reasoning": 0, "total": input + 110}
            }),
        ))
        .unwrap()
    }

    fn request(range: Range) -> Request {
        Request {
            range,
            provider: None,
            utc_offset_minutes: 0,
        }
    }

    #[test]
    fn a_receipt_names_its_family_tokens_and_cost() {
        let usage = run_usage(&completed(
            "r1",
            NOW,
            json!({"model": "openai-codex/gpt-5.5", "cost": "$0.012 est.", "tokens": {"input": 5, "output": 7, "cacheRead": 3}}),
        ))
        .unwrap();
        assert_eq!(usage.provider, "openai");
        assert_eq!(usage.model, "gpt-5.5");
        assert_eq!(usage.cost_usd, Some(0.012));
        assert_eq!(usage.tokens.processed(), 15);
        let routed = run_usage(&completed(
            "r2",
            NOW,
            json!({"model": "muniment-router/anthropic/claude-opus", "tokens": {"input": 1, "output": 1}}),
        ))
        .unwrap();
        assert_eq!(
            (routed.provider.as_str(), routed.model.as_str()),
            ("anthropic", "claude-opus")
        );
        assert!(run_usage(&completed("r3", NOW, json!({"model": "a/b"}))).is_none());
    }

    #[test]
    fn a_day_sums_by_hour_provider_model_agent_and_project() {
        let runs = vec![
            run("a", NOW - HOUR, "anthropic/claude", 50, Some(0.5)),
            run("b", NOW - 2 * HOUR, "openai-codex/gpt", 20, None),
            run("c", NOW - 30 * HOUR, "anthropic/claude", 999, Some(9.0)),
        ];
        let mut owners = Owners::default();
        owners.run_agents.insert("a".into(), "agent-1".into());
        owners
            .agent_names
            .insert("agent-1".into(), "Lead scout".into());
        owners
            .thread_projects
            .insert("thread-b".into(), "p1".into());
        owners.project_names.insert("p1".into(), "Launch".into());
        let summary = summarize(
            &runs,
            &owners,
            Vec::new(),
            Vec::new(),
            &request(Range::Day),
            NOW,
        );
        assert_eq!(summary.series.len(), 24);
        assert_eq!(summary.bucket, "hour");
        assert_eq!(summary.totals.runs, 2);
        assert_eq!(summary.totals.cached_input, 200);
        assert_eq!(summary.totals.uncached_input, 70);
        assert_eq!(summary.totals.output, 20);
        assert_eq!(summary.totals.processed, 290);
        assert_eq!(summary.totals.cost_usd, 0.5);
        assert_eq!(summary.totals.unpriced_runs, 1);
        assert_eq!(summary.providers, vec!["anthropic", "openai"]);
        assert_eq!(summary.breakdown.model[0].key, "anthropic/claude");
        let agent = &summary.breakdown.agent;
        assert!(agent
            .iter()
            .any(|row| row.key == "agent-1" && row.label == "Lead scout"));
        assert!(summary
            .breakdown
            .project
            .iter()
            .any(|row| row.label == "Launch" && row.amount.runs == 1));
        let filled: u64 = summary
            .series
            .iter()
            .flat_map(|bucket| bucket.providers.values())
            .map(|amount| amount.runs)
            .sum();
        assert_eq!(filled, 2);
        let only = summarize(
            &runs,
            &owners,
            Vec::new(),
            Vec::new(),
            &Request {
                provider: Some("openai".into()),
                ..request(Range::Week)
            },
            NOW,
        );
        assert_eq!(only.totals.runs, 1);
        assert_eq!(only.series.len(), 7);
        assert_eq!(only.providers, vec!["anthropic", "openai"]);
    }

    fn window(account: &str, remaining: f64, resets_in: i64) -> AccountWindow {
        AccountWindow {
            account_id: account.into(),
            label: account.into(),
            provider: "openai".into(),
            kind: quota::WindowKind::FiveHour,
            scope: String::new(),
            plan: Some("pro".into()),
            remaining_percent: remaining,
            limit_reached: remaining <= 0.0,
            resets_at_ms: Some(NOW + resets_in),
            observed_at_ms: NOW - 1000,
            identity: String::new(),
            shared: false,
        }
    }

    /// Every account card's rows, in card order.
    fn limits(windows: &[AccountWindow], now_ms: i64) -> Vec<Limit> {
        accounts(windows, now_ms)
            .into_iter()
            .flat_map(|card| card.windows)
            .collect()
    }

    #[test]
    fn one_account_has_one_card_with_a_row_per_window() {
        // Each window is most of the way through, so each row spends under pace.
        let kind = |kind, scope: &str, remaining: f64| AccountWindow {
            kind,
            scope: scope.into(),
            label: "user@example.test".into(),
            plan: Some("max".into()),
            provider: "anthropic".into(),
            identity: "anthropic:email:user@example.test:".into(),
            ..window(
                "acct-1",
                remaining,
                if kind == quota::WindowKind::FiveHour {
                    HOUR
                } else {
                    3 * 24 * HOUR
                },
            )
        };
        let week = quota::WindowKind::Weekly;
        let cards = accounts(
            &[
                kind(week, "Fable", 50.0),
                kind(quota::WindowKind::FiveHour, "", 90.0),
                kind(week, "", 70.0),
            ],
            NOW,
        );
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(
            (card.name.as_str(), card.subtitle.as_str()),
            ("Anthropic", "user@example.test · max")
        );
        assert_eq!(card.identity, "anthropic:email:user@example.test:");
        assert_eq!(card.account_id, "acct-1");
        assert!(card.note.is_none());
        assert_eq!(
            card.windows
                .iter()
                .map(|row| (row.kind, row.scope.as_str(), row.remaining_percent))
                .collect::<Vec<_>>(),
            vec![
                (quota::WindowKind::FiveHour, "", 90.0),
                (week, "", 70.0),
                (week, "Fable", 50.0),
            ]
        );
        assert_eq!(card.pace.as_deref(), Some("under"));
        // A 5-hour row that spends faster than its window passes leads the card.
        let ahead = accounts(
            &[
                kind(week, "", 70.0),
                AccountWindow {
                    resets_at_ms: Some(NOW + 4 * HOUR),
                    ..kind(quota::WindowKind::FiveHour, "", 10.0)
                },
            ],
            NOW,
        );
        assert_eq!(ahead[0].pace.as_deref(), Some("ahead"));
    }

    #[test]
    fn two_accounts_of_one_provider_have_two_cards() {
        let cards = accounts(
            &[
                window("one", 10.0, HOUR),
                AccountWindow {
                    kind: quota::WindowKind::Weekly,
                    ..window("two", 30.0, 2 * HOUR)
                },
                window("two", 60.0, HOUR),
            ],
            NOW,
        );
        assert_eq!(
            cards
                .iter()
                .map(|card| (card.subtitle.as_str(), card.windows.len()))
                .collect::<Vec<_>>(),
            vec![("one · pro", 1), ("two · pro", 2)]
        );
        assert!(cards.iter().all(|card| card.name == "OpenAI"));
    }

    #[test]
    fn each_account_has_its_own_card_and_the_pool_pools_them() {
        let both = [window("one", 10.0, HOUR), window("two", 30.0, 2 * HOUR)];
        let cards = limits(&both, NOW);
        assert_eq!(cards.len(), 2);
        assert_eq!(
            cards
                .iter()
                .map(|card| (card.account_id.as_str(), card.remaining_percent))
                .collect::<Vec<_>>(),
            vec![("one", 10.0), ("two", 30.0)]
        );
        assert_eq!(cards[0].label, "one");
        assert_eq!(cards[0].plan.as_deref(), Some("pro"));
        assert_eq!(cards[0].status, "low");
        let pooled = pools(&both, NOW);
        assert_eq!(pooled.len(), 1);
        let card = &pooled[0];
        assert!(card.account_id.is_empty() && card.label.is_empty());
        assert_eq!(card.remaining_percent, 20.0);
        assert_eq!(card.resets_at_ms, Some(NOW + HOUR));
        assert_eq!(card.elapsed_percent, Some(80.0));
        assert_eq!(card.pace.as_deref(), Some("on"));
        assert_eq!(card.status, "ok");
        let low = limits(&[window("one", 5.0, 4 * HOUR)], NOW);
        assert_eq!(low[0].status, "low");
        assert_eq!(low[0].pace.as_deref(), Some("ahead"));
        let spent = limits(&[window("one", 0.0, HOUR)], NOW);
        assert_eq!(spent[0].status, "limit");
        let passed = limits(&[window("one", 0.0, -HOUR)], NOW);
        assert_eq!(passed[0].remaining_percent, 100.0);
        assert_eq!(passed[0].status, "ok");
        // One spent account spends its own card, not the pool.
        let one_spent = [window("one", 0.0, HOUR), window("two", 80.0, HOUR)];
        assert_eq!(limits(&one_spent, NOW)[0].status, "limit");
        assert_eq!(pools(&one_spent, NOW)[0].status, "ok");
    }

    #[test]
    fn a_weekly_window_without_a_five_hour_window_says_so() {
        let weekly = |account: &str| AccountWindow {
            kind: quota::WindowKind::Weekly,
            ..window(account, 80.0, 6 * 24 * HOUR)
        };
        let alone = accounts(&[weekly("pro")], NOW);
        assert_eq!(alone.len(), 1);
        assert_eq!(
            alone[0].note.as_deref(),
            Some("The plan reports no 5-hour window.")
        );
        // Beside a 5-hour window, both rows show and the card has no note.
        let both = accounts(&[weekly("plus"), window("plus", 50.0, HOUR)], NOW);
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].windows.len(), 2);
        assert!(both[0].note.is_none());
    }

    #[test]
    fn machines_join_with_one_reading_per_account() {
        let summary = |input: u64, observed: i64| Summary {
            windows: vec![AccountWindow {
                observed_at_ms: observed,
                remaining_percent: if observed > NOW - 1000 { 40.0 } else { 90.0 },
                ..window("shared", 0.0, HOUR)
            }],
            ..summarize(
                &[run("x", NOW - HOUR, "anthropic/claude", input, Some(1.0))],
                &Owners::default(),
                Vec::new(),
                Vec::new(),
                &request(Range::Week),
                NOW,
            )
        };
        let joined = merge(
            vec![
                MachineAnswer {
                    id: "local".into(),
                    name: "This computer".into(),
                    summary: Ok(summary(100, NOW - 5000)),
                },
                MachineAnswer {
                    id: "studio".into(),
                    name: "Studio".into(),
                    summary: Ok(summary(300, NOW)),
                },
                MachineAnswer {
                    id: "pod".into(),
                    name: "Pod".into(),
                    summary: Err("That machine is offline.".into()),
                },
            ],
            NOW,
        );
        assert!(joined.partial);
        assert_eq!(joined.summary.totals.runs, 2);
        assert_eq!(joined.summary.totals.cost_usd, 2.0);
        assert_eq!(joined.summary.breakdown.machine[0].key, "studio");
        assert_eq!(joined.summary.breakdown.model.len(), 1);
        assert_eq!(joined.summary.breakdown.model[0].amount.runs, 2);
        assert_eq!(joined.summary.accounts.len(), 1);
        assert_eq!(joined.summary.accounts[0].windows.len(), 1);
        assert_eq!(
            joined.summary.accounts[0].windows[0].remaining_percent,
            40.0
        );
        assert_eq!(joined.machines[2].state, "error");
        assert_eq!(joined.summary.series.len(), 7);
    }

    #[test]
    fn a_shared_copy_of_an_account_joins_its_own_card_on_every_machine() {
        type Reading<'a> = (quota::WindowKind, &'a str, f64);
        let copy =
            |account: &str, identity: &str, kind, remaining: f64, observed: i64| AccountWindow {
                kind,
                label: format!("{identity}@example.test"),
                identity: format!("openai-codex:account:{identity}"),
                observed_at_ms: observed,
                ..window(account, remaining, 6 * 24 * HOUR)
            };
        let shared = |window: AccountWindow| AccountWindow {
            account_id: format!("shared:device-a:{}", window.account_id),
            label: format!("{} from Laptop", window.label),
            shared: true,
            ..window
        };
        let week = quota::WindowKind::Weekly;
        let five = quota::WindowKind::FiveHour;
        // The machine where the account signed in, with a second account.
        let signed_in = vec![
            copy("acct-1", "user", week, 100.0, NOW - 5000),
            copy("acct-1", "user", five, 90.0, NOW - 5000),
            copy("acct-2", "other", week, 30.0, NOW - 5000),
        ];
        // The machine the account was shared to reads it later, with a
        // scoped window the first machine has not read yet.
        let shared_to = vec![
            shared(copy("acct-1", "user", week, 70.0, NOW - 1000)),
            shared(copy("acct-1", "user", five, 60.0, NOW - 1000)),
            shared(AccountWindow {
                scope: "Fable".into(),
                ..copy("acct-1", "user", week, 50.0, NOW - 1000)
            }),
        ];
        let answer = |id: &str, windows: Vec<AccountWindow>| MachineAnswer {
            id: id.into(),
            name: id.into(),
            summary: Ok(Summary {
                windows,
                ..Summary::default()
            }),
        };
        let joined = merge(
            vec![
                answer("laptop", signed_in),
                answer("box", shared_to.clone()),
            ],
            NOW,
        );
        let cards: Vec<(&str, &str, Vec<Reading>)> = joined
            .summary
            .accounts
            .iter()
            .map(|card| {
                (
                    card.account_id.as_str(),
                    card.subtitle.as_str(),
                    card.windows
                        .iter()
                        .map(|row| (row.kind, row.scope.as_str(), row.remaining_percent))
                        .collect(),
                )
            })
            .collect();
        // One card per account, named as where it signed in, with the
        // freshest reading of each window. The other account keeps its own card.
        assert_eq!(
            cards,
            vec![
                ("acct-2", "other@example.test · pro", vec![(week, "", 30.0)]),
                (
                    "acct-1",
                    "user@example.test · pro",
                    vec![(five, "", 60.0), (week, "", 70.0), (week, "Fable", 50.0)]
                ),
            ]
        );
        assert_eq!(joined.summary.windows.len(), 4);
        // One machine that holds both its own sign-in and a shared copy of
        // the same account shows it once.
        let mut both = shared_to;
        both.push(copy("acct-1", "user", week, 100.0, NOW - 5000));
        let one = summarize(
            &[],
            &Owners::default(),
            both,
            Vec::new(),
            &request(Range::Week),
            NOW,
        );
        assert_eq!(one.accounts.len(), 1);
        assert_eq!(one.accounts[0].subtitle, "user@example.test · pro");
        let weekly: Vec<&Limit> = one.accounts[0]
            .windows
            .iter()
            .filter(|row| row.kind == week && row.scope.is_empty())
            .collect();
        assert_eq!(weekly.len(), 1);
        assert_eq!(weekly[0].label, "user@example.test");
        // A copy without an identity, from a machine that names none, joins
        // by the sender's id behind the shared prefix.
        let bare = |window: AccountWindow| AccountWindow {
            identity: String::new(),
            ..window
        };
        let joined = merge(
            vec![
                answer(
                    "laptop",
                    vec![bare(copy("acct-1", "user", week, 100.0, NOW - 5000))],
                ),
                answer(
                    "box",
                    vec![bare(shared(copy("acct-1", "user", week, 70.0, NOW)))],
                ),
            ],
            NOW,
        );
        assert_eq!(joined.summary.accounts.len(), 1);
        assert_eq!(joined.summary.accounts[0].account_id, "acct-1");
    }

    #[test]
    fn each_signed_in_account_reads_its_own_windows_and_a_new_one_is_pending() {
        use crate::model_router::config::{Account, Credential, RouterConfig};
        let agent = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let subscription = |id: &str, family: &str, provider: &str| Account {
            id: id.into(),
            family: family.into(),
            label: format!("{id}@example.test"),
            credential: Credential::Subscription {
                provider: provider.into(),
                access: "token".into(),
                refresh: None,
                expires_ms: None,
                account_id: Some(id.into()),
                email: None,
                plan: None,
                renews_at_ms: None,
            },
            base_url: None,
            models: Vec::new(),
            enabled: true,
            weight: 1,
            overflow: false,
        };
        let config = RouterConfig {
            accounts: vec![
                subscription("first", "openai", "openai-codex"),
                subscription("second", "openai", "openai-codex"),
                subscription("grok", "xai", "xai"),
            ],
            ..RouterConfig::default()
        };
        config::save(&agent, &config).unwrap();
        let weekly = |used: f64| quota::Quota {
            plan: Some("pro".into()),
            windows: vec![quota::Window {
                kind: quota::WindowKind::Weekly,
                scope: String::new(),
                used_percent: used,
                resets_at_ms: Some(NOW + HOUR),
                limit_reached: false,
            }],
            observed_at_ms: NOW,
            ..quota::Quota::default()
        };
        let mut store = quota::QuotaStore::default();
        store.accounts.insert("first".into(), weekly(10.0));
        store.accounts.insert("second".into(), weekly(60.0));
        quota::save(&agent, &store).unwrap();
        let (windows, unmetered) = account_windows(&agent);
        assert_eq!(windows[0].identity, "openai-codex:account:first");
        assert!(!windows[0].shared);
        let cards = limits(&windows, NOW);
        assert_eq!(
            cards
                .iter()
                .map(|card| (card.label.as_str(), card.remaining_percent))
                .collect::<Vec<_>>(),
            vec![("first@example.test", 90.0), ("second@example.test", 40.0)]
        );
        assert_eq!(unmetered.len(), 1);
        assert_eq!(
            (unmetered[0].provider.as_str(), unmetered[0].reason.as_str()),
            ("xai", "pending")
        );
        let _ = std::fs::remove_dir_all(&agent);
    }

    #[test]
    fn a_local_server_shows_the_name_the_models_page_shows() {
        let agent = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            r#"{"providers":{"custom-local-mlx":{"baseUrl":"http://127.0.0.1:8000/v1"},"ollama":{"baseUrl":"http://localhost:11434/v1"},"custom-unnamed":{"baseUrl":"http://localhost:9000/v1"},"custom-remote":{"baseUrl":"http://models.example.test/v1"}}}"#,
        )
        .unwrap();
        std::fs::write(
            agent.join(crate::models::pi_models::RECORD_FILE),
            r#"{"names":{"custom-local-mlx":"Local MLX"}}"#,
        )
        .unwrap();
        let (_, unmetered) = account_windows(&agent);
        assert_eq!(
            unmetered
                .iter()
                .map(|entry| (entry.provider.as_str(), entry.label.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("custom-local-mlx", "Local MLX"),
                ("custom-unnamed", "custom-unnamed"),
                ("ollama", "Ollama"),
            ]
        );
        let _ = std::fs::remove_dir_all(&agent);
    }

    #[test]
    fn the_request_names_a_known_range() {
        let parsed: Request = serde_json::from_value(json!({"range": "30d"})).unwrap();
        assert_eq!(parsed.range, Range::Month);
        assert!(serde_json::from_value::<Request>(json!({"range": "1y"})).is_err());
    }

    #[test]
    fn the_answer_holds_every_field_the_desktop_client_reads() {
        let profile = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let answer = operation(
            &profile,
            |_| Ok(Vec::new()),
            json!({"range": "24h", "utc_offset_minutes": 0}),
            NOW,
        )
        .unwrap();
        let keys = muniment_attach::agent_answer_keys(muniment_attach::Operation::UsageSummary);
        assert!(!keys.is_empty());
        for key in keys {
            assert!(answer.get(key).is_some(), "the answer lacks {key}");
        }
        let _ = std::fs::remove_dir_all(profile);
    }
}
