//! TOML unit declaration parser.
//!
//! Parses TOML files (`.taba.toml`) into [`taba_core::Unit`] objects.
//! The parser supports all levels from the TOML schema (DL-015): from
//! the minimal 3-line workload (Level 0) through bounded tasks with
//! spawning (Level 5) and governance units.
//!
//! ## Identity placeholders
//!
//! The parser fills in placeholder identity fields (nil UUIDs, default
//! timestamps). The caller (typically the `apply` command) is
//! responsible for replacing these with real values — the unit ID
//! (content-addressed), author (from keypair), trust domain (from
//! config), and cluster ID — before inserting into the graph.
//!
//! ## Artifact shorthand
//!
//! The `[unit]` section uses shorthand keys to infer the artifact type:
//!
//! | Key | Artifact type |
//! |-----|---------------|
//! | `image` | OCI container |
//! | `binary` | Native binary |
//! | `wasm` | WebAssembly module |
//! | `k8s` | Kubernetes manifest |
//!
//! If `type` is omitted, it is inferred from the artifact key. If no
//! artifact key is present and `type = "workload"`, the unit must
//! declare at least one provided capability.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::Deserialize;
use taba_common::{
    ContentDigest, DualClockEvent, LogicalClock, UnitId, ValidityWindow, Version, WallTime,
};
use taba_core::{
    Artifact, ArtifactType, Capability, Classification, ConflictTuple, CrashBehavior,
    CrossDomainCapabilityDef, DataSchema, FailureSemantics, GovernanceUnit, HealthCheck,
    HealthCheckType, OomBehavior, PolicyResolution, PolicyUnit, PromotionGateDef, PromotionMode,
    PromotionTransition, RecoveryRelationship, RetentionMode, RetentionPolicy, ScaleDirection,
    Scaling, ScalingTrigger, ShutdownBehavior, SpawnContext, StateRecovery, Tolerances,
    TrustDeclaration, Unit, UnitHeader, UnitState, WorkloadKind, WorkloadUnit,
};

use crate::error::CliError;

// ===========================================================================
// TOML-facing structs (serde Deserialize)
// ===========================================================================

/// Top-level TOML structure for a unit declaration.
///
/// Mirrors the progressive disclosure schema from DL-015. Only the
/// `[unit]` section with `name` is required; all other sections are
/// optional and filled with defaults per the spec.
#[derive(Debug, Clone, Deserialize)]
struct UnitDeclaration {
    unit: UnitSection,
    #[serde(default)]
    needs: BTreeMap<String, CapDecl>,
    #[serde(default)]
    provides: BTreeMap<String, CapDecl>,
    scaling: Option<ScalingDecl>,
    tolerates: Option<TolerancesDecl>,
    failure: Option<FailureDecl>,
    recovery: Option<RecoveryDecl>,
    health: Option<HealthDecl>,
    retention: Option<RetentionDecl>,
    classification: Option<ClassificationDecl>,
    schema: Option<SchemaDecl>,
    storage: Option<StorageDecl>,
    conflict: Option<ConflictDecl>,
    resolution: Option<ResolutionDecl>,
    deadline: Option<DeadlineDecl>,
    spawn: Option<SpawnDecl>,
    trust_domain_def: Option<TrustDomainDefDecl>,
    transitions: Option<Vec<TransitionDecl>>,
    cross_domain: Option<CrossDomainDecl>,
}

/// The `[unit]` section.
#[derive(Debug, Clone, Deserialize)]
struct UnitSection {
    name: String,
    #[serde(rename = "type")]
    unit_type: Option<String>,
    kind: Option<String>,
    image: Option<String>,
    binary: Option<String>,
    wasm: Option<String>,
    k8s: Option<String>,
    digest: Option<String>,
    governance_type: Option<String>,
    supersedes: Option<String>,
    parent: Option<String>,
}

/// A capability declaration under `[needs]` or `[provides]`.
#[derive(Debug, Clone, Deserialize)]
struct CapDecl {
    #[serde(rename = "type")]
    cap_type: String,
    purpose: Option<String>,
}

/// `[scaling]` section.
#[derive(Debug, Clone, Deserialize)]
struct ScalingDecl {
    min: Option<u32>,
    max: Option<u32>,
    #[serde(default)]
    triggers: Vec<TriggerDecl>,
}

/// `[[scaling.triggers]]` entry.
#[derive(Debug, Clone, Deserialize)]
struct TriggerDecl {
    name: Option<String>,
    metric: String,
    threshold: u64,
    direction: String,
}

/// `[tolerates]` section.
#[derive(Debug, Clone, Deserialize)]
struct TolerancesDecl {
    max_latency: Option<String>,
    #[serde(default)]
    failure_modes: Vec<String>,
    consistency: Option<String>,
}

/// `[failure]` section.
///
/// Field names mirror the TOML keys (`on_oom`, `on_crash`,
/// `on_shutdown`) and must not be renamed.
#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::struct_field_names)]
struct FailureDecl {
    on_oom: Option<String>,
    on_crash: Option<CrashDecl>,
    on_shutdown: Option<ShutdownDecl>,
}

/// `on_crash` can be a table or a string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum CrashDecl {
    Backoff {
        restart_with_backoff: u32,
    },
    #[allow(dead_code)]
    Value(String),
}

/// `on_shutdown` can be a table or a string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ShutdownDecl {
    Drain {
        drain: String,
    },
    #[allow(dead_code)]
    Value(String),
}

/// `[recovery]` section.
#[derive(Debug, Clone, Deserialize)]
struct RecoveryDecl {
    strategy: String,
    stream: Option<String>,
    offset: Option<u64>,
    min_peers: Option<u32>,
}

/// `[health]` section.
#[derive(Debug, Clone, Deserialize)]
struct HealthDecl {
    #[serde(rename = "type")]
    check_type: String,
    path: Option<String>,
    port: Option<u16>,
    command: Option<String>,
    interval: Option<String>,
    timeout: Option<String>,
}

/// `[retention]` section.
#[derive(Debug, Clone, Deserialize)]
struct RetentionDecl {
    mode: Option<String>,
    duration: Option<String>,
    legal_basis: Option<String>,
    #[serde(default)]
    mandatory: bool,
}

/// `[classification]` section.
#[derive(Debug, Clone, Deserialize)]
struct ClassificationDecl {
    level: String,
}

/// `[schema]` section.
#[derive(Debug, Clone, Deserialize)]
struct SchemaDecl {
    format: String,
    definition: String,
}

/// `[storage]` section.
#[derive(Debug, Clone, Deserialize)]
struct StorageDecl {
    #[serde(default)]
    encrypted_at_rest: bool,
    #[serde(default)]
    jurisdictions: Vec<String>,
}

/// `[conflict]` section (policy units).
#[derive(Debug, Clone, Deserialize)]
struct ConflictDecl {
    units: Vec<String>,
    capability: String,
}

/// `[resolution]` section (policy units).
#[derive(Debug, Clone, Deserialize)]
struct ResolutionDecl {
    action: String,
    rationale: Option<String>,
    #[serde(default)]
    conditions: Vec<String>,
}

/// `[deadline]` section (bounded tasks).
#[derive(Debug, Clone, Deserialize)]
struct DeadlineDecl {
    wall_time: Option<String>,
    lc_range: Option<(u64, u64)>,
}

/// `[spawn]` section.
#[derive(Debug, Clone, Deserialize)]
struct SpawnDecl {
    max_depth: Option<u8>,
    // Parsed from TOML for schema completeness; not yet wired into
    // SpawnContext (planned for M6 bounded-task enforcement).
    #[allow(dead_code)]
    max_count: Option<u32>,
}

/// `[trust_domain]` section (governance units).
#[derive(Debug, Clone, Deserialize)]
struct TrustDomainDefDecl {
    description: Option<String>,
}

/// `[[transitions]]` entry (promotion gates).
#[derive(Debug, Clone, Deserialize)]
struct TransitionDecl {
    from: String,
    to: String,
    mode: String,
}

/// `[cross_domain]` section.
#[derive(Debug, Clone, Deserialize)]
struct CrossDomainDecl {
    #[serde(flatten)]
    provides: CapDecl,
    conditions: Option<String>,
}

// ===========================================================================
// Duration parsing
// ===========================================================================

/// Parses a duration string like `"50ms"`, `"30s"`, `"2h"`, `"7y"`.
///
/// Supported suffixes: `ms`, `s`, `m`, `h`, `d`, `y`. The value is a
/// positive integer (or decimal for sub-second precision).
///
/// # Errors
///
/// Returns [`CliError::InvalidInput`] if the string is not a valid
/// duration.
///
/// Durations are converted to integer milliseconds via `as u64`, which
/// saturates for out-of-range values (Rust ≥1.45). Negative durations
/// saturate to 0 — callers should treat 0 ms as an immediate duration.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn parse_duration(s: &str) -> Result<Duration, CliError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(CliError::InvalidInput {
            reason: "empty duration string".to_string(),
        });
    }

    // Find the suffix boundary: last non-digit character that starts the suffix.
    let digits_end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .ok_or_else(|| CliError::InvalidInput {
            reason: format!("duration '{s}' has no unit suffix"),
        })?;

    let (num_str, suffix) = s.split_at(digits_end);
    let num: f64 = num_str.parse().map_err(|_| CliError::InvalidInput {
        reason: format!("duration '{s}' has an invalid number: '{num_str}'"),
    })?;

    let millis = match suffix {
        "ms" => (num) as u64,
        "s" => (num * 1000.0) as u64,
        "m" => (num * 60.0 * 1000.0) as u64,
        "h" => (num * 3600.0 * 1000.0) as u64,
        "d" => (num * 86_400.0 * 1000.0) as u64,
        "y" => (num * 365.0 * 86_400.0 * 1000.0) as u64,
        other => {
            return Err(CliError::InvalidInput {
                reason: format!("duration '{s}' has unknown unit '{other}'"),
            });
        }
    };

    Ok(Duration::from_millis(millis))
}

/// Parses a duration string into a [`WallTime`] (milliseconds).
#[allow(clippy::cast_possible_truncation)]
fn parse_wall_time(s: &str) -> Result<WallTime, CliError> {
    Ok(WallTime {
        millis: parse_duration(s)?.as_millis() as u64,
    })
}

// ===========================================================================
// Capability parsing
// ===========================================================================

/// Converts a map of `CapDecl` into a sorted `Vec<Capability>`.
///
/// Each entry's key is the capability name. The value provides `type`
/// (required) and `purpose` (optional).
fn parse_capabilities(decls: &BTreeMap<String, CapDecl>) -> Vec<Capability> {
    let mut caps: Vec<Capability> = decls
        .iter()
        .map(|(name, decl)| {
            decl.purpose.as_ref().map_or_else(
                || Capability::new(&decl.cap_type, name),
                |purpose| Capability::with_purpose(&decl.cap_type, name, purpose),
            )
        })
        .collect();
    // Sort lexicographically by (cap_type, name, purpose) for INV-K2.
    caps.sort();
    caps
}

// ===========================================================================
// Artifact parsing
// ===========================================================================

/// Determines the artifact type and reference from the `[unit]` section.
///
/// Returns `None` if no artifact key is present (valid for data,
/// policy, and governance units, or for workloads that only declare
/// capabilities).
fn parse_artifact(unit: &UnitSection) -> Result<Option<Artifact>, CliError> {
    let artifact_keys: Vec<(&str, &Option<String>, ArtifactType)> = vec![
        ("image", &unit.image, ArtifactType::Oci),
        ("binary", &unit.binary, ArtifactType::Native),
        ("wasm", &unit.wasm, ArtifactType::Wasm),
        ("k8s", &unit.k8s, ArtifactType::K8sManifest),
    ];

    let found: Vec<(&str, &str, ArtifactType)> = artifact_keys
        .into_iter()
        .filter_map(|(key, opt, at)| opt.as_ref().map(|v| (key, v.as_str(), at)))
        .collect();

    match found.len() {
        0 => Ok(None),
        1 => {
            let (_, ref_str, at) = found[0];
            Ok(Some(Artifact {
                artifact_type: at,
                artifact_ref: ref_str.to_string(),
                digest: unit.digest.as_ref().map_or_else(
                    || ContentDigest("sha256:unset".to_string()),
                    |d| ContentDigest(d.clone()),
                ),
                requires: Vec::new(),
            }))
        }
        _ => {
            let keys: Vec<&str> = found.iter().map(|(k, _, _)| *k).collect();
            Err(CliError::InvalidInput {
                reason: format!("only one artifact key per unit, found: {}", keys.join(", ")),
            })
        }
    }
}

// ===========================================================================
// Placeholder header
// ===========================================================================

/// Creates a placeholder [`UnitHeader`] with nil identity.
///
/// The caller must replace `id`, `author`, `trust_domain`, and
/// `created_at` with real values before inserting into the graph.
fn placeholder_header() -> UnitHeader {
    UnitHeader {
        id: UnitId(uuid::Uuid::nil()),
        author: taba_common::AuthorId(uuid::Uuid::nil()),
        trust_domain: taba_common::TrustDomainId(uuid::Uuid::nil()),
        created_at: DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        },
        validity: None,
        state: UnitState::Declared,
        version: None,
    }
}

// ===========================================================================
// Unit construction
// ===========================================================================

/// Parses a TOML string into a [`Unit`].
///
/// The parser determines the unit type from the `type` field (or
/// inffers `workload` from an artifact key), fills in all default
/// values per the TOML schema, and returns a [`Unit`] with placeholder
/// identity.
///
/// # Errors
///
/// - [`CliError::TomlParse`] if the TOML is syntactically invalid.
/// - [`CliError::InvalidInput`] if required fields are missing or
///   values are semantically invalid.
pub fn parse_unit(toml_str: &str) -> Result<Unit, CliError> {
    let decl: UnitDeclaration = toml::from_str(toml_str)?;

    // Validate the unit name.
    if decl.unit.name.is_empty() {
        return Err(CliError::InvalidInput {
            reason: "unit name must not be empty".to_string(),
        });
    }

    // Determine the unit type, defaulting to "workload".
    let unit_type = decl.unit.unit_type.as_deref().unwrap_or("workload");

    match unit_type {
        "workload" => parse_workload(&decl),
        "data" => parse_data(&decl),
        "policy" => parse_policy(&decl),
        "governance" => parse_governance(&decl),
        other => Err(CliError::InvalidInput {
            reason: format!(
                "unknown unit type '{other}' (expected: workload, data, policy, governance)"
            ),
        }),
    }
}

/// Parses a workload unit from the declaration.
#[allow(clippy::too_many_lines)]
fn parse_workload(decl: &UnitDeclaration) -> Result<Unit, CliError> {
    let artifact = parse_artifact(&decl.unit)?;
    let kind = match decl.unit.kind.as_deref() {
        Some("bounded-task") => WorkloadKind::BoundedTask,
        Some("service") | None => WorkloadKind::Service,
        Some(other) => {
            return Err(CliError::InvalidInput {
                reason: format!(
                    "unknown workload kind '{other}' (expected: service, bounded-task)"
                ),
            });
        }
    };

    // Capabilities.
    let needs = parse_capabilities(&decl.needs);
    let mut provides = parse_capabilities(&decl.provides);

    // If no provides declared but we have an artifact, default to
    // providing a compute capability named after the unit.
    if provides.is_empty() && artifact.is_some() {
        provides = vec![Capability::new("compute", &decl.unit.name)];
    }
    if provides.is_empty() {
        return Err(CliError::InvalidInput {
            reason: "workload must declare at least one provided capability (use [provides] or an artifact key like 'image')".to_string(),
        });
    }

    // Scaling.
    let scaling = if let Some(s) = &decl.scaling {
        let triggers: Result<Vec<_>, CliError> = s
            .triggers
            .iter()
            .map(|t| {
                let direction = match t.direction.as_str() {
                    "up" => ScaleDirection::Up,
                    "down" => ScaleDirection::Down,
                    other => {
                        return Err(CliError::InvalidInput {
                            reason: format!(
                                "unknown scale direction '{other}' (expected: up, down)"
                            ),
                        });
                    }
                };
                Ok(ScalingTrigger {
                    name: t.name.clone().unwrap_or_else(|| t.metric.clone()),
                    metric: t.metric.clone(),
                    threshold: taba_common::Ppm(t.threshold),
                    direction,
                })
            })
            .collect();
        Scaling {
            min_instances: s.min.unwrap_or(1),
            max_instances: s.max.unwrap_or(1),
            triggers: triggers?,
        }
    } else {
        Scaling {
            min_instances: 1,
            max_instances: 1,
            triggers: Vec::new(),
        }
    };

    // Tolerances.
    let tolerates = decl.tolerates.as_ref().map_or_else(
        || Tolerances {
            max_latency: None,
            failure_modes: vec!["any".to_string()],
            consistency: None,
        },
        |t| Tolerances {
            max_latency: t.max_latency.as_ref().and_then(|s| {
                parse_duration(s)
                    .map_err(|e| CliError::InvalidInput {
                        reason: format!("invalid max_latency: {e}"),
                    })
                    .ok()
            }),
            failure_modes: if t.failure_modes.is_empty() {
                vec!["any".to_string()]
            } else {
                t.failure_modes.clone()
            },
            consistency: t.consistency.clone(),
        },
    );

    // Failure semantics.
    let failure_semantics = decl.failure.as_ref().map_or_else(
        || FailureSemantics {
            on_oom: OomBehavior::Restart,
            on_crash: CrashBehavior::Unexpected,
            on_shutdown: ShutdownBehavior::Immediate,
        },
        |f| {
            let on_oom = f.on_oom.as_deref().map_or(OomBehavior::Restart, |s| {
                match s {
                    "backoff-inputs" => OomBehavior::BackoffInputs,
                    "restart" => OomBehavior::Restart,
                    "fail-permanent" => OomBehavior::FailPermanent,
                    _other => OomBehavior::Restart, // unknown defaults to restart
                }
            });

            let on_crash = f.on_crash.as_ref().map_or_else(
                || CrashBehavior::Unexpected,
                |c| match c {
                    CrashDecl::Backoff {
                        restart_with_backoff,
                    } => CrashBehavior::RestartWithBackoff {
                        max_retries: *restart_with_backoff,
                    },
                    CrashDecl::Value(_) => CrashBehavior::Unexpected,
                },
            );

            let on_shutdown = f.on_shutdown.as_ref().map_or_else(
                || ShutdownBehavior::Immediate,
                |s| match s {
                    ShutdownDecl::Drain { drain } => ShutdownBehavior::DrainAndExit {
                        timeout: parse_duration(drain).unwrap_or(Duration::from_secs(30)),
                    },
                    ShutdownDecl::Value(_) => ShutdownBehavior::Immediate,
                },
            );

            FailureSemantics {
                on_oom,
                on_crash,
                on_shutdown,
            }
        },
    );

    // Recovery.
    let state_recovery = if let Some(r) = &decl.recovery {
        match r.strategy.as_str() {
            "stateless" => StateRecovery::Stateless,
            "replay-from-offset" => StateRecovery::ReplayFromOffset {
                stream: r.stream.clone().unwrap_or_default(),
                offset: r.offset.unwrap_or(0),
            },
            "require-quorum" => StateRecovery::RequireQuorum {
                min_peers: r.min_peers.unwrap_or(1),
            },
            other => {
                return Err(CliError::InvalidInput {
                    reason: format!(
                        "unknown recovery strategy '{other}' (expected: stateless, replay-from-offset, require-quorum)"
                    ),
                });
            }
        }
    } else {
        StateRecovery::Stateless
    };

    // Health check.
    let health_check = if let Some(h) = &decl.health {
        Some(parse_health_check(h)?)
    } else {
        None
    };

    // Validity window (for bounded tasks).
    let validity = if kind == WorkloadKind::BoundedTask {
        let deadline = decl
            .deadline
            .as_ref()
            .ok_or_else(|| CliError::InvalidInput {
                reason: "bounded task must declare a [deadline] section (INV-W2)".to_string(),
            })?;
        Some(ValidityWindow {
            lc_range: deadline
                .lc_range
                .map(|(from, to)| (LogicalClock(from), LogicalClock(to))),
            wall_time_deadline: deadline
                .wall_time
                .as_ref()
                .map(|s| parse_wall_time(s))
                .transpose()?,
        })
    } else {
        None
    };

    // Spawn context.
    let spawn_context = decl.spawn.as_ref().map(|sp| SpawnContext {
        spawned_by: UnitId(uuid::Uuid::nil()),
        delegation_token_id: taba_common::DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: sp.max_depth.unwrap_or(1),
    });

    // Trust declarations.
    let trusts: Vec<TrustDeclaration> = Vec::new();

    // Recovery relationships.
    let recovery_relationships: Vec<RecoveryRelationship> = Vec::new();

    // Build the header.
    let mut header = placeholder_header();
    header.validity = validity;

    // Build the workload unit.
    let workload = WorkloadUnit {
        header,
        kind,
        artifact: artifact.unwrap_or_else(|| Artifact {
            artifact_type: ArtifactType::Oci,
            artifact_ref: String::new(),
            digest: ContentDigest("sha256:unset".to_string()),
            requires: Vec::new(),
        }),
        needs,
        provides,
        tolerates,
        trusts,
        scaling,
        failure_semantics,
        recovery_relationships,
        state_recovery,
        placement_on_failure: None,
        health_check,
        spawn_context,
    };

    Ok(Unit::Workload(workload))
}

/// Parses a health check from the TOML declaration.
fn parse_health_check(h: &HealthDecl) -> Result<HealthCheck, CliError> {
    let check_type = match h.check_type.as_str() {
        "http" => {
            let path = h.path.clone().ok_or_else(|| CliError::InvalidInput {
                reason: "http health check requires 'path'".to_string(),
            })?;
            let port = h.port.ok_or_else(|| CliError::InvalidInput {
                reason: "http health check requires 'port'".to_string(),
            })?;
            HealthCheckType::Http { path, port }
        }
        "tcp" => {
            let port = h.port.ok_or_else(|| CliError::InvalidInput {
                reason: "tcp health check requires 'port'".to_string(),
            })?;
            HealthCheckType::Tcp { port }
        }
        "command" => {
            let command = h.command.clone().ok_or_else(|| CliError::InvalidInput {
                reason: "command health check requires 'command'".to_string(),
            })?;
            HealthCheckType::Command { command }
        }
        other => {
            return Err(CliError::InvalidInput {
                reason: format!(
                    "unknown health check type '{other}' (expected: http, tcp, command)"
                ),
            });
        }
    };

    let interval = h
        .interval
        .as_ref()
        .map(|s| parse_duration(s))
        .transpose()?
        .unwrap_or(Duration::from_secs(10));
    let timeout = h
        .timeout
        .as_ref()
        .map(|s| parse_duration(s))
        .transpose()?
        .unwrap_or(Duration::from_secs(2));

    Ok(HealthCheck {
        check_type,
        interval,
        timeout,
    })
}

/// Parses a data unit from the declaration.
fn parse_data(decl: &UnitDeclaration) -> Result<Unit, CliError> {
    // Schema is required.
    let schema = decl.schema.as_ref().ok_or_else(|| CliError::InvalidInput {
        reason: "data unit must declare a [schema] section".to_string(),
    })?;
    let data_schema = DataSchema {
        format: schema.format.clone(),
        definition: schema.definition.clone(),
    };

    // Classification (default: Internal).
    let classification = if let Some(c) = &decl.classification {
        match c.level.as_str() {
            "public" => Classification::Public,
            "internal" => Classification::Internal,
            "confidential" => Classification::Confidential,
            "pii" => Classification::Pii,
            other => {
                return Err(CliError::InvalidInput {
                    reason: format!(
                        "unknown classification level '{other}' (expected: public, internal, confidential, pii)"
                    ),
                });
            }
        }
    } else {
        Classification::Internal
    };

    // Retention (default: persistent).
    let retention = if let Some(r) = &decl.retention {
        let mode = r
            .mode
            .as_deref()
            .map_or(RetentionMode::Persistent, |s| match s {
                "ephemeral" => RetentionMode::Ephemeral,
                "local-only" => RetentionMode::LocalOnly,
                _ => RetentionMode::Persistent,
            });
        let duration = r
            .duration
            .as_ref()
            .map(|s| parse_duration(s))
            .transpose()
            .map_err(|e| CliError::InvalidInput {
                reason: format!("invalid retention duration: {e}"),
            })?;
        let legal_basis = r
            .legal_basis
            .clone()
            .unwrap_or_else(|| "unspecified".to_string());
        RetentionPolicy {
            mode,
            duration,
            legal_basis,
            mandatory: r.mandatory,
        }
    } else {
        RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: None,
            legal_basis: "unspecified".to_string(),
            mandatory: false,
        }
    };

    // Storage requirements.
    let storage_requirements = decl.storage.as_ref().map_or_else(
        || taba_core::StorageRequirements {
            encrypted_at_rest: false,
            jurisdictions: Vec::new(),
            min_replicas: None,
        },
        |s| taba_core::StorageRequirements {
            encrypted_at_rest: s.encrypted_at_rest,
            jurisdictions: s.jurisdictions.clone(),
            min_replicas: None,
        },
    );

    // Provenance (None for new data units).
    let provenance = None;

    // Consent scopes (empty by default).
    let consent_scope = Vec::new();

    // Parent (for hierarchical data).
    let parent = decl
        .unit
        .parent
        .as_ref()
        .map(|_name| UnitId(uuid::Uuid::new_v4())); // Placeholder — resolved by apply command.

    // Provides (default to a storage capability named after the unit).
    let provides = parse_capabilities(&decl.provides);
    let provides = if provides.is_empty() {
        vec![Capability::new("storage", &decl.unit.name)]
    } else {
        provides
    };

    let data_unit = taba_core::unit::DataUnit {
        header: placeholder_header(),
        schema: data_schema,
        classification,
        provenance,
        retention,
        consent_scope,
        storage_requirements,
        parent,
        provides,
    };

    Ok(Unit::Data(data_unit))
}

/// Parses a policy unit from the declaration.
fn parse_policy(decl: &UnitDeclaration) -> Result<Unit, CliError> {
    // Conflict is required.
    let conflict = decl
        .conflict
        .as_ref()
        .ok_or_else(|| CliError::InvalidInput {
            reason: "policy unit must declare a [conflict] section".to_string(),
        })?;
    if conflict.units.is_empty() {
        return Err(CliError::InvalidInput {
            reason: "policy conflict must reference at least one unit".to_string(),
        });
    }

    // Resolution is required.
    let resolution = decl
        .resolution
        .as_ref()
        .ok_or_else(|| CliError::InvalidInput {
            reason: "policy unit must declare a [resolution] section".to_string(),
        })?;

    let policy_resolution = match resolution.action.as_str() {
        "allow" => PolicyResolution::Allow,
        "deny" => PolicyResolution::Deny,
        "conditional" => PolicyResolution::Conditional {
            conditions: resolution.conditions.clone(),
        },
        other => {
            return Err(CliError::InvalidInput {
                reason: format!(
                    "unknown resolution action '{other}' (expected: allow, deny, conditional)"
                ),
            });
        }
    };

    // Conflict tuple (unit names → placeholder UUIDs).
    let unit_ids: BTreeSet<taba_common::UnitId> = conflict
        .units
        .iter()
        .map(|_name| UnitId(uuid::Uuid::new_v4()))
        .collect();
    let conflict_tuple = ConflictTuple {
        unit_ids,
        capability_name: conflict.capability.clone(),
    };

    // Supersedes (placeholder UUID).
    let supersedes = decl
        .unit
        .supersedes
        .as_ref()
        .map(|_name| UnitId(uuid::Uuid::new_v4())); // Placeholder — resolved by apply command.

    let rationale = resolution
        .rationale
        .clone()
        .unwrap_or_else(|| "No rationale provided".to_string());

    let policy_unit = PolicyUnit {
        header: placeholder_header(),
        conflict: conflict_tuple,
        resolution: policy_resolution,
        scope: taba_common::TrustDomainId(uuid::Uuid::nil()),
        rationale,
        supersedes,
        version: Version(1),
        revoked: false,
    };

    Ok(Unit::Policy(policy_unit))
}

/// Parses a governance unit from the declaration.
fn parse_governance(decl: &UnitDeclaration) -> Result<Unit, CliError> {
    let gov_type = decl
        .unit
        .governance_type
        .as_ref()
        .ok_or_else(|| CliError::InvalidInput {
            reason: "governance unit must declare 'governance_type'".to_string(),
        })?;

    match gov_type.as_str() {
        "trust-domain" => {
            let td_def = decl.trust_domain_def.as_ref();
            let description = td_def
                .and_then(|t| t.description.clone())
                .unwrap_or_default();

            let gov = GovernanceUnit::TrustDomainDef(taba_core::TrustDomainDef {
                header: placeholder_header(),
                domain_id: taba_common::TrustDomainId(uuid::Uuid::new_v4()),
                name: decl.unit.name.clone(),
                description,
                // Solo bootstrap: the author is the sole signer.
                // INV-S10 requires 2+ signers for trust domain creation,
                // but Tier 0 solo bootstrap is exempt.
                signers: vec![taba_common::AuthorId(uuid::Uuid::nil()); 2],
                expires_at: None,
            });
            Ok(Unit::Governance(gov))
        }
        "promotion-gate" => {
            let transitions = decl
                .transitions
                .as_ref()
                .ok_or_else(|| CliError::InvalidInput {
                    reason: "promotion-gate must declare [[transitions]]".to_string(),
                })?;
            let trans: Vec<PromotionTransition> = transitions
                .iter()
                .map(|t| {
                    let mode = match t.mode.as_str() {
                        "human-approval" => PromotionMode::HumanApproval,
                        _ => PromotionMode::Auto,
                    };
                    PromotionTransition {
                        from_env: t.from.clone(),
                        to_env: t.to.clone(),
                        mode,
                    }
                })
                .collect();
            let gov = GovernanceUnit::PromotionGate(PromotionGateDef {
                header: placeholder_header(),
                transitions: trans,
            });
            Ok(Unit::Governance(gov))
        }
        "cross-domain-capability" => {
            let cd = decl
                .cross_domain
                .as_ref()
                .ok_or_else(|| CliError::InvalidInput {
                    reason: "cross-domain-capability must declare [cross_domain]".to_string(),
                })?;
            let provides = cd.provides.purpose.as_ref().map_or_else(
                || Capability::new(&cd.provides.cap_type, &decl.unit.name),
                |purpose| Capability::with_purpose(&cd.provides.cap_type, &decl.unit.name, purpose),
            );
            let conditions = cd.conditions.clone().unwrap_or_default();
            let gov = GovernanceUnit::CrossDomainCapability(CrossDomainCapabilityDef {
                header: placeholder_header(),
                provides,
                conditions,
            });
            Ok(Unit::Governance(gov))
        }
        other => Err(CliError::InvalidInput {
            reason: format!(
                "unknown governance_type '{other}' (expected: trust-domain, promotion-gate, cross-domain-capability)"
            ),
        }),
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_workload() {
        let toml = r#"
[unit]
name = "hello-web"
image = "hello:latest"
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        assert!(
            matches!(unit, Unit::Workload(_)),
            "should parse as workload"
        );
        if let Unit::Workload(w) = unit {
            assert_eq!(w.artifact.artifact_type, ArtifactType::Oci);
            assert_eq!(w.artifact.artifact_ref, "hello:latest");
            assert_eq!(w.kind, WorkloadKind::Service);
            assert!(!w.provides.is_empty(), "should have default provides");
        }
    }

    #[test]
    fn test_parse_with_capabilities() {
        let toml = r#"
[unit]
name = "api-server"
image = "api:v2.1.0"

[needs]
postgres = { type = "storage" }
redis = { type = "storage", purpose = "cache" }

[provides]
http-api = { type = "network", purpose = "user-facing" }
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.needs.len(), 2, "should have 2 needs");
            assert_eq!(w.provides.len(), 1, "should have 1 provide");
            // Needs should be sorted (INV-K2).
            assert!(w.needs[0] < w.needs[1], "needs should be sorted");
        }
    }

    #[test]
    fn test_parse_with_scaling() {
        let toml = r#"
[unit]
name = "api-server"
image = "api:v2.1.0"

[scaling]
min = 2
max = 10

[[scaling.triggers]]
metric = "cpu_ppm"
threshold = 700_000
direction = "up"
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.scaling.min_instances, 2);
            assert_eq!(w.scaling.max_instances, 10);
            assert_eq!(w.scaling.triggers.len(), 1);
            assert_eq!(w.scaling.triggers[0].metric, "cpu_ppm");
            assert_eq!(w.scaling.triggers[0].direction, ScaleDirection::Up);
        }
    }

    #[test]
    fn test_parse_data_unit() {
        let toml = r#"
[unit]
name = "customer-profiles"
type = "data"

[schema]
format = "json-schema"
definition = "schemas/customer.json"

[classification]
level = "pii"

[retention]
mode = "persistent"
duration = "7y"
legal_basis = "GDPR Art. 6(1)(b)"
mandatory = true
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Data(d) = unit {
            assert_eq!(d.schema.format, "json-schema");
            assert_eq!(d.classification, Classification::Pii);
            assert_eq!(d.retention.mode, RetentionMode::Persistent);
            assert!(d.retention.duration.is_some());
            assert_eq!(d.retention.legal_basis, "GDPR Art. 6(1)(b)");
            assert!(d.retention.mandatory);
            assert!(!d.provides.is_empty(), "should have default provides");
        }
    }

    #[test]
    fn test_parse_policy_unit() {
        let toml = r#"
[unit]
name = "allow-analytics-access"
type = "policy"

[conflict]
units = ["customer-profiles", "analytics-pipeline"]
capability = "customer-data"

[resolution]
action = "allow"
rationale = "Analytics pipeline has DPA"
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Policy(p) = unit {
            assert_eq!(p.conflict.unit_ids.len(), 2);
            assert_eq!(p.conflict.capability_name, "customer-data");
            assert_eq!(p.resolution, PolicyResolution::Allow);
            assert_eq!(p.rationale, "Analytics pipeline has DPA");
        }
    }

    #[test]
    fn test_parse_governance_unit() {
        let toml = r#"
[unit]
name = "production-domain"
type = "governance"
governance_type = "trust-domain"

[trust_domain]
description = "Production workloads for ACME Corp"
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        assert!(
            matches!(unit, Unit::Governance(GovernanceUnit::TrustDomainDef(_))),
            "should parse as TrustDomainDef"
        );
    }

    #[test]
    fn test_parse_bounded_task() {
        let toml = r#"
[unit]
name = "nightly-etl"
kind = "bounded-task"
image = "etl:v1.2.0"

[deadline]
wall_time = "2h"

[spawn]
max_depth = 2
max_count = 10
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.kind, WorkloadKind::BoundedTask);
            assert!(
                w.header.validity.is_some(),
                "bounded task must have validity"
            );
            assert!(w.spawn_context.is_some(), "should have spawn context");
        }
    }

    #[test]
    fn test_parse_with_health_check() {
        let toml = r#"
[unit]
name = "api-server"
image = "api:v2.1.0"

[health]
type = "http"
path = "/healthz"
port = 8080
interval = "10s"
timeout = "2s"
"#;
        let unit = parse_unit(toml).expect("parse should succeed");
        if let Unit::Workload(w) = unit {
            let hc = w.health_check.expect("should have health check");
            assert!(matches!(hc.check_type, HealthCheckType::Http { .. }));
            assert_eq!(hc.interval, Duration::from_secs(10));
            assert_eq!(hc.timeout, Duration::from_secs(2));
        }
    }

    #[test]
    fn test_parse_artifact_shorthand() {
        // image → OCI
        let unit = parse_unit(
            r#"[unit]
name = "a"
image = "x:latest""#,
        )
        .expect("parse");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.artifact.artifact_type, ArtifactType::Oci);
        }

        // binary → Native
        let unit = parse_unit(
            r#"[unit]
name = "b"
binary = "bin/app""#,
        )
        .expect("parse");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.artifact.artifact_type, ArtifactType::Native);
        }

        // wasm → Wasm
        let unit = parse_unit(
            r#"[unit]
name = "c"
wasm = "module.wasm""#,
        )
        .expect("parse");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.artifact.artifact_type, ArtifactType::Wasm);
        }

        // k8s → K8sManifest
        let unit = parse_unit(
            r#"[unit]
name = "d"
k8s = "pod-spec.yaml""#,
        )
        .expect("parse");
        if let Unit::Workload(w) = unit {
            assert_eq!(w.artifact.artifact_type, ArtifactType::K8sManifest);
        }
    }

    #[test]
    fn test_parse_duration_string() {
        assert_eq!(parse_duration("50ms").unwrap(), Duration::from_millis(50));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
        assert_eq!(
            parse_duration("7y").unwrap(),
            Duration::from_secs(7 * 365 * 86400)
        );
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
    }

    #[test]
    fn test_parse_invalid_toml() {
        let result = parse_unit("this is not valid toml {{{");
        assert!(result.is_err(), "invalid TOML should return an error");
    }

    #[test]
    fn test_parse_missing_required_field() {
        // Data unit without schema.
        let toml = r#"
[unit]
name = "no-schema"
type = "data"
"#;
        let result = parse_unit(toml);
        assert!(
            result.is_err(),
            "data unit without schema should return error"
        );
    }

    #[test]
    fn test_parse_policy_conditional_resolution() {
        let toml = r#"
[unit]
name = "conditional-policy"
type = "policy"

[conflict]
units = ["unit-a"]
capability = "storage"

[resolution]
action = "conditional"
conditions = ["processing within EU only", "audit trail enabled"]
rationale = "Updated per legal review"
"#;
        let unit = parse_unit(toml).expect("parse");
        if let Unit::Policy(p) = unit {
            match &p.resolution {
                PolicyResolution::Conditional { conditions } => {
                    assert_eq!(conditions.len(), 2);
                }
                other => panic!("expected Conditional, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_parse_governance_promotion_gate() {
        let toml = r#"
[unit]
name = "prod-gate"
type = "governance"
governance_type = "promotion-gate"

[[transitions]]
from = "dev"
to = "test"
mode = "auto"

[[transitions]]
from = "test"
to = "prod"
mode = "human-approval"
"#;
        let unit = parse_unit(toml).expect("parse");
        if let Unit::Governance(GovernanceUnit::PromotionGate(gate)) = unit {
            assert_eq!(gate.transitions.len(), 2);
            assert_eq!(gate.transitions[0].mode, PromotionMode::Auto);
            assert_eq!(gate.transitions[1].mode, PromotionMode::HumanApproval);
        }
    }

    #[test]
    fn test_parse_data_unit_with_parent() {
        let toml = r#"
[unit]
name = "eu-customers"
type = "data"
parent = "customer-profiles"

[schema]
format = "json-schema"
definition = "schemas/eu.json"

[classification]
level = "pii"
"#;
        let unit = parse_unit(toml).expect("parse");
        if let Unit::Data(d) = unit {
            assert!(d.parent.is_some(), "should have parent");
        }
    }
}
