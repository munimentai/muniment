// Bundled desktop guides share the website's public product evidence.
pub const URL: &str = "https://muniment.ai/docs/";
const GUIDE: &str = include_str!("../../docs/public-evidence/desktop-guide.json");
fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
pub fn html() -> String {
    let data: serde_json::Value = serde_json::from_str(GUIDE).expect("valid bundled desktop docs");
    let mut nav = String::new();
    let mut body = String::new();
    for guide in data["guides"].as_array().expect("desktop guides") {
        let slug = escape(guide["slug"].as_str().unwrap());
        let title = escape(guide["title"].as_str().unwrap());
        nav.push_str(&format!("<a href=\"#{slug}\">{title}</a>"));
        body.push_str(&format!("<article id=\"{slug}\"><h2>{title}</h2><p class=\"summary\">{}</p>", escape(guide["description"].as_str().unwrap())));
        for section in guide["sections"].as_array().unwrap() {
            body.push_str(&format!("<section id=\"{slug}-{}\"><h3>{}</h3><p>{}</p></section>", escape(section["id"].as_str().unwrap()), escape(section["title"].as_str().unwrap()), escape(section["body"].as_str().unwrap())));
        }
        body.push_str("</article>");
    }
    format!(r##"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Muniment desktop docs</title><style>
:root {{ color-scheme: light dark; --paper:#f6f7f6; --ink:#1a1d1c; --muted:#5c6461; --border:#d5dad7; --signal:#2f7e6d; }}
@media(prefers-color-scheme:dark) {{ :root {{ --paper:#0e110f; --ink:#e8ebe9; --muted:#9aa39e; --border:#29302b; --signal:#58b39f; }} }}
* {{ box-sizing:border-box; }} html {{ scroll-padding-top:24px; }} body {{ margin:0; background:var(--paper); color:var(--ink); font:15px/1.65 system-ui,sans-serif; }}
header {{ padding:32px; border-bottom:1px solid var(--border); }} header strong {{ font-size:22px; }} .layout {{ display:grid; grid-template-columns:220px minmax(0,1fr); max-width:1200px; margin:auto; }}
nav {{ padding:24px; position:sticky; top:0; align-self:start; }} a {{ color:inherit; text-underline-offset:4px; }} nav a {{ display:block; padding:8px; text-decoration:none; border-radius:6px; }} nav a:hover {{ background:var(--border); }} a:focus-visible {{ outline:2px solid var(--signal); outline-offset:3px; }}
main {{ padding:32px; min-width:0; }} article {{ padding:0 0 32px; margin-bottom:32px; border-bottom:1px solid var(--border); }} h1 {{ font-size:32px; line-height:1.2; }} h2 {{ font-size:24px; }} h3 {{ font-size:17px; margin:28px 0 8px; }} p {{ max-width:72ch; margin-top:8px; }} .summary,header p {{ color:var(--muted); }} .skip {{ position:absolute; top:-100px; }} .skip:focus {{ top:4px; background:var(--paper); padding:8px; }}
@media(max-width:720px) {{ .layout {{ display:block; }} nav {{ position:static; display:flex; flex-wrap:wrap; gap:4px; border-bottom:1px solid var(--border); }} main,header,nav {{ padding:20px; }} nav a {{ font-size:13px; }} }}
</style><a class="skip" href="#main">Skip to docs</a><header><strong>muniment</strong><p>Desktop docs · Available offline</p></header><div class="layout"><nav aria-label="Desktop guides">{nav}<a href="https://muniment.ai/docs/" rel="noreferrer">Online docs</a></nav><main id="main" tabindex="-1"><h1>Your desktop workspace</h1><p>Connect an account, choose a model, and make the workspace your own.</p>{body}</main></div></html>"##)
}
#[cfg(test)]
mod tests {
    #[test]
    fn bundled_guides_are_complete_and_offline() {
        let page = super::html();
        assert!(page.contains("MCP servers, skills, and plugins"));
        assert!(page.contains("id=\"troubleshooting\""));
        assert_eq!(page.matches("<article ").count(), 8);
        assert!(!page.contains("<script"));
        assert!(!page.contains("src=\"http"));
    }
}
