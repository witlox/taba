Run end-to-end tests. Use after integration is functional.

Prerequisites:
- taba binary built and runnable
- At least 3 peer nodes available (or simulated via localhost ports)

Steps:

1. Check binary exists:
   - `cargo build --release --workspace`

2. Start test cluster (3 peer nodes):
   - Use `tests/e2e/helpers/cluster.rs` or test harness if available

3. Run e2e tests:
   - `cargo test --test e2e -- --nocapture` (or equivalent)
   - Or: `cargo nextest run -E 'test(e2e)'`

4. Validate core flows:
   - Unit composition → solver → placement → reconciliation
   - Gossip membership: join → propagate → converge
   - CRDT merge: concurrent edits → convergence on all peers
   - Security: capability request → grant → enforce → revoke
   - Data lineage: declare → consume → provenance verifiable
   - Failure: node drop → erasure reconstruct → re-place

5. Report:
   - Pass/fail per test
   - CRDT convergence verified (all peers agree)
   - Security invariant violations
   - Cluster logs for failed tests

If ANY e2e test fails, investigate before declaring integration complete.
