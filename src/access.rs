use crate::config::{AuthConfig, Config};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub enum Access {
    Local,
    Cloudflare(Arc<CloudflareAccess>),
}

pub struct CloudflareAccess {
    issuer: String,
    audience: String,
    email: String,
    client: reqwest::Client,
    keys: Mutex<(Instant, JwkSet)>,
}

#[derive(Clone, Deserialize)]
pub struct Identity {
    pub email: String,
}

impl Access {
    pub async fn from_config(config: &Config) -> Result<Self, Box<dyn std::error::Error>> {
        match &config.auth {
            AuthConfig::Local => Ok(Self::Local),
            AuthConfig::Cloudflare {
                issuer,
                audience,
                email,
            } => {
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(5))
                    .build()?;
                let keys = fetch_keys(&client, issuer).await?;
                Ok(Self::Cloudflare(Arc::new(CloudflareAccess {
                    issuer: issuer.clone(),
                    audience: audience.clone(),
                    email: email.clone(),
                    client,
                    keys: Mutex::new((Instant::now(), keys)),
                })))
            }
        }
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloudflare(_) => "cloudflare",
        }
    }

    pub async fn authenticate(&self, token: Option<&str>) -> Option<Identity> {
        match self {
            Self::Local => Some(Identity {
                email: "本地开发".into(),
            }),
            Self::Cloudflare(access) => {
                let token = token?;
                let header = decode_header(token).ok()?;
                if header.alg != Algorithm::RS256 || header.kid.is_none() {
                    return None;
                }
                let mut keys = access.keys.lock().await;
                // Bounded refresh frequency prevents unknown-kid request floods.
                if keys.0.elapsed() > Duration::from_secs(300) {
                    match fetch_keys(&access.client, &access.issuer).await {
                        Ok(fresh) => *keys = (Instant::now(), fresh),
                        Err(_) => {
                            // Fail closed and back off, rather than fetching once per request.
                            *keys = (Instant::now(), JwkSet { keys: Vec::new() });
                            return None;
                        }
                    }
                }
                verify_token(
                    token,
                    &keys.1,
                    &access.issuer,
                    &access.audience,
                    &access.email,
                )
            }
        }
    }
}

async fn fetch_keys(client: &reqwest::Client, issuer: &str) -> Result<JwkSet, reqwest::Error> {
    client
        .get(format!("{issuer}/cdn-cgi/access/certs"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

pub fn verify_token(
    token: &str,
    keys: &JwkSet,
    issuer: &str,
    audience: &str,
    email: &str,
) -> Option<Identity> {
    let header = decode_header(token).ok()?;
    if header.alg != Algorithm::RS256 {
        return None;
    }
    let key = DecodingKey::from_jwk(keys.find(header.kid.as_deref()?)?).ok()?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[audience]);
    validation.set_issuer(&[issuer]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.validate_nbf = true;
    validation.leeway = 0;
    let identity = decode::<Identity>(token, &key, &validation).ok()?.claims;
    identity
        .email
        .eq_ignore_ascii_case(email)
        .then_some(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn cloudflare_guard_protects_pages_assets_and_api_even_with_spoofed_email() {
        let config = Config {
            port: 3000,
            origin: "https://finanio.app".into(),
            web_dir: "web/dist".into(),
            auth: AuthConfig::Cloudflare {
                issuer: "https://test.cloudflareaccess.com".into(),
                audience: "test".into(),
                email: "tester@example.com".into(),
            },
        };
        let access = Access::Cloudflare(Arc::new(CloudflareAccess {
            issuer: "https://test.cloudflareaccess.com".into(),
            audience: "test".into(),
            email: "tester@example.com".into(),
            client: reqwest::Client::new(),
            keys: Mutex::new((Instant::now(), JwkSet { keys: vec![] })),
        }));
        let app = crate::http::router(config, access, None);
        for path in ["/", "/favicon.svg", "/api/session", "/api/messages"] {
            for token in [None, Some("forged-token")] {
                let mut request = Request::builder()
                    .uri(path)
                    .method(if path == "/api/messages" {
                        "POST"
                    } else {
                        "GET"
                    })
                    .header("origin", "https://finanio.app")
                    .header("cf-access-authenticated-user-email", "tester@example.com");
                if let Some(token) = token {
                    request = request.header("cf-access-jwt-assertion", token);
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
            }
        }
    }
}
