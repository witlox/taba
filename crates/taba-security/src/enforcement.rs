//! Runtime capability enforcement — zero default, fail closed (INV-S1, INV-S2).
//!
//! A unit can only access capabilities it explicitly declared AND that
//! policy approved. Zero default: no capability is implicitly granted.
//! Fail closed (INV-S2): if the check is ambiguous or policy is missing,
//! access is denied.

use std::collections::{HashMap, HashSet};

use taba_common::UnitId;
use taba_core::Capability;

use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// CapabilityEnforcer trait
// ---------------------------------------------------------------------------

/// Runtime enforcement of declared vs. allowed capabilities.
///
/// Enforces INV-S1: a unit can only access capabilities it explicitly
/// declared AND that policy approved. Zero default. Fail closed on
/// ambiguity (INV-S2).
pub trait CapabilityEnforcer {
    /// Check whether a unit is permitted to exercise a specific capability.
    ///
    /// Returns [`Ok`] if the capability is in the unit's granted set.
    /// Returns [`SecurityError::CapabilityDenied`] if access is not
    /// permitted, or if the unit is unknown (fail closed, INV-S2).
    fn check_access(&self, unit: &UnitId, capability: &Capability) -> Result<(), SecurityError>;

    /// Check whether a set of capabilities are all permitted for a unit.
    ///
    /// Short-circuits on the first denial. Returns the specific
    /// [`SecurityError`] for the first capability that was denied.
    fn check_all(&self, unit: &UnitId, capabilities: &[Capability]) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultCapabilityEnforcer
// ---------------------------------------------------------------------------

/// Default implementation of [`CapabilityEnforcer`].
///
/// Holds a map from [`UnitId`] to the set of granted capabilities.
/// Access is checked by looking up the unit and verifying the
/// capability is in the granted set. If the unit is unknown or the
/// capability is missing, access is denied (fail closed, INV-S2).
#[derive(Debug, Clone, Default)]
pub struct DefaultCapabilityEnforcer {
    /// Map from unit ID to the set of capabilities granted to that unit.
    granted: HashMap<UnitId, HashSet<Capability>>,
}

impl DefaultCapabilityEnforcer {
    /// Creates a new empty enforcer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            granted: HashMap::new(),
        }
    }

    /// Grants a capability to a unit.
    pub fn grant(&mut self, unit: UnitId, capability: Capability) {
        self.granted.entry(unit).or_default().insert(capability);
    }

    /// Grants multiple capabilities to a unit.
    pub fn grant_all(&mut self, unit: UnitId, capabilities: Vec<Capability>) {
        self.granted.entry(unit).or_default().extend(capabilities);
    }
}

impl CapabilityEnforcer for DefaultCapabilityEnforcer {
    fn check_access(&self, unit: &UnitId, capability: &Capability) -> Result<(), SecurityError> {
        match self.granted.get(unit) {
            Some(caps) if caps.contains(capability) => Ok(()),
            Some(_) => Err(SecurityError::CapabilityDenied {
                capability: format!("{}:{}", capability.cap_type, capability.name),
                reason: "capability not in granted set".to_string(),
            }),
            None => Err(SecurityError::CapabilityDenied {
                capability: format!("{}:{}", capability.cap_type, capability.name),
                reason: "unit unknown — no capabilities granted (fail closed, INV-S2)".to_string(),
            }),
        }
    }

    fn check_all(&self, unit: &UnitId, capabilities: &[Capability]) -> Result<(), SecurityError> {
        for capability in capabilities {
            self.check_access(unit, capability)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_access_allowed() {
        let unit = UnitId(uuid::Uuid::new_v4());
        let cap = Capability::new("storage", "postgres");

        let mut enforcer = DefaultCapabilityEnforcer::new();
        enforcer.grant(unit, cap.clone());

        assert!(
            enforcer.check_access(&unit, &cap).is_ok(),
            "capability in granted set should be allowed"
        );
    }

    #[test]
    fn test_check_access_denied() {
        let unit = UnitId(uuid::Uuid::new_v4());
        let granted_cap = Capability::new("storage", "postgres");
        let denied_cap = Capability::new("compute", "cpu");

        let mut enforcer = DefaultCapabilityEnforcer::new();
        enforcer.grant(unit, granted_cap);

        let result = enforcer.check_access(&unit, &denied_cap);
        assert!(
            matches!(result, Err(SecurityError::CapabilityDenied { .. })),
            "capability not in granted set should be denied, got: {result:?}"
        );
    }

    #[test]
    fn test_check_access_unknown_unit_fail_closed() {
        let unit = UnitId(uuid::Uuid::new_v4());
        let cap = Capability::new("storage", "postgres");

        // No capabilities granted at all.
        let enforcer = DefaultCapabilityEnforcer::new();

        let result = enforcer.check_access(&unit, &cap);
        assert!(
            matches!(result, Err(SecurityError::CapabilityDenied { .. })),
            "unknown unit should be denied (fail closed, INV-S2), got: {result:?}"
        );
    }

    #[test]
    fn test_check_all_all_allowed() {
        let unit = UnitId(uuid::Uuid::new_v4());
        let caps = vec![
            Capability::new("storage", "postgres"),
            Capability::new("compute", "cpu"),
            Capability::new("network", "http"),
        ];

        let mut enforcer = DefaultCapabilityEnforcer::new();
        enforcer.grant_all(unit, caps.clone());

        assert!(
            enforcer.check_all(&unit, &caps).is_ok(),
            "all granted capabilities should be allowed"
        );
    }

    #[test]
    fn test_check_all_first_denial() {
        let unit = UnitId(uuid::Uuid::new_v4());
        let cap_ok = Capability::new("storage", "postgres");
        let cap_denied = Capability::new("compute", "cpu");
        let cap_unchecked = Capability::new("network", "http");

        let mut enforcer = DefaultCapabilityEnforcer::new();
        // Only grant cap_ok — cap_denied and cap_unchecked are not granted.
        enforcer.grant(unit, cap_ok.clone());

        // The list starts with cap_ok (granted), then cap_denied (not granted).
        // check_all should short-circuit on cap_denied and never check cap_unchecked.
        let caps = vec![cap_ok, cap_denied, cap_unchecked];

        let result = enforcer.check_all(&unit, &caps);
        assert!(
            matches!(result, Err(SecurityError::CapabilityDenied { ref capability, .. })
                if capability == "compute:cpu"),
            "check_all should short-circuit on first denial (compute:cpu), got: {result:?}"
        );
    }
}
