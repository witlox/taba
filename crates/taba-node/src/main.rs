//! taba-node — node daemon binary.
//!
//! The continuous reconciliation daemon is implemented as
//! `taba daemon` in the taba-cli crate (to avoid circular
//! dependencies). This binary is reserved for future
//! multi-node functionality (gRPC server, cluster membership).

fn main() {
    eprintln!("taba-node: multi-node daemon (not yet implemented)");
    eprintln!("For single-node reconciliation, use: taba daemon");
    std::process::exit(1);
}
