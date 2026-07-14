use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    HaRead,
    HaControl,
    Z2mRead,
    Z2mControl,
}

impl Scope {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ha:read" => Some(Self::HaRead),
            "ha:control" => Some(Self::HaControl),
            "z2m:read" => Some(Self::Z2mRead),
            "z2m:control" => Some(Self::Z2mControl),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HaRead => "ha:read",
            Self::HaControl => "ha:control",
            Self::Z2mRead => "z2m:read",
            Self::Z2mControl => "z2m:control",
        }
    }
}

pub fn parse_scopes(strings: &[String]) -> HashSet<Scope> {
    let mut scopes = HashSet::new();
    for s in strings {
        if s == "*" {
            scopes.insert(Scope::HaRead);
            scopes.insert(Scope::HaControl);
            scopes.insert(Scope::Z2mRead);
            scopes.insert(Scope::Z2mControl);
            return scopes;
        }
        if let Some(scope) = Scope::parse(s) {
            scopes.insert(scope);
        }
    }
    scopes
}

pub fn tool_required_scope(tool_name: &str) -> Option<Scope> {
    match tool_name {
        // HA read
        "ha_entity.get_state"
        | "ha_entity.list"
        | "ha_entity.search"
        | "ha_area.get_status"
        | "ha_area.list"
        | "ha_todo.get_items" => Some(Scope::HaRead),

        // HA control
        "ha_light.turn_on"
        | "ha_light.turn_off"
        | "ha_light.set_brightness"
        | "ha_light.turn_on_in_area"
        | "ha_light.turn_off_in_area"
        | "ha_todo.add_item"
        | "ha_todo.update_item"
        | "ha_todo.remove_item"
        | "ha_service.call" => Some(Scope::HaControl),

        // Z2M read
        "z2m_device.list" | "z2m_device.get_state" | "z2m_group.list" | "z2m_bridge.info" => {
            Some(Scope::Z2mRead)
        }

        // Z2M control
        "z2m_device.set"
        | "z2m_device.rename"
        | "z2m_group.add"
        | "z2m_bridge.permit_join"
        | "z2m_bridge.networkmap" => Some(Scope::Z2mControl),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wildcard_returns_all() {
        let scopes = parse_scopes(&["*".to_string()]);
        assert!(scopes.contains(&Scope::HaRead));
        assert!(scopes.contains(&Scope::HaControl));
        assert!(scopes.contains(&Scope::Z2mRead));
        assert!(scopes.contains(&Scope::Z2mControl));
    }

    #[test]
    fn parse_specific_scopes() {
        let scopes = parse_scopes(&["ha:read".to_string(), "z2m:read".to_string()]);
        assert!(scopes.contains(&Scope::HaRead));
        assert!(scopes.contains(&Scope::Z2mRead));
        assert!(!scopes.contains(&Scope::HaControl));
        assert!(!scopes.contains(&Scope::Z2mControl));
    }

    #[test]
    fn unknown_scope_ignored() {
        let scopes = parse_scopes(&["ha:read".to_string(), "bogus".to_string()]);
        assert_eq!(scopes.len(), 1);
    }

    #[test]
    fn tool_scope_mapping() {
        assert_eq!(
            tool_required_scope("ha_entity.get_state"),
            Some(Scope::HaRead)
        );
        assert_eq!(
            tool_required_scope("ha_light.turn_on"),
            Some(Scope::HaControl)
        );
        assert_eq!(tool_required_scope("z2m_device.list"), Some(Scope::Z2mRead));
        assert_eq!(
            tool_required_scope("z2m_device.set"),
            Some(Scope::Z2mControl)
        );
        assert_eq!(tool_required_scope("unknown_tool"), None);
    }
}
