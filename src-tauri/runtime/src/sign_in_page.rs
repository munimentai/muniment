//! The page a provider sign-in's browser lands on after the provider
//! redirects: Muniment's mark and voice, on the app's paper.

const MARK_SVG: &str = include_str!("../../icons/muniment-mark-auth.svg");
/// The mark a finished sign-in plays once: the elephant rears and settles.
const SIGNED_IN_SVG: &str = include_str!("../../icons/muniment-mark-signed-in.svg");

/// `Some` with the provider's name after a redirect that carried a code,
/// `None` without one.
pub fn sign_in_page(connected: Option<&str>) -> String {
    let (mark, heading, body) = match connected {
        Some(provider) => (
            SIGNED_IN_SVG,
            "Signed in".to_owned(),
            format!(
                "{provider} is connected. Muniment is ready in the app, and this window can close."
            ),
        ),
        None => (
            MARK_SVG,
            "Sign-in did not finish".to_owned(),
            "Muniment did not get a code from this page. Return to the app and try again."
                .to_owned(),
        ),
    };
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Muniment</title>
<style>
:root {{ color-scheme: light dark; --paper: #F6F7F6; --ink: #1A1D1C; --muted: #5C6461; }}
@media (prefers-color-scheme: dark) {{ :root {{ --paper: #000000; --ink: #ECEFED; --muted: #9AA29E; }} }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 24px; background: var(--paper); color: var(--ink); font-family: system-ui, -apple-system, "Segoe UI", sans-serif; -webkit-font-smoothing: antialiased; text-align: center; }}
main {{ display: grid; justify-items: center; gap: 12px; max-width: 480px; }}
main svg {{ width: 56px; height: 56px; margin-bottom: 12px; }}
h1 {{ margin: 0; font-size: 28px; font-weight: 600; line-height: 1.3; letter-spacing: -0.01em; }}
p {{ margin: 0; color: var(--muted); font-size: 15px; line-height: 1.55; }}
</style>
</head>
<body>
<main>
{mark}
<h1>{heading}</h1>
<p>{body}</p>
</main>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_in_page_is_muniment() {
        let page = sign_in_page(Some("OpenAI"));
        assert!(page.contains("<title>Muniment</title>"));
        assert!(page.contains("aria-label=\"muniment\""));
        // The page draws the pocket-fold mark, its body and its ear.
        assert!(page.contains("<path class=\"body\" d=\"M160 0 H274"));
        assert!(page.contains("<path class=\"ear\" d=\"M294 0 L510 190"));
        // A finished sign-in plays the mark once, and a failed one shows it still.
        assert!(page.contains("<animate"));
        assert!(page.contains("prefers-reduced-motion"));
        assert!(page.contains("OpenAI is connected."));
        assert!(!page.contains("Authentication successful"));
        let missing = sign_in_page(None);
        assert!(missing.contains("Sign-in did not finish"));
        assert!(!missing.contains("<animate"));
    }
}
