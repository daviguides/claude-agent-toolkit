//! Permission types.

use serde::{Deserialize, Serialize};

/// Permission-prompt behavior for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    /// Prompt normally (CLI default).
    #[serde(rename = "default")]
    Default,
    /// Auto-accept file edits.
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    /// Planning mode: no tool execution.
    #[serde(rename = "plan")]
    Plan,
    /// Skip all permission checks.
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    /// Don't prompt; deny anything not pre-approved.
    #[serde(rename = "dontAsk")]
    DontAsk,
    /// Automatic mode (CLI-defined heuristics).
    #[serde(rename = "auto")]
    Auto,
}

impl PermissionMode {
    /// Wire string used by the CLI flag and control protocol.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
            Self::BypassPermissions => "bypassPermissions",
            Self::DontAsk => "dontAsk",
            Self::Auto => "auto",
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(PermissionMode::Default, "default")]
    #[case(PermissionMode::AcceptEdits, "acceptEdits")]
    #[case(PermissionMode::Plan, "plan")]
    #[case(PermissionMode::BypassPermissions, "bypassPermissions")]
    #[case(PermissionMode::DontAsk, "dontAsk")]
    #[case(PermissionMode::Auto, "auto")]
    fn permission_mode_as_str(#[case] mode: PermissionMode, #[case] expected: &str) {
        assert_eq!(mode.as_str(), expected);
    }

    #[rstest]
    #[case(PermissionMode::Default, "\"default\"")]
    #[case(PermissionMode::AcceptEdits, "\"acceptEdits\"")]
    #[case(PermissionMode::Plan, "\"plan\"")]
    #[case(PermissionMode::BypassPermissions, "\"bypassPermissions\"")]
    #[case(PermissionMode::DontAsk, "\"dontAsk\"")]
    #[case(PermissionMode::Auto, "\"auto\"")]
    fn permission_mode_serde_roundtrip(#[case] mode: PermissionMode, #[case] wire: &str) {
        let json = serde_json::to_string(&mode).expect("serializes");
        assert_eq!(json, wire);
        let parsed: PermissionMode = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(parsed, mode);
    }
}
