use axum::http::{HeaderMap, HeaderName, header::AUTHORIZATION};

/// Normalize an Authorization header value into a bare API key
pub fn normalize_auth_value_to_key(value: &str) -> String {
    value
        .trim()
        .strip_prefix("Bearer ")
        .map(str::trim)
        .unwrap_or(value.trim())
        .to_string()
}

/// Mask sensitive tokens for logs while keeping useful context
pub fn mask_token(token: &str) -> String {
    if token.len() > 12 {
        format!("{}...{}", &token[..6], &token[token.len() - 4..])
    } else if !token.is_empty() {
        "***".to_string()
    } else {
        "<empty>".into()
    }
}

/// Extract client key from headers
pub fn extract_client_key(headers: &HeaderMap) -> Option<String> {
    let x_api_key_header = HeaderName::from_static("x-api-key");
    let raw_x_api_key = headers
        .get(&x_api_key_header)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let raw_authorization = headers
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    raw_authorization
        .as_ref()
        .map(|auth| normalize_auth_value_to_key(auth))
        .or_else(|| raw_x_api_key.clone())
}