//! The gateway's settings: a TOML file, then environment overrides.
//!
//! Every setting has an environment variable named `MUNIMENT_ROUTER_` plus the
//! setting in upper case, with each section name joined by an underscore:
//! `listen` is `MUNIMENT_ROUTER_LISTEN`, `otlp.endpoint` is
//! `MUNIMENT_ROUTER_OTLP_ENDPOINT`. The variable wins over the file. The
//! `otlp` settings also read the standard OpenTelemetry variables, below the
//! gateway's own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::config::Classifier;
use crate::policy::Mode;
use crate::store::SuccessWindow;

use super::otlp;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct File {
    listen: Option<String>,
    admin_token: Option<String>,
    run_token_signing_key: Option<String>,
    database_url: Option<String>,
    state_dir: Option<PathBuf>,
    credentials_dir: Option<PathBuf>,
    catalog: Option<PathBuf>,
    catalog_poll_s: Option<u64>,
    policy_mode: Option<String>,
    drain_timeout_s: Option<u64>,
    quota_probe_interval_s: Option<u64>,
    metrics: Option<bool>,
    success: SuccessFile,
    otlp: OtlpFile,
    classifier: ClassifierFile,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SuccessFile {
    half_life_days: Option<f64>,
    min_samples: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct OtlpFile {
    endpoint: Option<String>,
    traces_endpoint: Option<String>,
    headers: Option<String>,
    service_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ClassifierFile {
    kind: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    family: Option<String>,
}

/// Where account state lives.
#[derive(Debug, Clone, PartialEq)]
pub enum Storage {
    Postgres(String),
    /// The desktop's JSON files in one directory, for development.
    Files(PathBuf),
}

/// The settings the server runs with.
#[derive(Debug, Clone)]
pub struct Settings {
    pub listen: String,
    pub admin_token: Option<String>,
    pub run_token_signing_key: Option<String>,
    pub storage: Option<Storage>,
    /// The credential store directory that holds account credentials on a
    /// Postgres store. `None` reads them from the environment alone.
    pub credentials_dir: Option<PathBuf>,
    pub catalog: Option<PathBuf>,
    pub catalog_poll: Duration,
    pub policy_mode: Mode,
    pub drain_timeout: Duration,
    /// Zero turns the background quota probe off.
    pub quota_probe_interval: Duration,
    pub metrics: bool,
    pub success: SuccessWindow,
    pub classifier: Classifier,
    /// OTLP trace export, on when an endpoint is set.
    pub otlp: Option<otlp::Settings>,
}

/// The environment variable for one setting.
pub fn variable(name: &str) -> String {
    format!(
        "MUNIMENT_ROUTER_{}",
        name.to_ascii_uppercase().replace('.', "_")
    )
}

/// The standard OpenTelemetry variables the `otlp` settings fall back to.
const OTEL_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";
const OTEL_TRACES_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
const OTEL_HEADERS: &str = "OTEL_EXPORTER_OTLP_HEADERS";
const OTEL_SERVICE_NAME: &str = "OTEL_SERVICE_NAME";
const OTEL_SDK_DISABLED: &str = "OTEL_SDK_DISABLED";

fn mode(text: &str) -> Result<Mode, String> {
    match text.trim() {
        "current" => Ok(Mode::Current),
        "strong" => Ok(Mode::Strong),
        "ratchet" => Ok(Mode::Ratchet),
        "adaptive" => Ok(Mode::Adaptive),
        other => Err(format!(
            "policy_mode {other:?} is not current, strong, ratchet or adaptive."
        )),
    }
}

impl Settings {
    /// Reads `path` when given, then applies `env` over it.
    pub fn load(path: Option<&Path>, env: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let file: File = match path {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?
            }
            None => File::default(),
        };
        let get = |name: &str, value: Option<String>| -> Option<String> {
            env(&variable(name))
                .or(value)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let number = |name: &str, value: Option<u64>| -> Result<Option<u64>, String> {
            match env(&variable(name)).filter(|value| !value.trim().is_empty()) {
                Some(text) => text
                    .trim()
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{} is not a whole number.", variable(name))),
                None => Ok(value),
            }
        };
        let float = |name: &str, value: Option<f64>| -> Result<Option<f64>, String> {
            match env(&variable(name)).filter(|value| !value.trim().is_empty()) {
                Some(text) => text
                    .trim()
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{} is not a number.", variable(name))),
                None => Ok(value),
            }
        };
        let storage = match (
            get("database_url", file.database_url),
            get("state_dir", file.state_dir.map(|p| p.display().to_string())),
        ) {
            (Some(url), _) => Some(Storage::Postgres(url)),
            (None, Some(dir)) => Some(Storage::Files(dir.into())),
            (None, None) => None,
        };
        let metrics = match env(&variable("metrics")).filter(|v| !v.trim().is_empty()) {
            Some(text) => match text.trim() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => return Err(format!("{} is not true or false.", variable("metrics"))),
            },
            None => file.metrics.unwrap_or(true),
        };
        let half_life_days =
            float("success.half_life_days", file.success.half_life_days)?.unwrap_or(7.0);
        let min_samples = float("success.min_samples", file.success.min_samples)?.unwrap_or(5.0);
        if !(half_life_days.is_finite() && half_life_days > 0.0)
            || !(min_samples.is_finite() && min_samples > 0.0)
        {
            return Err("success.half_life_days and success.min_samples must be positive.".into());
        }
        let otlp = otlp_settings(&file.otlp, &env)?;
        let classifier = match get("classifier.kind", file.classifier.kind).as_deref() {
            None | Some("none") => Classifier::None,
            Some("typesafe") => Classifier::Typesafe {
                api_key: get("classifier.api_key", file.classifier.api_key)
                    .ok_or("The typesafe classifier needs classifier.api_key.")?,
                model: get("classifier.model", file.classifier.model)
                    .unwrap_or_else(|| "jev-latest".into()),
                base_url: get("classifier.base_url", file.classifier.base_url),
                max_options: None,
                limits: Default::default(),
            },
            Some("endpoint") => Classifier::Endpoint {
                base_url: get("classifier.base_url", file.classifier.base_url)
                    .ok_or("The endpoint classifier needs classifier.base_url.")?,
                api_key: get("classifier.api_key", file.classifier.api_key),
                model: get("classifier.model", file.classifier.model)
                    .unwrap_or_else(|| "jev-latest".into()),
                max_options: None,
                limits: Default::default(),
            },
            Some("pooled") => Classifier::Pooled {
                family: get("classifier.family", file.classifier.family)
                    .ok_or("The pooled classifier needs classifier.family.")?,
                model: get("classifier.model", file.classifier.model)
                    .ok_or("The pooled classifier needs classifier.model.")?,
            },
            Some(other) => {
                return Err(format!(
                    "classifier.kind {other:?} is not none, typesafe, endpoint or pooled."
                ))
            }
        };
        Ok(Self {
            listen: get("listen", file.listen).unwrap_or_else(|| "127.0.0.1:8790".into()),
            admin_token: get("admin_token", file.admin_token),
            run_token_signing_key: get("run_token_signing_key", file.run_token_signing_key),
            storage,
            credentials_dir: get(
                "credentials_dir",
                file.credentials_dir.map(|p| p.display().to_string()),
            )
            .map(PathBuf::from),
            catalog: get("catalog", file.catalog.map(|p| p.display().to_string()))
                .map(PathBuf::from),
            catalog_poll: Duration::from_secs(
                number("catalog_poll_s", file.catalog_poll_s)?
                    .unwrap_or(5)
                    .max(1),
            ),
            policy_mode: mode(
                &get("policy_mode", file.policy_mode).unwrap_or_else(|| "adaptive".into()),
            )?,
            drain_timeout: Duration::from_secs(
                number("drain_timeout_s", file.drain_timeout_s)?.unwrap_or(100),
            ),
            quota_probe_interval: Duration::from_secs(
                number("quota_probe_interval_s", file.quota_probe_interval_s)?.unwrap_or(900),
            ),
            metrics,
            success: SuccessWindow {
                half_life_ms: (half_life_days * 86_400_000.0) as i64,
                min_samples,
            },
            classifier,
            otlp,
        })
    }
}

/// The OTLP export settings: the gateway's own variable, then the standard
/// OpenTelemetry variable, then the file.
fn otlp_settings(
    file: &OtlpFile,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<Option<otlp::Settings>, String> {
    let read = |name: &str| {
        env(name)
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    };
    let pick = |setting: &str, standard: &str, value: &Option<String>| {
        read(&variable(setting))
            .or_else(|| read(standard))
            .or_else(|| {
                value
                    .as_ref()
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty())
            })
    };
    if read(OTEL_SDK_DISABLED).is_some_and(|value| value.eq_ignore_ascii_case("true")) {
        return Ok(None);
    }
    let endpoint = match (
        pick(
            "otlp.traces_endpoint",
            OTEL_TRACES_ENDPOINT,
            &file.traces_endpoint,
        ),
        pick("otlp.endpoint", OTEL_ENDPOINT, &file.endpoint),
    ) {
        (Some(traces), _) => traces,
        (None, Some(base)) => otlp::traces_url(&base),
        (None, None) => return Ok(None),
    };
    match url::Url::parse(&endpoint) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => {}
        _ => {
            return Err(format!(
                "The OTLP endpoint {endpoint:?} is not an http(s) URL."
            ))
        }
    }
    let headers = pick("otlp.headers", OTEL_HEADERS, &file.headers)
        .map(|text| otlp::parse_headers(&text))
        .transpose()?
        .unwrap_or_default();
    Ok(Some(otlp::Settings {
        endpoint,
        headers,
        service_name: pick("otlp.service_name", OTEL_SERVICE_NAME, &file.service_name)
            .unwrap_or_else(|| otlp::SCOPE.into()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name: &str| pairs.get(name).cloned()
    }

    fn with_file(text: &str, pairs: &[(&str, &str)]) -> Result<Settings, String> {
        let path = std::env::temp_dir().join(format!(
            "muniment-router-settings-{}.toml",
            uuid::Uuid::now_v7()
        ));
        std::fs::write(&path, text).unwrap();
        let settings = Settings::load(Some(&path), env(pairs));
        std::fs::remove_file(path).unwrap();
        settings
    }

    #[test]
    fn the_environment_overrides_the_file() {
        let settings = with_file(
            r#"
listen = "0.0.0.0:9000"
admin_token = "from-file"
database_url = "postgres://file"
policy_mode = "strong"
metrics = false

credentials_dir = "/var/lib/gateway/credentials"

[classifier]
kind = "pooled"
family = "openai"
model = "gpt-5.6-luna"

[success]
half_life_days = 2
"#,
            &[
                ("MUNIMENT_ROUTER_ADMIN_TOKEN", "from-env"),
                ("MUNIMENT_ROUTER_METRICS", "true"),
                ("MUNIMENT_ROUTER_SUCCESS_MIN_SAMPLES", "3"),
                ("MUNIMENT_ROUTER_CREDENTIALS_DIR", "/srv/credentials"),
            ],
        )
        .unwrap();
        assert_eq!(settings.listen, "0.0.0.0:9000");
        assert_eq!(settings.admin_token.as_deref(), Some("from-env"));
        assert_eq!(
            settings.storage,
            Some(Storage::Postgres("postgres://file".into()))
        );
        assert_eq!(settings.policy_mode, Mode::Strong);
        assert!(settings.metrics);
        assert_eq!(settings.success.half_life_ms, 2 * 86_400_000);
        assert_eq!(settings.success.min_samples, 3.0);
        assert_eq!(
            settings.credentials_dir.as_deref(),
            Some(Path::new("/srv/credentials"))
        );
        assert!(matches!(settings.classifier, Classifier::Pooled { .. }));
        assert_eq!(settings.otlp, None);
    }

    #[test]
    fn defaults_and_refusals() {
        let settings = Settings::load(None, |_| None).unwrap();
        assert_eq!(settings.listen, "127.0.0.1:8790");
        assert_eq!(settings.storage, None);
        assert_eq!(settings.credentials_dir, None);
        assert_eq!(settings.policy_mode, Mode::Adaptive);
        assert_eq!(settings.drain_timeout, Duration::from_secs(100));
        assert!(Settings::load(None, env(&[("MUNIMENT_ROUTER_POLICY_MODE", "fast")])).is_err());
        // An unknown section is refused, so a stale settings file fails loudly.
        assert!(with_file("[vault]\naddress = \"x\"\n", &[]).is_err());
        assert!(Settings::load(
            None,
            env(&[("MUNIMENT_ROUTER_CLASSIFIER_KIND", "typesafe")])
        )
        .is_err());
        assert!(Settings::load(None, env(&[("MUNIMENT_ROUTER_DRAIN_TIMEOUT_S", "soon")])).is_err());
        let files = Settings::load(None, env(&[("MUNIMENT_ROUTER_STATE_DIR", "/tmp/r")])).unwrap();
        assert_eq!(files.storage, Some(Storage::Files("/tmp/r".into())));
        assert_eq!(
            variable("otlp.service_name"),
            "MUNIMENT_ROUTER_OTLP_SERVICE_NAME"
        );
    }

    #[test]
    fn otlp_reads_the_gateway_then_the_standard_variables_then_the_file() {
        let otlp = |pairs: &[(&str, &str)]| Settings::load(None, env(pairs)).map(|s| s.otlp);
        assert_eq!(otlp(&[]).unwrap(), None);
        let standard = otlp(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318/"),
            ("OTEL_EXPORTER_OTLP_HEADERS", "authorization=Basic%20YTpi"),
            ("OTEL_SERVICE_NAME", "gateway-a"),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(standard.endpoint, "http://collector:4318/v1/traces");
        assert_eq!(
            standard.headers,
            vec![("authorization".to_owned(), "Basic YTpi".to_owned())]
        );
        assert_eq!(standard.service_name, "gateway-a");
        let own = otlp(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            (
                "MUNIMENT_ROUTER_OTLP_ENDPOINT",
                "https://traces.example/otel",
            ),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(own.endpoint, "https://traces.example/otel/v1/traces");
        assert_eq!(own.service_name, "muniment-router");
        assert!(own.headers.is_empty());
        let traces = otlp(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            (
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
                "http://other:4318/custom",
            ),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(traces.endpoint, "http://other:4318/custom");
        assert_eq!(
            otlp(&[
                ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
                ("OTEL_SDK_DISABLED", "true"),
            ])
            .unwrap(),
            None
        );
        assert!(otlp(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "collector:4318")]).is_err());
        assert!(otlp(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            ("OTEL_EXPORTER_OTLP_HEADERS", "broken"),
        ])
        .is_err());
        let file = with_file(
            "[otlp]\nendpoint = \"http://file:4318\"\nheaders = \"x-a=1\"\nservice_name = \"from-file\"\n",
            &[("OTEL_SERVICE_NAME", "from-env")],
        )
        .unwrap()
        .otlp
        .unwrap();
        assert_eq!(file.endpoint, "http://file:4318/v1/traces");
        assert_eq!(file.headers, vec![("x-a".to_owned(), "1".to_owned())]);
        assert_eq!(file.service_name, "from-env");
    }
}
