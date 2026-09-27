use std::{env, path::PathBuf};

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub origin: String,
    pub web_dir: PathBuf,
    pub auth: AuthConfig,
}

#[derive(Clone)]
pub enum AuthConfig {
    Local,
    Cloudflare {
        issuer: String,
        audience: String,
        email: String,
    },
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let port = env::var("PORT").unwrap_or_else(|_| "3000".into()).parse()?;
        let origin = env::var("APP_ORIGIN").unwrap_or_else(|_| "https://finanio.app".into());
        let url = reqwest::Url::parse(&origin)?;
        if url.origin().ascii_serialization() != origin {
            return Err(
                "APP_ORIGIN must be an origin without path, credentials, or trailing slash".into(),
            );
        }
        let auth = match env::var("AUTH_MODE").as_deref() {
            Ok("local") => {
                if url.scheme() != "http"
                    || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
                {
                    return Err("Local auth requires a loopback http APP_ORIGIN".into());
                }
                AuthConfig::Local
            }
            Ok("cloudflare") | Err(_) => {
                if url.scheme() != "https" {
                    return Err("Cloudflare mode requires HTTPS APP_ORIGIN".into());
                }
                let team = required("CF_ACCESS_TEAM")?;
                if !team
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                {
                    return Err("CF_ACCESS_TEAM must be the team slug, not a URL".into());
                }
                AuthConfig::Cloudflare {
                    issuer: format!("https://{team}.cloudflareaccess.com"),
                    audience: required("CF_ACCESS_AUD")?,
                    email: required("ALLOWED_EMAIL")?,
                }
            }
            Ok(_) => return Err("AUTH_MODE must be local or cloudflare".into()),
        };
        Ok(Self {
            port,
            origin,
            auth,
            web_dir: env::var_os("WEB_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("web/dist")),
        })
    }
}

fn required(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("Missing {name}; use AUTH_MODE=local and a loopback APP_ORIGIN for local development").into())
}
