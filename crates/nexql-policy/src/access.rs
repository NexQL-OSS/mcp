// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 NexQL-OSS Team

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::PolicyError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessMode {
    #[default]
    Read,
    Write,
    Admin,
}

impl FromStr for AccessMode {
    type Err = PolicyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "write" => Ok(Self::Write),
            "admin" => Ok(Self::Admin),
            other => Err(PolicyError::Denied(format!(
                "unknown access mode '{other}' (expected read|write|admin)"
            ))),
        }
    }
}

impl AccessMode {
    pub fn allows_writes(self) -> bool {
        matches!(self, Self::Write | Self::Admin)
    }

    pub fn allows_admin(self) -> bool {
        matches!(self, Self::Admin)
    }
}

/// Refuse write/admin against a superuser unless override is set.
pub fn check_superuser_guard(
    mode: AccessMode,
    is_superuser: bool,
    override_flag: bool,
) -> Result<(), PolicyError> {
    if is_superuser && mode.allows_writes() && !override_flag {
        return Err(PolicyError::Denied(
            "refusing write/admin mode against a superuser — pass --i-know-what-im-doing to override"
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!("read".parse::<AccessMode>().unwrap(), AccessMode::Read);
        assert_eq!("WRITE".parse::<AccessMode>().unwrap(), AccessMode::Write);
        assert_eq!("admin".parse::<AccessMode>().unwrap(), AccessMode::Admin);
        assert!("nope".parse::<AccessMode>().is_err());
    }

    #[test]
    fn superuser_guard() {
        assert!(check_superuser_guard(AccessMode::Read, true, false).is_ok());
        assert!(check_superuser_guard(AccessMode::Write, true, false).is_err());
        assert!(check_superuser_guard(AccessMode::Write, true, true).is_ok());
        assert!(check_superuser_guard(AccessMode::Write, false, false).is_ok());
    }
}
