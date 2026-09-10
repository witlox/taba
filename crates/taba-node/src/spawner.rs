//! Task spawning via delegation tokens (INV-W4, INV-W4a).
//!
//! The [`TaskSpawner`] trait spawns bounded tasks at runtime using
//! pre-signed delegation tokens. It validates the token, checks
//! governance blocks (INV-W4a — spawned tasks can't create
//! policy/governance), and inserts the spawned task into the graph.

use std::collections::HashMap;
use std::sync::Mutex;

use taba_common::UnitId;
use taba_core::{DelegationToken, Unit, UnitKind};
use taba_security::DelegationValidator;

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// TaskSpawner trait
// ---------------------------------------------------------------------------

/// Spawns bounded tasks at runtime via delegation tokens (INV-W4).
///
/// The spawner uses a delegation token (pre-signed by the author at
/// placement time) to validate spawned tasks. It never holds the
/// author's private key.
pub trait TaskSpawner: Send + Sync {
    /// Spawn a bounded task from a running service.
    ///
    /// 1. Validate delegation token (calls taba-security
    ///    `DelegationValidator`)
    /// 2. Check governance block (INV-W4a — spawned tasks can't
    ///    create policy/governance)
    /// 3. Insert the spawned task into the graph
    /// 4. Track spawn count against token limit
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the token is invalid,
    ///   expired, revoked, or spawn limit is exceeded.
    /// - [`NodeError::DegradedModeRestriction`] if the spawned task
    ///   attempts a governance operation (INV-W4a).
    async fn spawn(&self, token: &DelegationToken, task_unit: Unit) -> Result<UnitId, NodeError>;
}

// ---------------------------------------------------------------------------
// DefaultTaskSpawner
// ---------------------------------------------------------------------------

/// Default implementation of [`TaskSpawner`].
///
/// Uses a [`DelegationValidator`] to validate delegation tokens and a
/// `HashMap` to track spawn counts per token ID. For M3, the graph
/// insertion is logged but not actually performed — the spawner
/// validates the token, checks governance blocks, and returns the
/// spawned unit's ID.
///
/// Thread-safe via interior mutability.
pub struct DefaultTaskSpawner<V: DelegationValidator + Send + Sync> {
    /// The delegation validator.
    validator: V,
    /// Spawn counts per token ID.
    spawn_counts: Mutex<HashMap<taba_common::DelegationTokenId, u32>>,
}

impl<V: DelegationValidator + Send + Sync> DefaultTaskSpawner<V> {
    /// Creates a new `DefaultTaskSpawner` with the given validator.
    #[must_use]
    pub fn new(validator: V) -> Self {
        Self {
            validator,
            spawn_counts: Mutex::new(HashMap::new()),
        }
    }
}

impl<V: DelegationValidator + Send + Sync> TaskSpawner for DefaultTaskSpawner<V> {
    async fn spawn(&self, token: &DelegationToken, task_unit: Unit) -> Result<UnitId, NodeError> {
        let unit_id = task_unit.id();
        let unit_kind = task_unit.kind();

        // Step 1: Validate delegation token.
        //
        // Use the task unit's creation logical clock for validation.
        let spawned_lc = task_unit.header().created_at.logical_clock;
        self.validator.validate(token, &spawned_lc).map_err(|e| {
            NodeError::ReconciliationFailed {
                unit: unit_id,
                reason: format!("delegation token invalid: {e}"),
            }
        })?;

        // Step 2: Check governance block (INV-W4a).
        //
        // Spawned tasks cannot create policy, governance, or
        // initiate declassification.
        let unit_type_str = match unit_kind {
            UnitKind::Policy => "policy",
            UnitKind::Governance => "governance",
            UnitKind::Workload => "workload",
            UnitKind::Data => "data",
        };
        self.validator
            .check_governance_block(token, unit_type_str)
            .map_err(|e| NodeError::DegradedModeRestriction {
                operation: format!("spawn {unit_type_str} unit (INV-W4a): {e}"),
            })?;

        // Step 3: Check spawn count against token limit.
        {
            let mut counts = self
                .spawn_counts
                .lock()
                .expect("spawn_counts mutex should not be poisoned");

            let count = counts.entry(token.id).or_insert(0);
            if *count >= token.max_spawns {
                return Err(NodeError::ReconciliationFailed {
                    unit: unit_id,
                    reason: format!(
                        "spawn limit exceeded: {} spawns (max {})",
                        *count, token.max_spawns
                    ),
                });
            }
            *count += 1;
        }

        // Step 4: Insert into graph (M3: logged, not performed).
        // In a full implementation, this would call graph.insert(task_unit).
        tracing::debug!(unit_id = %unit_id.0, kind = ?unit_kind, "task spawned via delegation token");

        Ok(unit_id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, ValidityWindow, WallTime};
    use taba_core::{SpawnContext, WorkloadKind};
    use taba_security::{DefaultDelegationValidator, DelegationManager, KeyPair};
    use taba_test_harness::WorkloadUnitBuilder;

    /// Creates a valid delegation token for testing.
    fn valid_token(max_spawns: u32) -> (DelegationToken, DefaultDelegationValidator, KeyPair) {
        let key_pair = KeyPair::generate();

        let service_id = UnitId(uuid::Uuid::new_v4());
        let node_id = taba_common::NodeId(uuid::Uuid::new_v4());
        let trust_domain = taba_common::TrustDomainId(uuid::Uuid::new_v4());
        let lc_start = LogicalClock(0);
        let lc_end = LogicalClock(u64::MAX);

        let manager = taba_security::DefaultDelegationManager::new();
        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        let mut validator = DefaultDelegationValidator::new();
        validator.add_token(token.clone(), *key_pair.public_key());

        (token, validator, key_pair)
    }

    /// Creates a workload unit for spawning.
    fn spawnable_unit() -> Unit {
        let mut builder = WorkloadUnitBuilder::new();
        builder = builder
            .with_kind(WorkloadKind::BoundedTask)
            .with_validity(ValidityWindow {
                lc_range: Some((LogicalClock(10), LogicalClock(100))),
                wall_time_deadline: Some(WallTime { millis: 1_000_000 }),
            });
        // Set spawn context so the unit is a spawned task.
        builder = builder.with_spawn_context(SpawnContext {
            spawned_by: UnitId(uuid::Uuid::new_v4()),
            delegation_token_id: taba_common::DelegationTokenId(uuid::Uuid::new_v4()),
            spawn_depth: 1,
        });
        Unit::Workload(builder.build())
    }

    #[tokio::test]
    async fn test_spawn_valid_token() {
        let (token, validator, _) = valid_token(10);
        let spawner = DefaultTaskSpawner::new(validator);

        let unit = spawnable_unit();
        let expected_id = unit.id();

        let result = spawner.spawn(&token, unit).await;

        assert!(
            result.is_ok(),
            "spawn with valid token should succeed, got: {result:?}"
        );
        assert_eq!(result.expect("spawn result"), expected_id);
    }

    #[tokio::test]
    async fn test_spawn_revoked_token() {
        let (mut token, mut validator, _) = valid_token(10);

        // Revoke the token.
        token.revoked = true;
        validator.add_token(token.clone(), taba_security::PublicKey([0u8; 32]));

        let spawner = DefaultTaskSpawner::new(validator);
        let unit = spawnable_unit();

        let result = spawner.spawn(&token, unit).await;

        assert!(
            result.is_err(),
            "spawn with revoked token should fail, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_spawn_governance_block() {
        let (token, validator, _) = valid_token(10);
        let spawner = DefaultTaskSpawner::new(validator);

        // Create a policy unit (spawned tasks can't create policy).
        let policy = taba_test_harness::PolicyUnitBuilder::new().build();
        let unit = Unit::Policy(policy);

        let result = spawner.spawn(&token, unit).await;

        assert!(
            matches!(result, Err(NodeError::DegradedModeRestriction { .. })),
            "spawn of policy unit should be blocked (INV-W4a), got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_spawn_limit_exceeded() {
        let (token, validator, _) = valid_token(1);
        let spawner = DefaultTaskSpawner::new(validator);

        // First spawn should succeed.
        let unit1 = spawnable_unit();
        spawner
            .spawn(&token, unit1)
            .await
            .expect("first spawn should succeed");

        // Second spawn should fail (limit = 1).
        let unit2 = spawnable_unit();
        let result = spawner.spawn(&token, unit2).await;

        assert!(
            matches!(result, Err(NodeError::ReconciliationFailed { .. })),
            "second spawn should fail (limit exceeded), got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_spawn_data_unit_allowed() {
        let (token, validator, _) = valid_token(10);
        let spawner = DefaultTaskSpawner::new(validator);

        // Data units are not governance — should be allowed.
        let data = taba_test_harness::DataUnitBuilder::new().build();
        let unit = Unit::Data(data);

        let result = spawner.spawn(&token, unit).await;

        assert!(
            result.is_ok(),
            "spawn of data unit should be allowed, got: {result:?}"
        );
    }
}
