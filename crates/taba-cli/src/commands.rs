//! Subcommand definitions and handlers for the taba CLI.
//!
//! Each subcommand is a function that takes a [`LocalClient`] (or
//! [`LocalAuth`] for `init`) and returns `Result<(), CliError>`. The
//! `main.rs` entry point parses the CLI with clap and dispatches to
//! these handlers.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::client::LocalClient;
use crate::error::CliError;
use crate::format::{self, OutputFormat};
use crate::parser;
use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};
use taba_common::{DualClockEvent, LogicalClock, UnitId, WallTime};
use taba_core::{DefaultValidator, Unit, UnitHeader, UnitState, UnitValidator};

// ===========================================================================
// CLI definitions
// ===========================================================================

/// The taba command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "taba",
    version,
    about = "Self-describing, capability-aware workload composition"
)]
pub struct Cli {
    /// State directory (default: ~/.taba).
    #[arg(long, global = true)]
    pub state_dir: Option<PathBuf>,

    /// Output format.
    #[arg(long, global = true, value_enum, default_value_t = OutputFormat::Table)]
    pub format: OutputFormat,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize the local node (Tier 0 solo bootstrap).
    Init {
        /// Re-initialize even if already initialized.
        #[arg(long)]
        force: bool,
    },

    /// Sign and insert a unit TOML into the graph.
    Apply {
        /// Path to the .taba.toml file.
        file: PathBuf,

        /// Validate and show what would be inserted, but don't insert.
        #[arg(long)]
        dry_run: bool,
    },

    /// Create, inspect, validate, list, or archive units.
    Unit {
        /// Subcommand for unit operations.
        #[command(subcommand)]
        sub: UnitCommand,
    },

    /// Show node health, graph stats, and operational mode.
    Status,

    /// Run the solver and show placements and conflicts.
    Compose,

    /// Audit lineage, provenance, and decision trails.
    Audit {
        /// Subcommand for audit operations.
        #[command(subcommand)]
        sub: AuditCommand,
    },

    /// Push an artifact to the local cache (air-gapped/dev).
    Push {
        /// Path to the artifact file to cache.
        file: PathBuf,

        /// Cache directory (default: ~/.taba/cache/).
        #[arg(long)]
        cache_dir: Option<PathBuf>,
    },
}

/// Subcommands for unit operations.
#[derive(Subcommand, Debug)]
pub enum UnitCommand {
    /// List all units in the graph.
    List,

    /// Show details of a specific unit.
    Inspect {
        /// The unit ID (UUID).
        id: String,
    },

    /// Validate a TOML file without inserting.
    Validate {
        /// Path to the .taba.toml file.
        file: PathBuf,
    },

    /// Archive a unit (soft-delete).
    Archive {
        /// The unit ID (UUID).
        id: String,
    },
}

/// Subcommands for audit operations.
#[derive(Subcommand, Debug)]
pub enum AuditCommand {
    /// Show provenance chain for a data unit.
    Provenance {
        /// The data unit ID (UUID).
        unit_id: String,
    },

    /// List decision trails.
    Trails,
}

// ===========================================================================
// Command handlers
// ===========================================================================

/// Runs the `init` command: initializes the local node.
///
/// Generates a keypair, creates a trust domain and governance unit,
/// and saves everything to the state directory.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if already initialized and `--force`
///   was not given.
/// - [`CliError::Security`] if key generation fails.
/// - [`CliError::Io`] if files cannot be written.
pub fn run_init(state_dir: Option<PathBuf>, force: bool) -> Result<(), CliError> {
    let auth = match state_dir {
        Some(dir) => crate::auth::LocalAuth::with_state_dir(dir)?,
        None => crate::auth::LocalAuth::new()?,
    };

    if auth.is_initialized() && !force {
        return Err(CliError::InvalidInput {
            reason: "node is already initialized. Use --force to re-initialize.".to_string(),
        });
    }

    let result = auth.init()?;

    println!("taba node initialized successfully.");
    println!();
    println!(
        "  Key ID:          {}",
        hex::encode(result.key_id.as_bytes())
    );
    println!(
        "  Public Key:      {}",
        hex::encode(result.public_key.to_public_key().as_bytes())
    );
    println!("  Trust Domain:    {}", result.trust_domain);
    println!("  Governance Unit: {}", result.governance_unit_id);
    println!("  Role Assignment: {}", result.role_assignment_id);
    println!();
    println!("State directory: {}", auth.state_dir().display());

    Ok(())
}

/// Runs the `apply` command: parses a TOML file, fills identity,
/// validates, and inserts into the graph.
///
/// With `--dry-run`, validates and prints the unit without inserting.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if the TOML is invalid or validation
///   fails.
/// - [`CliError::Graph`] if insertion fails.
pub async fn run_apply(
    state_dir: Option<PathBuf>,
    file: &PathBuf,
    dry_run: bool,
) -> Result<(), CliError> {
    let toml_str = std::fs::read_to_string(file).map_err(CliError::Io)?;
    let unit = parser::parse_unit(&toml_str)?;

    // Load the client (initializes if needed).
    let client = LocalClient::load(state_dir).await?;

    // Fill in identity.
    let unit = fill_identity(unit, &client);

    // Validate using DefaultValidator.
    let validator = DefaultValidator::empty();
    validator.validate(&unit).map_err(CliError::Core)?;

    if dry_run {
        println!("Valid unit (dry run — not inserted):");
        println!();
        println!("{}", format::format_unit(&unit, OutputFormat::Table));
        return Ok(());
    }

    // Insert into the graph.
    let unit_id = unit.id();
    client.insert_unit(unit).await?;
    println!("Unit {unit_id} inserted successfully.");
    Ok(())
}

/// Runs the `status` command: shows graph stats and operational mode.
///
/// # Errors
///
/// - [`CliError::Graph`] if the stats cannot be retrieved.
pub async fn run_status(state_dir: Option<PathBuf>, output: OutputFormat) -> Result<(), CliError> {
    let client = LocalClient::load(state_dir).await?;
    let stats = client.graph_stats();

    let mut out = format::format_stats(&stats, output);

    // Add operational mode and node info for table format.
    if output == OutputFormat::Table {
        out.push('\n');
        out.push_str("OPERATIONAL MODE: local (single-node)\n");
        let _ = writeln!(out, "NODE ID:          {}", client.node_id());
        let _ = writeln!(out, "TRUST DOMAIN:     {}", client.trust_domain());
        let _ = writeln!(out, "CLUSTER ID:       {}", client.cluster_id());
    }

    print!("{out}");
    Ok(())
}

/// Runs the `compose` command: takes a snapshot, runs the solver,
/// and shows placements and conflicts.
///
/// # Errors
///
/// - [`CliError::Graph`] if the snapshot cannot be taken.
pub async fn run_compose(state_dir: Option<PathBuf>, output: OutputFormat) -> Result<(), CliError> {
    let client = LocalClient::load(state_dir).await?;
    let snapshot = client.snapshot().await?;
    let result = client.solve(&snapshot);

    print!("{}", format::format_solver_result(&result, output));
    Ok(())
}

/// Runs the `unit list` command: lists all units in the graph.
///
/// # Errors
///
/// - [`CliError::Graph`] if the list cannot be retrieved.
pub async fn run_unit_list(
    state_dir: Option<PathBuf>,
    output: OutputFormat,
) -> Result<(), CliError> {
    let client = LocalClient::load(state_dir).await?;
    let units = client.list_units().await?;

    if units.is_empty() {
        println!("No units in the graph.");
        return Ok(());
    }

    print!("{}", format::format_units(&units, output));
    Ok(())
}

/// Runs the `unit inspect <id>` command: shows details of a unit.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if the ID is not a valid UUID.
/// - [`CliError::Graph`] if the unit is not found.
pub async fn run_unit_inspect(
    state_dir: Option<PathBuf>,
    id_str: &str,
    output: OutputFormat,
) -> Result<(), CliError> {
    let id = parse_unit_id(id_str)?;
    let client = LocalClient::load(state_dir).await?;
    let unit = client.get_unit(&id)?;

    print!("{}", format::format_unit(&unit, output));
    Ok(())
}

/// Runs the `unit validate <file>` command: validates a TOML file
/// without inserting.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if the TOML is invalid or validation
///   fails.
pub fn run_unit_validate(file: &PathBuf) -> Result<(), CliError> {
    let toml_str = std::fs::read_to_string(file)?;
    let unit = parser::parse_unit(&toml_str)?;

    // Structural validation (no author scope check since we have no
    // role assignments in local mode).
    let validator = DefaultValidator::empty();
    validator.validate(&unit)?;

    println!("Valid unit: {} ({})", unit.id(), unit.kind());
    Ok(())
}

/// Runs the `unit archive <id>` command: archives a unit.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if the ID is not a valid UUID.
/// - [`CliError::Graph`] if the unit is not found or cannot be
///   archived.
pub async fn run_unit_archive(state_dir: Option<PathBuf>, id_str: &str) -> Result<(), CliError> {
    let id = parse_unit_id(id_str)?;
    let client = LocalClient::load(state_dir).await?;
    client.archive_unit(&id).await?;

    println!("Unit {id} archived.");
    Ok(())
}

/// Runs the `audit provenance <unit-id>` command: shows the provenance
/// chain for a data unit.
///
/// # Errors
///
/// - [`CliError::InvalidInput`] if the ID is not a valid UUID.
/// - [`CliError::Graph`] if the unit is not found or provenance
///   traversal fails.
pub async fn run_audit_provenance(
    state_dir: Option<PathBuf>,
    unit_id_str: &str,
    output: OutputFormat,
) -> Result<(), CliError> {
    let id = parse_unit_id(unit_id_str)?;
    let client = LocalClient::load(state_dir).await?;
    let links = client.provenance(&id)?;

    if links.is_empty() {
        println!("No provenance chain found for unit {id}.");
        return Ok(());
    }

    print!("{}", format::format_provenance(&links, output));
    Ok(())
}

/// Runs the `audit trails` command: lists decision trails.
///
/// # Errors
///
/// - [`CliError::Observe`] if the query fails.
pub async fn run_audit_trails(
    state_dir: Option<PathBuf>,
    output: OutputFormat,
) -> Result<(), CliError> {
    let client = LocalClient::load(state_dir).await?;
    let trails = client.decision_trails()?;

    if trails.is_empty() {
        println!("No decision trails recorded.");
        return Ok(());
    }

    print!("{}", format::format_trails(&trails, output));
    Ok(())
}

/// Runs the `push <file>` command: computes SHA256 and copies the file
/// to the local cache directory.
///
/// # Errors
///
/// - [`CliError::Io`] if the file cannot be read or the cache
///   directory cannot be written.
pub fn run_push(file: &PathBuf, cache_dir: Option<PathBuf>) -> Result<(), CliError> {
    let data = std::fs::read(file)?;

    // Compute SHA256.
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let digest = hasher.finalize();
    let hex_digest = hex::encode(digest);

    // Determine cache directory.
    let cache_dir = cache_dir.unwrap_or_else(|| {
        std::env::var("HOME").map_or_else(
            |_| PathBuf::from(".taba").join("cache"),
            |h| PathBuf::from(h).join(".taba").join("cache"),
        )
    });

    if !cache_dir.exists() {
        std::fs::create_dir_all(&cache_dir)?;
    }

    // Use the SHA256 hex as the filename, preserving the original
    // extension if any.
    let ext = file
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dest = cache_dir.join(format!("{hex_digest}{ext}"));

    std::fs::write(&dest, &data)?;

    println!("Pushed {} to {}", file.display(), dest.display());
    println!("SHA256: {hex_digest}");

    Ok(())
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Fills in the identity fields of a parsed unit.
///
/// The parser creates units with nil UUIDs and placeholder timestamps.
/// This function replaces those with:
/// - A new random `UnitId` (M5: content-addressed hashing is future work)
/// - The local author's `AuthorId` (derived from public key)
/// - The local trust domain
/// - A current timestamp (dual clock)
#[allow(clippy::cast_possible_truncation)]
fn fill_identity(unit: Unit, client: &LocalClient) -> Unit {
    let new_id = UnitId(uuid::Uuid::new_v4());
    let author = client.author_id();
    let trust_domain = client.trust_domain();

    let new_header = UnitHeader {
        id: new_id,
        author,
        trust_domain,
        created_at: DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime {
                millis: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64),
            },
            timezone: "UTC".to_string(),
        },
        validity: unit.header().validity.clone(),
        state: UnitState::Declared,
        version: None,
    };

    match unit {
        Unit::Workload(mut w) => {
            w.header = new_header;
            Unit::Workload(w)
        }
        Unit::Data(mut d) => {
            d.header = new_header;
            Unit::Data(d)
        }
        Unit::Policy(mut p) => {
            p.header = new_header;
            p.scope = trust_domain;
            Unit::Policy(p)
        }
        Unit::Governance(g) => {
            // Governance units are more complex — replace header
            // on each variant.
            match g {
                taba_core::GovernanceUnit::TrustDomainDef(mut td) => {
                    td.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::TrustDomainDef(td))
                }
                taba_core::GovernanceUnit::RoleAssignment(mut ra) => {
                    ra.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::RoleAssignment(ra))
                }
                taba_core::GovernanceUnit::Certification(mut c) => {
                    c.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::Certification(c))
                }
                taba_core::GovernanceUnit::OperationalCommand(mut oc) => {
                    oc.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::OperationalCommand(oc))
                }
                taba_core::GovernanceUnit::PromotionGate(mut pg) => {
                    pg.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::PromotionGate(pg))
                }
                taba_core::GovernanceUnit::CrossDomainCapability(mut cdc) => {
                    cdc.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::CrossDomainCapability(cdc))
                }
                taba_core::GovernanceUnit::KeyRevocation(mut kr) => {
                    kr.header = new_header;
                    Unit::Governance(taba_core::GovernanceUnit::KeyRevocation(kr))
                }
            }
        }
    }
}

/// Parses a unit ID from a string (UUID format).
fn parse_unit_id(s: &str) -> Result<UnitId, CliError> {
    uuid::Uuid::parse_str(s)
        .map(UnitId)
        .map_err(|e| CliError::InvalidInput {
            reason: format!("invalid unit ID '{s}': {e}"),
        })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unit_id_valid() {
        let uuid_str = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
        let id = parse_unit_id(uuid_str).expect("valid UUID should parse");
        assert_eq!(id.to_string(), uuid_str);
    }

    #[test]
    fn test_parse_unit_id_invalid() {
        assert!(parse_unit_id("not-a-uuid").is_err());
        assert!(parse_unit_id("").is_err());
    }

    #[tokio::test]
    async fn test_run_push() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let cache = tmp.path().join("cache");
        let file = tmp.path().join("test.txt");
        std::fs::write(&file, b"hello, taba!").expect("write test file");

        run_push(&file, Some(cache.clone())).expect("push should succeed");

        // The cache should contain one file.
        let entries: Vec<_> = std::fs::read_dir(&cache).expect("read cache dir").collect();
        assert_eq!(entries.len(), 1, "cache should contain one file");
    }

    #[tokio::test]
    async fn test_run_apply_dry_run() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");

        let toml_file = tmp.path().join("unit.taba.toml");
        std::fs::write(
            &toml_file,
            r#"[unit]
name = "hello-web"
image = "hello:latest"
"#,
        )
        .expect("write toml");

        let result = run_apply(Some(state), &toml_file, true).await;
        assert!(result.is_ok(), "dry run should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_run_apply_insert() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");

        let toml_file = tmp.path().join("unit.taba.toml");
        std::fs::write(
            &toml_file,
            r#"[unit]
name = "hello-web"
image = "hello:latest"
"#,
        )
        .expect("write toml");

        let result = run_apply(Some(state.clone()), &toml_file, false).await;
        assert!(result.is_ok(), "apply should succeed: {result:?}");

        // Verify the unit is in the graph.
        let client = LocalClient::load_unverified(Some(state.clone()))
            .await
            .expect("load client");
        let units = client.list_units().await.expect("list units");
        assert_eq!(units.len(), 1, "should have 1 unit after apply");
    }

    #[tokio::test]
    async fn test_run_status_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");
        let result = run_status(Some(state), OutputFormat::Table).await;
        assert!(result.is_ok(), "status should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_run_unit_list_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");
        let result = run_unit_list(Some(state), OutputFormat::Table).await;
        assert!(result.is_ok(), "unit list should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_run_compose_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");
        let result = run_compose(Some(state), OutputFormat::Table).await;
        assert!(result.is_ok(), "compose should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_run_init_and_reinit() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");

        // First init should succeed.
        run_init(Some(state.clone()), false).expect("first init");

        // Second init without --force should fail.
        let result = run_init(Some(state.clone()), false);
        assert!(result.is_err(), "re-init without --force should fail");

        // Re-init with --force should succeed.
        let result = run_init(Some(state), true);
        assert!(result.is_ok(), "re-init with --force should succeed");
    }

    #[tokio::test]
    async fn test_run_audit_trails_empty() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("state");
        let result = run_audit_trails(Some(state), OutputFormat::Table).await;
        assert!(result.is_ok(), "audit trails should succeed: {result:?}");
    }
}
