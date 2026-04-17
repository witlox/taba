Project status assessment. Run at the start of any session.

1. Read `specs/architecture/module-map.md` — identify crate boundaries
2. Check for source code: `ls crates/*/src/lib.rs 2>/dev/null` — which crates exist?
3. Check test state: `cargo test --no-run 2>&1 | tail -5` — does it compile?
4. Check fidelity index: `cat specs/fidelity/INDEX.md 2>/dev/null` — has auditor run?
5. Check open escalations: `ls specs/escalations/*.md 2>/dev/null`
6. Check adversary findings: `cat specs/findings/INDEX.md 2>/dev/null`
7. Check git status: uncommitted changes? Ahead/behind remote?

Report:
- Crates implemented / total
- Test status (compiles? passes? coverage?)
- Fidelity status (sweep done? confidence levels?)
- Open escalations
- Open adversary findings by severity
- Recommended next action
