use calcal::access::verify_token;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, jwk::JwkSet};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

const ISSUER: &str = "https://test-team.cloudflareaccess.com";
const EMAIL: &str = "tester@example.com";

fn claims() -> Value {
    json!({"iss": ISSUER, "aud": ["test-audience"], "sub": "test-user", "email": EMAIL,
        "exp": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 300})
}

fn sign(claims: &Value, kid: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.into());
    let key = EncodingKey::from_rsa_pem(include_bytes!("fixtures/test-only-private.pem")).unwrap();
    encode(&header, claims, &key).unwrap()
}

fn verify(token: &str) -> bool {
    let keys: JwkSet = serde_json::from_str(include_str!("fixtures/jwks.json")).unwrap();
    verify_token(token, &keys, ISSUER, "test-audience", EMAIL).is_some()
}

#[test]
fn accepts_only_signed_tokens_for_the_expected_user_and_application() {
    assert!(verify(&sign(&claims(), "test-key")));
    for (field, value) in [
        ("email", json!("other@example.com")),
        ("iss", json!("https://evil.example")),
        ("aud", json!(["another-app"])),
        ("exp", json!(1)),
        ("nbf", json!(9999999999u64)),
    ] {
        let mut invalid = claims();
        invalid[field] = value;
        assert!(!verify(&sign(&invalid, "test-key")), "must reject {field}");
    }
    for field in ["exp", "iss", "aud", "sub", "email"] {
        let mut invalid = claims();
        invalid.as_object_mut().unwrap().remove(field);
        assert!(!verify(&sign(&invalid, "test-key")), "must require {field}");
    }
}

#[test]
fn rejects_unknown_keys_forged_tokens_and_wrong_algorithms() {
    assert!(!verify("not-a-token"));
    assert!(!verify(&sign(&claims(), "unknown-key")));
    let token = sign(&claims(), "test-key");
    let mut parts: Vec<_> = token.split('.').collect();
    parts[2] = "Zm9yZ2Vk";
    assert!(!verify(&parts.join(".")));
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims(),
        &EncodingKey::from_secret(b"test-only"),
    )
    .unwrap();
    assert!(!verify(&token));
}
