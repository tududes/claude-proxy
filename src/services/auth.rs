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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    // ============================================================================
    // normalize_auth_value_to_key tests
    // ============================================================================

    #[test]
    fn test_normalize_auth_with_bearer_prefix() {
        let result = normalize_auth_value_to_key("Bearer sk-1234567890");
        assert_eq!(result, "sk-1234567890");
    }

    #[test]
    fn test_normalize_auth_with_bearer_and_extra_spaces() {
        let result = normalize_auth_value_to_key("Bearer   sk-1234567890  ");
        assert_eq!(result, "sk-1234567890");
    }

    #[test]
    fn test_normalize_auth_without_bearer() {
        let result = normalize_auth_value_to_key("sk-1234567890");
        assert_eq!(result, "sk-1234567890");
    }

    #[test]
    fn test_normalize_auth_with_leading_spaces() {
        let result = normalize_auth_value_to_key("   sk-1234567890");
        assert_eq!(result, "sk-1234567890");
    }

    #[test]
    fn test_normalize_auth_empty() {
        let result = normalize_auth_value_to_key("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_normalize_auth_only_bearer_with_space() {
        // Input: "Bearer " -> trim -> "Bearer" -> strip_prefix fails -> returns "Bearer"
        let result = normalize_auth_value_to_key("Bearer ");
        assert_eq!(result, "Bearer");
    }

    #[test]
    fn test_normalize_auth_bearer_only_word() {
        // If input is just "Bearer" with no space, nothing to strip
        let result = normalize_auth_value_to_key("Bearer");
        assert_eq!(result, "Bearer");
    }

    #[test]
    fn test_normalize_auth_lowercase_bearer() {
        // Note: strip_prefix is case-sensitive, so this won't strip
        let result = normalize_auth_value_to_key("bearer sk-1234567890");
        assert_eq!(result, "bearer sk-1234567890");
    }

    // ============================================================================
    // mask_token tests
    // ============================================================================

    #[test]
    fn test_mask_token_long() {
        let result = mask_token("sk-ant-1234567890abcdefghijklmnop");
        assert_eq!(result, "sk-ant...mnop");
    }

    #[test]
    fn test_mask_token_exactly_13_chars() {
        // Function masks when > 12 chars (not >= 12)
        let result = mask_token("1234567890123");
        assert_eq!(result, "123456...0123");
    }

    #[test]
    fn test_mask_token_short() {
        let result = mask_token("short");
        assert_eq!(result, "***");
    }

    #[test]
    fn test_mask_token_very_short() {
        let result = mask_token("ab");
        assert_eq!(result, "***");
    }

    #[test]
    fn test_mask_token_empty() {
        let result = mask_token("");
        assert_eq!(result, "<empty>");
    }

    #[test]
    fn test_mask_token_anthropic_format() {
        let result = mask_token("sk-ant-api03-1234567890abcdefghijklmnopqrstuvwxyz");
        assert!(result.starts_with("sk-ant"));
        assert!(result.ends_with("wxyz"));
        assert!(result.contains("..."));
    }

    #[test]
    fn test_mask_token_openai_format() {
        let result = mask_token("sk-proj-1234567890abcdefghijklmnop");
        assert!(result.starts_with("sk-pro"));
        assert!(result.ends_with("mnop"));
    }

    // ============================================================================
    // extract_client_key tests
    // ============================================================================

    #[test]
    fn test_extract_client_key_from_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer sk-test-123"));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-test-123".to_string()));
    }

    #[test]
    fn test_extract_client_key_from_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("sk-test-456"));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-test-456".to_string()));
    }

    #[test]
    fn test_extract_client_key_authorization_takes_precedence() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer sk-auth-key"));
        headers.insert("x-api-key", HeaderValue::from_static("sk-x-key"));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-auth-key".to_string()));
    }

    #[test]
    fn test_extract_client_key_no_headers() {
        let headers = HeaderMap::new();
        
        let result = extract_client_key(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_client_key_empty_authorization() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static(""));
        headers.insert("x-api-key", HeaderValue::from_static("sk-fallback"));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-fallback".to_string()));
    }

    #[test]
    fn test_extract_client_key_whitespace_only() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("   "));
        headers.insert("x-api-key", HeaderValue::from_static("sk-fallback"));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-fallback".to_string()));
    }

    #[test]
    fn test_extract_client_key_both_empty() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static(""));
        headers.insert("x-api-key", HeaderValue::from_static(""));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_client_key_strips_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer  sk-with-spaces  "));
        
        let result = extract_client_key(&headers);
        assert_eq!(result, Some("sk-with-spaces".to_string()));
    }
}