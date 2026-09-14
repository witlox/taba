//! taba-k8s — K8s manifest reader that generates taba unit declarations.
//!
//! Usage: taba k8s convert <file.yaml> [--output-dir <dir>] [--trust-domain <td>]
//!
//! Reads K8s manifests (YAML, multi-document supported) and generates
//! taba unit declaration files (.taba.toml) for each resource that
//! can be mapped. Unmappable resources are reported.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use taba_k8s::K8sConverter;

#[derive(Parser)]
#[command(
    name = "taba-k8s",
    version,
    about = "K8s manifest → taba unit converter"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Output directory for generated .taba.toml files (default: stdout).
    #[arg(long, global = true)]
    output_dir: Option<PathBuf>,

    /// Trust domain for generated units.
    #[arg(long, global = true)]
    trust_domain: Option<String>,

    /// Generate SLSA provenance placeholders.
    #[arg(long, global = true)]
    provenance: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Convert K8s manifests to taba unit declarations.
    Convert {
        /// Input YAML file (use - for stdin).
        file: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Convert { file } => {
            let yaml = if file == "-" {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                buf
            } else {
                std::fs::read_to_string(&file)?
            };

            let mut converter = K8sConverter::new();
            if let Some(td) = &cli.trust_domain {
                converter = converter.with_trust_domain(td);
            }
            if cli.provenance {
                converter = converter.with_provenance();
            }

            let report = converter.convert(&yaml)?;

            // Write generated units.
            for (name, toml) in &report.generated {
                if let Some(dir) = &cli.output_dir {
                    std::fs::create_dir_all(dir)?;
                    let path = dir.join(format!("{name}.taba.toml"));
                    std::fs::write(&path, toml)?;
                    println!("  ✓ {} → {}", name, path.display());
                } else {
                    println!("=== {name}.taba.toml ===");
                    println!("{toml}");
                }
            }

            // Print report.
            if !report.is_clean() {
                eprintln!();
                eprintln!("{}", report.format());
            }

            if report.unmappable_count() > 0 {
                std::process::exit(1);
            }

            Ok(())
        }
    }
}
