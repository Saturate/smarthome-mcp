use std::collections::HashSet;
use std::net::IpAddr;

use crate::auth::scopes::{Scope, parse_scopes};
use crate::config::AuthConfig;

pub fn check_whitelist(auth: &AuthConfig, ip: IpAddr) -> Option<HashSet<Scope>> {
    for entry in &auth.whitelist {
        if entry.cidrs.iter().any(|cidr| cidr.contains(&ip)) {
            return Some(parse_scopes(&entry.scopes));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, WhitelistEntry};

    fn test_auth() -> AuthConfig {
        AuthConfig {
            whitelist: vec![
                WhitelistEntry {
                    cidrs: vec!["192.168.1.0/24".parse().unwrap()],
                    scopes: vec!["ha:read".to_string()],
                },
                WhitelistEntry {
                    cidrs: vec!["10.0.0.0/8".parse().unwrap()],
                    scopes: vec!["*".to_string()],
                },
            ],
            tokens: vec![],
            proxy: None,
        }
    }

    #[test]
    fn matches_first_rule() {
        let auth = test_auth();
        let scopes = check_whitelist(&auth, "192.168.1.50".parse().unwrap()).unwrap();
        assert!(scopes.contains(&Scope::HaRead));
        assert!(!scopes.contains(&Scope::HaControl));
    }

    #[test]
    fn matches_second_rule() {
        let auth = test_auth();
        let scopes = check_whitelist(&auth, "10.5.3.1".parse().unwrap()).unwrap();
        assert!(scopes.contains(&Scope::HaRead));
        assert!(scopes.contains(&Scope::HaControl));
        assert!(scopes.contains(&Scope::Z2mRead));
        assert!(scopes.contains(&Scope::Z2mControl));
    }

    #[test]
    fn no_match() {
        let auth = test_auth();
        assert!(check_whitelist(&auth, "172.16.0.1".parse().unwrap()).is_none());
    }
}
