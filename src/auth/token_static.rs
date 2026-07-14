use std::collections::HashSet;

use crate::auth::scopes::{Scope, parse_scopes};
use crate::config::AuthConfig;

pub fn check_static_token(auth: &AuthConfig, bearer: &str) -> Option<HashSet<Scope>> {
    for entry in &auth.tokens {
        if entry.token == bearer {
            return Some(parse_scopes(&entry.scopes));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, TokenEntry};

    fn test_auth() -> AuthConfig {
        AuthConfig {
            whitelist: vec![],
            tokens: vec![
                TokenEntry {
                    token: "full-access".to_string(),
                    scopes: vec!["*".to_string()],
                },
                TokenEntry {
                    token: "read-only".to_string(),
                    scopes: vec!["ha:read".to_string(), "z2m:read".to_string()],
                },
            ],
            proxy: None,
        }
    }

    #[test]
    fn matches_full_access() {
        let auth = test_auth();
        let scopes = check_static_token(&auth, "full-access").unwrap();
        assert!(scopes.contains(&Scope::HaControl));
        assert!(scopes.contains(&Scope::Z2mControl));
    }

    #[test]
    fn matches_read_only() {
        let auth = test_auth();
        let scopes = check_static_token(&auth, "read-only").unwrap();
        assert!(scopes.contains(&Scope::HaRead));
        assert!(scopes.contains(&Scope::Z2mRead));
        assert!(!scopes.contains(&Scope::HaControl));
    }

    #[test]
    fn no_match() {
        let auth = test_auth();
        assert!(check_static_token(&auth, "wrong-token").is_none());
    }
}
