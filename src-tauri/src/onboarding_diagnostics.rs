use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FirstRunFailure {
    HomeUnavailable,
    HomeConfirm,
    Startup,
    SessionStatus,
    LocalMode,
    Runtime,
}

impl FirstRunFailure {
    fn message(&self) -> &'static str {
        match self {
            Self::HomeUnavailable => "Home is unavailable. Quit and reopen Muniment.",
            Self::HomeConfirm => {
                "Muniment could not create Home. Check folder access and try Send again."
            }
            Self::Startup => "Muniment could not finish startup. Choose a model again.",
            Self::SessionStatus => {
                "Muniment could not read session status. Choose a model again."
            }
            Self::LocalMode => "Muniment could not enter local mode. Choose a model again.",
            Self::Runtime => "The runtime is not connected yet. Choose a model again.",
        }
    }
}

#[tauri::command]
pub fn onboarding_model_settings_error(cause: FirstRunFailure) {
    eprintln!("{}", cause.message());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fixed_causes_reach_stderr() {
        for input in [
            r#""draft text""#,
            r#""runtime\nforged log""#,
            "null",
            "{}",
            "1",
        ] {
            assert!(serde_json::from_str::<FirstRunFailure>(input).is_err());
        }
        let cause: FirstRunFailure = serde_json::from_str(r#""runtime""#).unwrap();
        assert_eq!(
            cause.message(),
            "The runtime is not connected yet. Choose a model again."
        );
    }
}
