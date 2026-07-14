pub mod ip_whitelist;
pub mod middleware;
pub mod scopes;
pub mod token_proxy;
pub mod token_static;

use std::collections::HashSet;
use std::net::SocketAddr;

use crate::config::Config;
use ip_whitelist::check_whitelist;
use scopes::Scope;
use token_proxy::ProxyValidator;
use token_static::check_static_token;

#[derive(Clone)]
pub struct AuthResolver {
    config: Option<crate::config::AuthConfig>,
    proxy: Option<ProxyValidator>,
}

impl AuthResolver {
    pub fn new(config: &Config) -> Self {
        let proxy = config
            .auth
            .as_ref()
            .and_then(|auth| auth.proxy.as_ref())
            .map(|proxy_config| {
                ProxyValidator::new(
                    proxy_config.clone(),
                    config.ha.as_ref().map(|ha| ha.url.clone()),
                )
            });

        Self {
            config: config.auth.clone(),
            proxy,
        }
    }

    pub async fn resolve(&self, remote_addr: SocketAddr, bearer: Option<&str>) -> GrantedScopes {
        let Some(ref auth_config) = self.config else {
            return GrantedScopes::all();
        };

        // 1. IP whitelist (first match wins)
        if let Some(scopes) = check_whitelist(auth_config, remote_addr.ip()) {
            return GrantedScopes(scopes);
        }

        // 2. Static token
        if let Some(bearer) = bearer {
            if let Some(scopes) = check_static_token(auth_config, bearer) {
                return GrantedScopes(scopes);
            }

            // 3. Proxy validation
            if let Some(ref proxy) = self.proxy
                && let Some(scopes) = proxy.validate(bearer).await
            {
                return GrantedScopes(scopes);
            }
        }

        GrantedScopes::none()
    }
}

#[derive(Debug, Clone)]
pub struct GrantedScopes(HashSet<Scope>);

impl GrantedScopes {
    pub fn all() -> Self {
        Self(scopes::parse_scopes(&["*".to_string()]))
    }

    pub fn none() -> Self {
        Self(HashSet::new())
    }

    pub fn has(&self, scope: &Scope) -> bool {
        self.0.contains(scope)
    }

    pub fn check_tool(&self, tool_name: &str) -> Result<(), String> {
        if let Some(required) = scopes::tool_required_scope(tool_name) {
            if self.has(&required) {
                Ok(())
            } else {
                Err(format!(
                    "missing scope '{}' required for tool '{}'",
                    required.as_str(),
                    tool_name
                ))
            }
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn granted_all_allows_everything() {
        let scopes = GrantedScopes::all();
        assert!(scopes.check_tool("ha_entity.get_state").is_ok());
        assert!(scopes.check_tool("ha_light.turn_on").is_ok());
        assert!(scopes.check_tool("z2m_device.set").is_ok());
    }

    #[test]
    fn granted_none_blocks_everything() {
        let scopes = GrantedScopes::none();
        assert!(scopes.check_tool("ha_entity.get_state").is_err());
        assert!(scopes.check_tool("ha_light.turn_on").is_err());
    }

    #[test]
    fn read_only_blocks_control() {
        let scopes = GrantedScopes(scopes::parse_scopes(&[
            "ha:read".to_string(),
            "z2m:read".to_string(),
        ]));
        assert!(scopes.check_tool("ha_entity.get_state").is_ok());
        assert!(scopes.check_tool("ha_entity.list").is_ok());
        assert!(scopes.check_tool("z2m_device.list").is_ok());
        assert!(scopes.check_tool("ha_light.turn_on").is_err());
        assert!(scopes.check_tool("z2m_device.set").is_err());
    }

    #[test]
    fn unknown_tool_allowed() {
        let scopes = GrantedScopes::none();
        assert!(scopes.check_tool("unknown_tool").is_ok());
    }

    #[test]
    fn error_message_names_scope() {
        let scopes = GrantedScopes::none();
        let err = scopes.check_tool("ha_light.turn_on").unwrap_err();
        assert!(err.contains("ha:control"));
        assert!(err.contains("ha_light.turn_on"));
    }
}
