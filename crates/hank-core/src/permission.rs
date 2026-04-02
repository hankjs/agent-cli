/// Permission decisions returned by tool permission checks.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionDecision {
    Allow,
    Deny(String),
    Ask,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResponse {
    Allow,
    Deny,
    AlwaysAllow(String), // pattern
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Bypass,
}

#[derive(Debug, Clone)]
pub struct PermissionRule {
    pub tool_pattern: String,
    pub behavior: PermissionDecision,
}

impl PermissionRule {
    pub fn matches(&self, tool_name: &str) -> bool {
        if self.tool_pattern == "*" {
            return true;
        }
        if self.tool_pattern.ends_with('*') {
            let prefix = &self.tool_pattern[..self.tool_pattern.len() - 1];
            return tool_name.starts_with(prefix);
        }
        self.tool_pattern == tool_name
    }
}

pub struct PermissionChecker {
    mode: PermissionMode,
    rules: Vec<PermissionRule>,
}

impl PermissionChecker {
    pub fn new(mode: PermissionMode, rules: Vec<PermissionRule>) -> Self {
        Self { mode, rules }
    }

    pub fn check(&self, tool_name: &str, tool_decision: PermissionDecision) -> PermissionDecision {
        // Deny rules first
        for rule in &self.rules {
            if rule.matches(tool_name) {
                if let PermissionDecision::Deny(ref reason) = rule.behavior {
                    return PermissionDecision::Deny(reason.clone());
                }
            }
        }
        // Tool's own check
        if let PermissionDecision::Deny(reason) = &tool_decision {
            return PermissionDecision::Deny(reason.clone());
        }
        // Allow rules
        for rule in &self.rules {
            if rule.matches(tool_name) {
                if rule.behavior == PermissionDecision::Allow {
                    return PermissionDecision::Allow;
                }
            }
        }
        // Mode-based
        match self.mode {
            PermissionMode::Bypass => PermissionDecision::Allow,
            _ => tool_decision,
        }
    }

    pub fn add_session_rule(&mut self, rule: PermissionRule) {
        self.rules.push(rule);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_rules_take_precedence_over_allow_rules() {
        let checker = PermissionChecker::new(
            PermissionMode::Bypass,
            vec![
                PermissionRule {
                    tool_pattern: "bash".into(),
                    behavior: PermissionDecision::Allow,
                },
                PermissionRule {
                    tool_pattern: "bash".into(),
                    behavior: PermissionDecision::Deny("blocked".into()),
                },
            ],
        );

        assert_eq!(
            checker.check("bash", PermissionDecision::Ask),
            PermissionDecision::Deny("blocked".into())
        );
    }

    #[test]
    fn wildcard_rules_match_prefixes() {
        let rule = PermissionRule {
            tool_pattern: "file_*".into(),
            behavior: PermissionDecision::Allow,
        };

        assert!(rule.matches("file_write"));
        assert!(!rule.matches("bash"));
    }

    #[test]
    fn modes_fall_back_to_expected_decisions() {
        let default_checker = PermissionChecker::new(PermissionMode::Default, vec![]);
        assert_eq!(
            default_checker.check("write", PermissionDecision::Ask),
            PermissionDecision::Ask
        );

        let bypass_checker = PermissionChecker::new(PermissionMode::Bypass, vec![]);
        assert_eq!(
            bypass_checker.check("write", PermissionDecision::Ask),
            PermissionDecision::Allow
        );
    }
}
