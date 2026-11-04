use anyhow::{anyhow, Context, Result};
use scraper::{Html, Selector};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BootstrapToken {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: Option<String>,
    #[serde(rename = "expiresIn")]
    pub expires_in: u64,
    #[serde(rename = "tokenType")]
    pub token_type: String,
}

const SELECTOR: &str = "input#authToken";

pub fn parse_bootstrap_html(html: &str) -> Result<BootstrapToken> {
    let document = Html::parse_document(html);
    let selector = Selector::parse(SELECTOR).expect("static selector");
    let input = document.select(&selector).next().ok_or_else(|| {
        anyhow!("library page did not provide a bootstrap token (missing <input id=\"authToken\">)")
    })?;
    let value = input
        .value()
        .attr("value")
        .context("authToken input has no value attribute")?;
    parse_bootstrap_json(value)
}

pub fn parse_bootstrap_json(raw: &str) -> Result<BootstrapToken> {
    let trimmed = raw.trim_matches(['"', '\'']);
    let payload = if trimmed.contains("&quot;") {
        trimmed
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&#39;", "'")
    } else {
        trimmed.to_string()
    };
    serde_json::from_str(&payload).context("authToken input value is not valid bootstrap JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_HTML: &str = r#"<input type="hidden" id="authToken" name="token" value="{&quot;accessToken&quot;:&quot;TESTTOKEN&quot;,&quot;expiresIn&quot;:86400,&quot;tokenType&quot;:&quot;Bearer&quot;,&quot;refreshToken&quot;:&quot;TESTREFRESH&quot;}"/>"#;

    #[test]
    fn parses_fixture_html_with_escaped_json() {
        let boot = parse_bootstrap_html(FIXTURE_HTML).unwrap();
        assert_eq!(boot.access_token, "TESTTOKEN");
        assert_eq!(boot.refresh_token.as_deref(), Some("TESTREFRESH"));
        assert_eq!(boot.expires_in, 86400);
        assert_eq!(boot.token_type, "Bearer");
    }

    #[test]
    fn parses_raw_json_too() {
        let boot = parse_bootstrap_json(
            r#"{"accessToken":"T","expiresIn":86400,"tokenType":"Bearer","refreshToken":null}"#,
        )
        .unwrap();
        assert_eq!(boot.access_token, "T");
        assert_eq!(boot.expires_in, 86400);
        assert!(boot.refresh_token.is_none());
    }

    #[test]
    fn missing_input_is_specific_error() {
        let err = parse_bootstrap_html("<html><body>nope</body></html>").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("did not provide a bootstrap token"),
            "got: {msg}"
        );
    }
}
