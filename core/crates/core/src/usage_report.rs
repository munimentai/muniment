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
const ROUTER_PROVIDER: &str = "muniment-router";
/// How far apart two machines' starts of one day can lie: a daylight saving
/// change moves a fixed offset's midnight by an hour, two in a few zones.
const DAY_SKEW_MS: i64 = 2 * 60 * 60 * 1000;

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
    /// The client's offset from UTC, so days start at its midnight when
    /// `time_zone` names no zone.
    #[serde(default)]
    pub utc_offset_minutes: i32,
    /// The client's IANA time zone. Each day starts at its own midnight
    /// there, so a daylight saving change moves no run to another day. A
    /// runtime before this field refuses a request that holds it, so the
    /// desktop asks such a runtime again without it.
    #[serde(default)]
    pub time_zone: Option<String>,
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
    /// The window has no reset time and nothing spent: the upstream starts
    /// it on the next use, or its last reset has passed since the probe.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub not_started: bool,
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
    /// What the card says under its rows, such as a weekly window whose
    /// upstream reports no 5-hour window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// One row per window kind and scope, the shortest window first.
    pub windows: Vec<Limit>,
}

/// A provider no window meters, with the reason and what its runs used.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Unmetered {
    pub provider: String,
    pub label: String,
    /// `local` for a server on this machine, `key` for an API key billed by
    /// use, `pending` for a subscription whose first probe has not answered,
    /// `unread` for a subscription whose upstream reports no quota.
    pub reason: String,
    /// Where a local server listens, such as `127.0.0.1:8080`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub endpoint: String,
    /// The ids of the machines that list the provider, which [`merge`] names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub machines: Vec<String>,
    /// What the provider's finished runs used in the range.
    #[serde(default)]
    pub usage: RangeTokens,
}

/// One provider's tokens in the range, from the same receipts as the totals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RangeTokens {
    /// Every input token, cached or not.
    pub input: u64,
    pub output: u64,
    /// Input and output together.
    pub processed: u64,
    pub runs: u64,
}

/// Each unmetered provider's tokens in the range, summed from the model rows.
fn tally(unmetered: &mut [Unmetered], models: &[Row]) {
    for entry in unmetered {
        let mut usage = RangeTokens::default();
        for row in models
            .iter()
            .filter(|row| row.provider.as_deref() == Some(entry.provider.as_str()))
        {
            let tokens = row.amount.tokens;
            usage.input += tokens.input + tokens.cache_read + tokens.cache_write;
            usage.output += tokens.output;
            usage.runs += row.amount.runs;
        }
        usage.processed = usage.input + usage.output;
        entry.usage = usage;
    }
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
    /// The runs in the range this machine holds only as copies of another
    /// swarm member's runs. The totals leave them out, and [`merge`] counts
    /// them for a member whose own answer also counts its copies. None in an
    /// answer whose totals count every copy it holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replicas: Option<Vec<HeldRun>>,
}

/// One run a machine holds as a copy, with what it used, for [`merge`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeldRun {
    pub run_id: String,
    /// The swarm member that ran it.
    pub origin: String,
    /// The start of the series bucket it falls in.
    pub bucket_ms: i64,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub agent_label: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub project_label: String,
    #[serde(flatten)]
    pub amount: Amount,
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
    /// The swarm member that ran it, when this machine holds a copy.
    pub origin: Option<String>,
}

/// The provider family a receipt's provider id names. A turn on a Claude
/// Code account runs through the bridge provider, and its runs belong to
/// the Anthropic account's card.
pub fn family_id(provider: &str) -> String {
    let provider = if provider == crate::models::claude_accounts::PROVIDER {
        "anthropic"
    } else {
        family::family_for_pi_provider(provider)
            .map(|family| family.id)
            .unwrap_or(provider)
    };
    if provider.is_empty() {
        "unknown".into()
    } else {
        provider.to_owned()
    }
}

/// The provider family and model a receipt's `provider/model` names. A
/// router turn names the family after the router's own id.
pub fn split_model(model: &str) -> (String, String) {
    let (provider, rest) = model.split_once('/').unwrap_or(("", model));
    let (provider, rest) = match (provider, rest.split_once('/')) {
        (ROUTER_PROVIDER, Some((family, model))) => (family, model),
        _ => (provider, rest),
    };
    (family_id(provider), rest.to_owned())
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
        origin: completed.origin.clone(),
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
/// The bucket starts and the end of `request`'s range as of `now_ms`: in
/// its time zone when it names a known one, else at its fixed offset.
fn request_buckets(request: &Request, now_ms: i64) -> (Vec<i64>, i64) {
    match request
        .time_zone
        .as_deref()
        .and_then(|zone| crate::agent_routines::parse_time_zone(zone).ok())
    {
        Some(zone) => zone_bucket_starts(request.range, now_ms, zone),
        None => bucket_starts(request.range, now_ms, request.utc_offset_minutes),
    }
}

/// The bucket starts in `zone`: whole hours back from the current hour, or
/// each local midnight back from today's, so a day across a daylight saving
/// change lasts 23 or 25 hours.
fn zone_bucket_starts(range: Range, now_ms: i64, zone: chrono_tz::Tz) -> (Vec<i64>, i64) {
    let now = Utc
        .timestamp_millis_opt(now_ms)
        .single()
        .unwrap_or_else(Utc::now);
    if range.hourly() {
        let local = now.with_timezone(&zone);
        let into_hour = Duration::minutes(i64::from(local.minute()))
            + Duration::seconds(i64::from(local.second()))
            + Duration::nanoseconds(i64::from(local.nanosecond()));
        let last = now - into_hour;
        let starts = (0..range.buckets())
            .rev()
            .map(|back| (last - Duration::hours(back)).timestamp_millis())
            .collect();
        return (starts, (last + Duration::hours(1)).timestamp_millis());
    }
    // A midnight that a daylight saving change skips starts the day at the
    // first instant after the gap.
    let midnight = |date: chrono::NaiveDate| {
        let mut at = date.and_time(chrono::NaiveTime::MIN);
        loop {
            if let Some(start) = zone.from_local_datetime(&at).earliest() {
                return start.with_timezone(&Utc).timestamp_millis();
            }
            at += Duration::minutes(15);
        }
    };
    let today = now.with_timezone(&zone).date_naive();
    let starts = (0..range.buckets())
        .rev()
        .map(|back| midnight(today - Duration::days(back)))
        .collect();
    (starts, midnight(today + Duration::days(1)))
}

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

    /// The agent row a run counts under: its id and name, empty for none.
    fn agent_row(&self, run: &RunUsage) -> (String, String) {
        let agent = self.agent(run).cloned().unwrap_or_default();
        let label = self.agent_names.get(&agent).cloned().unwrap_or_default();
        (agent, label)
    }

    /// The project row a run counts under: its id and name, empty for none.
    fn project_row(&self, run: &RunUsage) -> (String, String) {
        let project = run
            .thread_id
            .as_ref()
            .and_then(|thread| self.thread_projects.get(thread))
            .cloned()
            .unwrap_or_default();
        let label = self
            .project_names
            .get(&project)
            .cloned()
            .unwrap_or_default();
        (project, label)
    }
}

/// Adds `amount` to the row `key` of `rows`, which takes `label` and
/// `provider` when it is new.
fn add_amount(
    rows: &mut BTreeMap<String, Row>,
    key: String,
    label: String,
    provider: Option<String>,
    amount: &Amount,
) {
    rows.entry(key.clone())
        .or_insert_with(|| Row {
            key,
            label,
            provider,
            ..Row::default()
        })
        .amount
        .add(amount);
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
    let (starts, end_ms) = request_buckets(request, now_ms);
    let start_ms = starts.first().copied().unwrap_or(end_ms);
    // A copy of another member's run counts on the member that ran it.
    let (copies, in_range): (Vec<&RunUsage>, Vec<&RunUsage>) = runs
        .iter()
        .filter(|run| run.at_ms >= start_ms && run.at_ms < end_ms)
        .partition(|run| run.origin.is_some());
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
    let bucket_of = |run: &RunUsage| {
        starts
            .partition_point(|start| *start <= run.at_ms)
            .checked_sub(1)
    };
    let replicas = copies
        .into_iter()
        .filter(|run| wanted(&run.provider))
        .filter_map(|run| {
            let mut amount = Amount::default();
            amount.record(run);
            let (agent, agent_label) = owners.agent_row(run);
            let (project, project_label) = owners.project_row(run);
            Some(HeldRun {
                run_id: run.run_id.clone(),
                origin: run.origin.clone()?,
                bucket_ms: *starts.get(bucket_of(run)?)?,
                provider: run.provider.clone(),
                model: run.model.clone(),
                agent,
                agent_label,
                project,
                project_label,
                amount,
            })
        })
        .collect();
    for run in in_range.into_iter().filter(|run| wanted(&run.provider)) {
        total.record(run);
        if let Some(bucket) = bucket_of(run).and_then(|index| series.get_mut(index)) {
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
        let (agent, agent_label) = owners.agent_row(run);
        agents
            .entry(agent.clone())
            .or_insert_with(|| Row {
                key: agent,
                label: agent_label,
                ..Row::default()
            })
            .amount
            .record(run);
        let (project, project_label) = owners.project_row(run);
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
    let models = ranked(models);
    let mut unmetered: Vec<Unmetered> = unmetered
        .into_iter()
        .filter(|entry| wanted(&entry.provider))
        .collect();
    tally(&mut unmetered, &models);
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
            model: models,
            agent: ranked(agents),
            project: ranked(projects),
            machine: Vec::new(),
        },
        providers: providers.into_iter().collect(),
        accounts: accounts(&windows, now_ms),
        pools: pools(&windows, now_ms),
        unmetered,
        windows,
        replicas: Some(replicas),
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
                not_started: resets_at_ms.is_none() && remaining >= 100.0,
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
                    ..Unmetered::default()
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
                    ..Unmetered::default()
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
                ..Unmetered::default()
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
        .filter_map(|(id, provider)| {
            let url = provider
                .get("baseUrl")
                .and_then(Value::as_str)
                .and_then(|url| url::Url::parse(url).ok())?;
            let host = url.host_str()?;
            let loopback = host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback());
            let endpoint = match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            };
            loopback.then_some((id, provider, endpoint))
        })
        .map(|(id, provider, endpoint)| Unmetered {
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
            endpoint,
            ..Unmetered::default()
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
    let (starts, _) = request_buckets(&request, now_ms);
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

/// A summary with each provider id named by its family, as a runtime that
/// names a Claude Code turn by its bridge provider answers, so one family
/// shows once.
fn by_family(mut summary: Summary) -> Summary {
    for bucket in &mut summary.series {
        let mut providers: BTreeMap<String, Amount> = BTreeMap::new();
        for (provider, amount) in std::mem::take(&mut bucket.providers) {
            providers
                .entry(family_id(&provider))
                .or_default()
                .add(&amount);
        }
        bucket.providers = providers;
    }
    let mut models = BTreeMap::new();
    for row in std::mem::take(&mut summary.breakdown.model) {
        let Some(provider) = row.provider.as_deref().map(family_id) else {
            add_rows(&mut models, vec![row]);
            continue;
        };
        let model = row
            .key
            .split_once('/')
            .map_or(row.label.as_str(), |(_, model)| model);
        let key = format!("{provider}/{model}");
        add_amount(&mut models, key, row.label, Some(provider), &row.amount);
    }
    summary.breakdown.model = models.into_values().collect();
    let providers: BTreeSet<String> = summary.providers.iter().map(|p| family_id(p)).collect();
    summary.providers = providers.into_iter().collect();
    summary
}

/// The summary of `machine` with its runs read from the copies other
/// machines hold of the runs it ran. An answer without replicas counts every
/// copy it holds of another member's runs, which that member counts too.
fn own_runs(mut summary: Summary, machine: &str, held: &BTreeMap<String, HeldRun>) -> Summary {
    let mut series: BTreeMap<i64, BTreeMap<String, Amount>> = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut agents = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut providers = BTreeSet::new();
    let mut total = Amount::default();
    for run in held.values().filter(|run| run.origin == machine) {
        total.add(&run.amount);
        series
            .entry(run.bucket_ms)
            .or_default()
            .entry(run.provider.clone())
            .or_default()
            .add(&run.amount);
        add_amount(
            &mut models,
            format!("{}/{}", run.provider, run.model),
            run.model.clone(),
            Some(run.provider.clone()),
            &run.amount,
        );
        add_amount(
            &mut agents,
            run.agent.clone(),
            run.agent_label.clone(),
            None,
            &run.amount,
        );
        add_amount(
            &mut projects,
            run.project.clone(),
            run.project_label.clone(),
            None,
            &run.amount,
        );
        providers.insert(run.provider.clone());
    }
    summary.totals = Totals::of(&total);
    summary.series = series
        .into_iter()
        .map(|(start_ms, providers)| Bucket {
            start_ms,
            providers,
        })
        .collect();
    summary.breakdown = Breakdown {
        model: ranked(models),
        agent: ranked(agents),
        project: ranked(projects),
        machine: Vec::new(),
    };
    summary.providers = providers.into_iter().collect();
    summary
}

/// Joins the answers of several machines: sums, the series by bucket, the
/// breakdowns by key, a machine row each, and each account's newest reading
/// pooled once however many machines hold it. Each run counts once, on the
/// machine that ran it: an answer leaves out the copies it holds, and an
/// answer that counts its copies takes its runs from the copies the other
/// machines hold of them, each run id once.
pub fn merge(answers: Vec<MachineAnswer>, now_ms: i64) -> FleetSummary {
    let mut held: BTreeMap<String, HeldRun> = BTreeMap::new();
    let mut copies_listed = false;
    for replicas in answers
        .iter()
        .filter_map(|answer| answer.summary.as_ref().ok()?.replicas.as_ref())
    {
        copies_listed = true;
        for run in replicas {
            held.entry(run.run_id.clone())
                .or_insert_with(|| run.clone());
        }
    }
    let mut joined = Summary::default();
    let mut total = Amount::default();
    let mut series: BTreeMap<i64, BTreeMap<String, Amount>> = BTreeMap::new();
    let mut models = BTreeMap::new();
    let mut agents = BTreeMap::new();
    let mut projects = BTreeMap::new();
    let mut machines_rows = Vec::new();
    let mut providers = BTreeSet::new();
    let mut unmetered: BTreeMap<(String, String, String, String), Unmetered> = BTreeMap::new();
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
        let summary = if summary.replicas.is_none() && copies_listed {
            own_runs(summary, &answer.id, &held)
        } else {
            summary
        };
        let summary = by_family(summary);
        if joined.range.is_none() {
            joined.range = summary.range;
            joined.start_ms = summary.start_ms;
            joined.end_ms = summary.end_ms;
            joined.bucket = summary.bucket.clone();
        }
        let mut machine = Amount::default();
        // A runtime before the time zone field starts its days at a fixed
        // offset, an hour off the zone's own midnight across a daylight
        // saving change. Its day joins the day that starts within that hour.
        let day = summary.bucket == "day";
        for bucket in summary.series {
            let start = if day {
                series
                    .range(bucket.start_ms - DAY_SKEW_MS..=bucket.start_ms + DAY_SKEW_MS)
                    .next()
                    .map_or(bucket.start_ms, |(start, _)| *start)
            } else {
                bucket.start_ms
            };
            let held = series.entry(start).or_default();
            for (provider, amount) in bucket.providers {
                machine.add(&amount);
                held.entry(provider).or_default().add(&amount);
            }
        }
        total.add(&machine);
        machines_rows.push(Row {
            key: answer.id.clone(),
            label: answer.name,
            provider: None,
            amount: machine,
        });
        add_rows(&mut models, summary.breakdown.model);
        add_rows(&mut agents, summary.breakdown.agent);
        add_rows(&mut projects, summary.breakdown.project);
        providers.extend(summary.providers);
        for entry in summary.unmetered {
            let key = (
                entry.provider.clone(),
                entry.label.clone(),
                entry.reason.clone(),
                entry.endpoint.clone(),
            );
            let held = unmetered.entry(key).or_insert_with(|| Unmetered {
                machines: Vec::new(),
                ..entry
            });
            if !held.machines.contains(&answer.id) {
                held.machines.push(answer.id.clone());
            }
        }
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
    joined.unmetered = unmetered.into_values().collect();
    tally(&mut joined.unmetered, &joined.breakdown.model);
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
            origin: None,
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
            time_zone: None,
        }
    }

    fn ms(at: &str) -> i64 {
        DateTime::parse_from_rfc3339(at).unwrap().timestamp_millis()
    }

    fn zoned(range: Range, zone: &str, offset: i32) -> Request {
        Request {
            time_zone: Some(zone.into()),
            utc_offset_minutes: offset,
            ..request(range)
        }
    }

    fn times(starts: &[i64]) -> Vec<String> {
        starts
            .iter()
            .map(|start| crate::agent_runs::timestamp(start / 1000))
            .collect()
    }

    #[test]
    fn days_start_at_the_zones_own_midnight_across_both_daylight_saving_changes() {
        // New York leaves daylight time at 06:00Z on Nov 1 and enters it at
        // 07:00Z on Mar 8.
        let (starts, end) = request_buckets(
            &zoned(Range::Week, "America/New_York", -300),
            ms("2026-11-03T15:00:00Z"),
        );
        assert_eq!(
            times(&starts),
            [
                "2026-10-28T04:00:00Z",
                "2026-10-29T04:00:00Z",
                "2026-10-30T04:00:00Z",
                "2026-10-31T04:00:00Z",
                "2026-11-01T04:00:00Z",
                "2026-11-02T05:00:00Z",
                "2026-11-03T05:00:00Z",
            ]
        );
        assert_eq!(end, ms("2026-11-04T05:00:00Z"));
        let (spring, _) = request_buckets(
            &zoned(Range::Week, "America/New_York", -240),
            ms("2026-03-09T15:00:00Z"),
        );
        assert_eq!(
            times(&spring[4..]),
            [
                "2026-03-07T05:00:00Z",
                "2026-03-08T05:00:00Z",
                "2026-03-09T04:00:00Z"
            ]
        );
        // Sydney leaves daylight time at 16:00Z on Apr 4 and enters it at
        // 16:00Z on Oct 3.
        let (autumn, _) = request_buckets(
            &zoned(Range::Week, "Australia/Sydney", 600),
            ms("2026-04-06T02:00:00Z"),
        );
        assert_eq!(
            times(&autumn[4..]),
            [
                "2026-04-03T13:00:00Z",
                "2026-04-04T13:00:00Z",
                "2026-04-05T14:00:00Z"
            ]
        );
        let (sydney_spring, _) = request_buckets(
            &zoned(Range::Week, "Australia/Sydney", 660),
            ms("2026-10-05T02:00:00Z"),
        );
        assert_eq!(
            times(&sydney_spring[4..]),
            [
                "2026-10-02T14:00:00Z",
                "2026-10-03T14:00:00Z",
                "2026-10-04T13:00:00Z"
            ]
        );
        // Hours stay whole hours of the zone, and an unknown zone keeps the
        // fixed offset.
        let (hours, end) = request_buckets(
            &zoned(Range::Day, "Asia/Kolkata", 330),
            ms("2026-11-01T15:10:00Z"),
        );
        assert_eq!(hours.len(), 24);
        assert_eq!(end, ms("2026-11-01T15:30:00Z"));
        assert_eq!(
            request_buckets(
                &zoned(Range::Week, "Mars/Base", -300),
                ms("2026-11-03T15:00:00Z")
            ),
            bucket_starts(Range::Week, ms("2026-11-03T15:00:00Z"), -300)
        );
    }

    #[test]
    fn todays_early_runs_count_on_today_after_the_change_to_standard_time() {
        let runs = [
            run(
                "early",
                ms("2026-11-01T04:10:00Z"),
                "anthropic/claude",
                1,
                None,
            ),
            run(
                "before",
                ms("2026-11-01T03:50:00Z"),
                "anthropic/claude",
                2,
                None,
            ),
        ];
        let now = ms("2026-11-01T15:00:00Z");
        let day = |request: &Request| {
            let summary = summarize(
                &runs,
                &Owners::default(),
                Vec::new(),
                Vec::new(),
                request,
                now,
            );
            summary
                .series
                .iter()
                .map(|bucket| {
                    bucket
                        .providers
                        .get("anthropic")
                        .map_or(0, |amount| amount.processed)
                })
                .collect::<Vec<_>>()
        };
        let zoned = day(&zoned(Range::Week, "America/New_York", -300));
        assert_eq!(
            &zoned[5..],
            [
                run("x", 0, "anthropic/claude", 2, None).tokens.processed(),
                run("x", 0, "anthropic/claude", 1, None).tokens.processed()
            ]
        );
        // The offset alone counts both on yesterday.
        let fixed = day(&Request {
            utc_offset_minutes: -300,
            ..request(Range::Week)
        });
        assert_eq!(fixed[6], 0);
    }

    #[test]
    fn an_older_runtimes_fixed_offset_days_join_the_zones_days() {
        let now = ms("2026-11-03T15:00:00Z");
        let used = |at: &str| run("x", ms(at), "anthropic/claude", 1, None);
        let zoned_summary = summarize(
            &[used("2026-10-30T12:00:00Z")],
            &Owners::default(),
            Vec::new(),
            Vec::new(),
            &zoned(Range::Week, "America/New_York", -300),
            now,
        );
        let fixed_summary = summarize(
            &[used("2026-10-30T12:00:00Z")],
            &Owners::default(),
            Vec::new(),
            Vec::new(),
            &Request {
                utc_offset_minutes: -300,
                ..request(Range::Week)
            },
            now,
        );
        let joined = merge(
            vec![
                MachineAnswer {
                    id: "local".into(),
                    name: "This computer".into(),
                    summary: Ok(zoned_summary),
                },
                MachineAnswer {
                    id: "hub".into(),
                    name: "Factory hub".into(),
                    summary: Ok(fixed_summary),
                },
            ],
            now,
        )
        .summary;
        assert_eq!(joined.series.len(), 7);
        let oct_30 = joined
            .series
            .iter()
            .find(|bucket| bucket.start_ms == ms("2026-10-30T04:00:00Z"))
            .unwrap();
        assert_eq!(
            oct_30.providers["anthropic"].processed,
            2 * used("2026-10-30T12:00:00Z").tokens.processed()
        );
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
        assert_eq!(summary.breakdown.model[0].key, "anthropic/claude"); // gitleaks:allow
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
        assert!(card.windows.iter().all(|row| !row.not_started));
        // The card and its rows carry no pace.
        let shown = serde_json::to_value(card).unwrap();
        assert!(shown.get("pace").is_none());
        assert!(shown["windows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row.get("pace").is_none() && row.get("elapsed_percent").is_none()));
    }

    #[test]
    fn a_full_window_without_a_reset_has_not_started() {
        let unset = |remaining: f64| AccountWindow {
            resets_at_ms: None,
            ..window("one", remaining, 0)
        };
        let fresh = limits(&[unset(100.0)], NOW);
        assert!(fresh[0].not_started);
        assert_eq!(
            serde_json::to_value(&fresh[0]).unwrap()["not_started"],
            true
        );
        // Some spent with no reset time is a reset the upstream did not name.
        let unknown = limits(&[unset(60.0)], NOW);
        assert!(!unknown[0].not_started);
        assert!(serde_json::to_value(&unknown[0])
            .unwrap()
            .get("not_started")
            .is_none());
        // A passed reset reads as a full window that starts on the next use.
        let passed = limits(&[window("one", 30.0, -HOUR)], NOW);
        assert!(passed[0].not_started && passed[0].resets_at_ms.is_none());
        assert!(!limits(&[window("one", 100.0, HOUR)], NOW)[0].not_started);
    }

    #[test]
    fn an_older_answer_with_pace_still_reads() {
        let older = json!({
            "range": "7d", "start_ms": 0, "end_ms": 0, "bucket": "day",
            "totals": Totals::default(), "series": [], "breakdown": Breakdown::default(),
            "providers": [],
            "accounts": [{"identity": "i", "provider": "openai", "account_id": "a", "name": "OpenAI", "pace": "on", "windows": [{"provider": "openai", "kind": "weekly", "remaining_percent": 50.0, "elapsed_percent": 40.0, "pace": "ahead", "status": "ok"}]}],
            "pools": [{"provider": "openai", "kind": "weekly", "remaining_percent": 50.0, "elapsed_percent": 40.0, "pace": "ahead", "status": "ok"}],
            "unmetered": [{"provider": "ollama", "label": "Ollama", "reason": "local"}],
            "windows": []
        });
        let summary: Summary = serde_json::from_value(older).unwrap();
        assert_eq!(summary.accounts[0].windows[0].remaining_percent, 50.0);
        assert_eq!(summary.unmetered[0].usage, RangeTokens::default());
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
        assert_eq!(card.status, "ok");
        let low = limits(&[window("one", 5.0, 4 * HOUR)], NOW);
        assert_eq!(low[0].status, "low");
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
    fn a_claude_code_turn_counts_under_its_anthropic_account() {
        assert_eq!(
            split_model("claude-bridge/claude-opus-4-8"),
            ("anthropic".to_owned(), "claude-opus-4-8".to_owned())
        );
        assert_eq!(
            split_model("openai-codex/gpt-6"),
            ("openai".to_owned(), "gpt-6".to_owned())
        );
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
        assert_eq!(
            unmetered
                .iter()
                .map(|entry| entry.endpoint.as_str())
                .collect::<Vec<_>>(),
            vec!["127.0.0.1:8000", "localhost:9000", "localhost:11434"]
        );
        let _ = std::fs::remove_dir_all(&agent);
    }

    fn local(provider: &str) -> Unmetered {
        Unmetered {
            provider: provider.into(),
            label: provider.into(),
            reason: "local".into(),
            endpoint: "127.0.0.1:8080".into(),
            ..Unmetered::default()
        }
    }

    #[test]
    fn each_unmetered_provider_carries_its_tokens_in_the_range() {
        let runs = vec![
            run("a", NOW - HOUR, "omlx/qwen", 50, None),
            run("b", NOW - 2 * HOUR, "omlx/gemma", 20, None),
            run("c", NOW - 3 * HOUR, "anthropic/claude", 999, Some(1.0)),
            // Outside the day, so the day leaves it out.
            run("d", NOW - 30 * HOUR, "omlx/qwen", 400, None),
        ];
        let unmetered = vec![local("omlx"), local("ollama")];
        let day = summarize(
            &runs,
            &Owners::default(),
            Vec::new(),
            unmetered.clone(),
            &request(Range::Day),
            NOW,
        );
        // Each run reads 100 cached and its own uncached input, and writes 10.
        let omlx = &day.unmetered[0];
        assert_eq!(omlx.provider, "omlx");
        assert_eq!(
            omlx.usage,
            RangeTokens {
                input: 270,
                output: 20,
                processed: 290,
                runs: 2
            }
        );
        assert_eq!(day.unmetered[1].usage, RangeTokens::default());
        let week = summarize(
            &runs,
            &Owners::default(),
            Vec::new(),
            unmetered.clone(),
            &request(Range::Week),
            NOW,
        );
        assert_eq!(week.unmetered[0].usage.runs, 3);
        assert_eq!(week.unmetered[0].usage.processed, 800);
        // The provider filter keeps only the provider asked for.
        let only = summarize(
            &runs,
            &Owners::default(),
            Vec::new(),
            unmetered,
            &Request {
                provider: Some("omlx".into()),
                ..request(Range::Day)
            },
            NOW,
        );
        assert_eq!(only.unmetered.len(), 1);
        assert_eq!(only.unmetered[0].usage.runs, 2);
    }

    #[test]
    fn machines_join_one_unmetered_provider_and_sum_its_tokens() {
        let answer = |id: &str, input: u64, servers: Vec<Unmetered>| MachineAnswer {
            id: id.into(),
            name: id.into(),
            summary: Ok(summarize(
                &[run(id, NOW - HOUR, "omlx/qwen", input, None)],
                &Owners::default(),
                Vec::new(),
                servers,
                &request(Range::Week),
                NOW,
            )),
        };
        let joined = merge(
            vec![
                answer("local", 50, vec![local("omlx")]),
                answer("studio", 150, vec![local("omlx")]),
                answer("box", 0, Vec::new()),
            ],
            NOW,
        );
        assert_eq!(joined.summary.unmetered.len(), 1);
        let omlx = &joined.summary.unmetered[0];
        assert_eq!(omlx.machines, vec!["local", "studio"]);
        assert_eq!(omlx.endpoint, "127.0.0.1:8080");
        // Three runs: 50, 150 and 0 uncached input, each with 100 cached.
        assert_eq!(
            omlx.usage,
            RangeTokens {
                input: 500,
                output: 30,
                processed: 530,
                runs: 3
            }
        );
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

    fn copied(run_id: &str, at_ms: i64, model: &str, input: u64, origin: &str) -> RunUsage {
        RunUsage {
            origin: Some(origin.into()),
            ..run(run_id, at_ms, model, input, None)
        }
    }

    fn answer(id: &str, name: &str, summary: Summary) -> MachineAnswer {
        MachineAnswer {
            id: id.into(),
            name: name.into(),
            summary: Ok(summary),
        }
    }

    /// The answer of a runtime that counts every copy it holds and names a
    /// Claude Code turn by its bridge provider, as it reaches the desktop.
    fn older_answer(runs: &[RunUsage]) -> Summary {
        let runs: Vec<RunUsage> = runs
            .iter()
            .map(|run| RunUsage {
                origin: None,
                provider: if run.model.starts_with("claude-fable") {
                    "claude-bridge".into()
                } else {
                    run.provider.clone()
                },
                ..run.clone()
            })
            .collect();
        let summary = summarize(
            &runs,
            &Owners::default(),
            Vec::new(),
            Vec::new(),
            &request(Range::Day),
            NOW,
        );
        let mut wire = serde_json::to_value(summary).unwrap();
        wire.as_object_mut().unwrap().remove("replicas");
        serde_json::from_value(wire).unwrap()
    }

    fn machine_row<'a>(joined: &'a Summary, id: &str) -> &'a Row {
        joined
            .breakdown
            .machine
            .iter()
            .find(|row| row.key == id)
            .unwrap()
    }

    #[test]
    fn a_copy_of_another_members_run_counts_on_the_member_that_ran_it() {
        let mut owners = Owners::default();
        owners.run_agents.insert("copy".into(), "agent-1".into());
        owners.agent_names.insert("agent-1".into(), "Scout".into());
        let summary = summarize(
            &[
                run("own", NOW - HOUR, "openai-codex/gpt", 50, None),
                copied("copy", NOW - HOUR, "anthropic/claude", 70, "hub-runtime"),
                copied(
                    "old",
                    NOW - 30 * HOUR,
                    "anthropic/claude",
                    70,
                    "hub-runtime",
                ),
            ],
            &owners,
            Vec::new(),
            Vec::new(),
            &request(Range::Day),
            NOW,
        );
        assert_eq!(summary.totals.runs, 1);
        assert_eq!(summary.providers, ["openai"]);
        assert_eq!(summary.breakdown.model.len(), 1);
        let replicas = summary.replicas.unwrap();
        assert_eq!(replicas.len(), 1);
        let copy = &replicas[0];
        assert_eq!(
            (copy.run_id.as_str(), copy.origin.as_str()),
            ("copy", "hub-runtime")
        );
        assert_eq!(
            (copy.agent.as_str(), copy.agent_label.as_str()),
            ("agent-1", "Scout")
        );
        assert_eq!(copy.amount.runs, 1);
        assert_eq!(copy.amount.processed, 180);
        assert!(summary
            .series
            .iter()
            .any(|bucket| bucket.start_ms == copy.bucket_ms));
    }

    #[test]
    fn an_older_member_that_counts_the_copies_it_holds_counts_each_run_once() {
        let mine = [
            run("a", NOW - HOUR, "openai-codex/gpt", 50, None),
            run(
                "b",
                NOW - 2 * HOUR,
                "claude-bridge/claude-fable-5-1",
                30,
                None,
            ),
        ];
        let hubs = run("h", NOW - 3 * HOUR, "openai-codex/gpt", 20, None);
        // This computer holds a copy of the hub's run, and the hub holds
        // copies of both of this computer's runs.
        let mut local_runs = mine.to_vec();
        local_runs.push(RunUsage {
            origin: Some("hub-runtime".into()),
            ..hubs.clone()
        });
        let local = summarize(
            &local_runs,
            &Owners::default(),
            Vec::new(),
            Vec::new(),
            &request(Range::Day),
            NOW,
        );
        let hub = older_answer(&[mine[0].clone(), mine[1].clone(), hubs.clone()]);
        assert!(hub.replicas.is_none());
        assert!(hub.providers.contains(&"claude-bridge".to_string()));
        let joined = merge(
            vec![
                answer("local", "This computer", local),
                answer("hub-runtime", "Factory hub", hub),
            ],
            NOW,
        )
        .summary;
        let processed =
            |runs: &[&RunUsage]| -> u64 { runs.iter().map(|run| run.tokens.processed()).sum() };
        assert_eq!(joined.totals.runs, 3);
        assert_eq!(
            joined.totals.processed,
            processed(&[&mine[0], &mine[1], &hubs])
        );
        assert_eq!(machine_row(&joined, "local").amount.runs, 2);
        assert_eq!(
            machine_row(&joined, "local").amount.processed,
            processed(&[&mine[0], &mine[1]])
        );
        assert_eq!(machine_row(&joined, "hub-runtime").amount.runs, 1);
        assert_eq!(
            machine_row(&joined, "hub-runtime").amount.processed,
            processed(&[&hubs])
        );
        assert_eq!(joined.providers, ["anthropic", "openai"]);
        let fable: Vec<&Row> = joined
            .breakdown
            .model
            .iter()
            .filter(|row| row.label == "claude-fable-5-1")
            .collect();
        assert_eq!(fable.len(), 1);
        assert_eq!(fable[0].key, "anthropic/claude-fable-5-1");
        assert_eq!(fable[0].amount.runs, 1);
        let series_runs: u64 = joined
            .series
            .iter()
            .flat_map(|bucket| bucket.providers.values())
            .map(|amount| amount.runs)
            .sum();
        assert_eq!(series_runs, 3);
        assert!(joined
            .series
            .iter()
            .all(|bucket| !bucket.providers.contains_key("claude-bridge")));
    }

    #[test]
    fn a_run_two_machines_hold_copies_of_counts_once_on_the_member_that_ran_it() {
        let hubs = run("h", NOW - HOUR, "openai-codex/gpt", 20, None);
        let holder = |id: &str| {
            let copy = RunUsage {
                origin: Some("hub-runtime".into()),
                ..hubs.clone()
            };
            answer(
                id,
                id,
                summarize(
                    &[copy],
                    &Owners::default(),
                    Vec::new(),
                    Vec::new(),
                    &request(Range::Day),
                    NOW,
                ),
            )
        };
        let joined = merge(
            vec![
                holder("local"),
                holder("laptop"),
                answer(
                    "hub-runtime",
                    "Factory hub",
                    older_answer(std::slice::from_ref(&hubs)),
                ),
            ],
            NOW,
        )
        .summary;
        assert_eq!(joined.totals.runs, 1);
        assert_eq!(machine_row(&joined, "hub-runtime").amount.runs, 1);
        assert_eq!(machine_row(&joined, "local").amount.runs, 0);
        assert_eq!(machine_row(&joined, "laptop").amount.runs, 0);
    }

    #[test]
    fn an_older_answer_alone_keeps_its_runs_under_one_name_per_family() {
        let joined = merge(
            vec![answer(
                "hub-runtime",
                "Factory hub",
                older_answer(&[
                    run("b", NOW - HOUR, "claude-bridge/claude-fable-5-1", 30, None),
                    run("c", NOW - HOUR, "anthropic/claude-opus", 30, None),
                ]),
            )],
            NOW,
        )
        .summary;
        assert_eq!(joined.totals.runs, 2);
        assert_eq!(joined.providers, ["anthropic"]);
        let bucket = joined
            .series
            .iter()
            .find(|bucket| !bucket.providers.is_empty())
            .unwrap();
        assert_eq!(bucket.providers.keys().collect::<Vec<_>>(), ["anthropic"]);
        assert_eq!(bucket.providers["anthropic"].runs, 2);
    }
}
