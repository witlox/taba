# Workflow Router

Development workflow that applies to the taba project. Project-specific context
lives in the root CLAUDE.md.

Role definitions in `.claude/roles/`. Read the relevant role file when
activating a mode. These are behavioral constraints, not suggestions.

## Standards

Engineering guidelines in `.claude/guidelines/` (general, cross-project):
- `engineering.md` — commits, errors, code org, testing philosophy
- `rust.md` — Rust tooling, style, clippy, cargo-deny
- `ci.md` — CI/CD pipeline structure
- `docs.md` — documentation requirements

Project-specific coding standards in `.claude/coding/`:
- `rust.md` — taba Rust: CRDT, solver, units, gossip, property testing

## Pre-commit discipline

**Before claiming "all tests pass" or committing code, ALWAYS:**

1. Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`
2. Run the relevant `cargo test` commands and **show the output**
3. Never commit based solely on subagent reports — verify in the main context
4. When adding steps/functions to shared namespaces (BDD steps, trait impls), grep for name conflicts first

Use `/project:verify` to run the full checklist.

## Automatic command invocation

| Command | When to invoke automatically |
|---|---|
| `/project:status` | **First message of every new session.** Establishes project state before any work. |
| `/project:verify` | **Before every commit.** Do not commit without running this. If it fails, fix and re-run. |
| `/project:spec-check` | **After completing a feature or design phase.** Validates specs still align with code. Also run after any spec or architecture change. |
| `/project:e2e` | **After integration phase and before declaring integration complete.** Also run after any change that touches cross-context boundaries. |

## Before every response: determine mode

Do not skip. Do not assume from prior context. Evaluate fresh.

### Step 1: Project state

```
1. specs/fidelity/INDEX.md exists with completed checkpoint?
   → Baselined. Step 2.

2. specs/fidelity/SWEEP.md exists, status IN PROGRESS?
   → Sweep underway. Default: SWEEP (resume). Step 2.

3. Source code exists beyond scaffolding?
   → Brownfield, no baseline. Suggest sweep if user hasn't asked
     for something specific. Step 2.

4. Specs/docs exist but source empty/minimal?
   → Greenfield with docs. Step 2.

5. Repo empty or near-empty?
   → Pure greenfield. Step 2.
```

### Step 2: User intent

| Intent | Mode | Role(s) |
| --- | --- | --- |
| "where are we" / "status" | ASSESS | Read fidelity/findings indexes or inventory |
| "sweep" / "baseline" / "full audit" | SWEEP | `.claude/roles/auditor.md` |
| "adversary sweep" / "security review" / "full review" | ADV-SWEEP | `.claude/roles/adversary.md` |
| "audit [X]" | AUDIT | `.claude/roles/auditor.md` |
| "new feature" / "add" / "implement" | FEATURE | See Feature Protocol |
| "fix" / "bug" / "broken" / "error" | BUGFIX | See Bugfix Protocol |
| "design" / "spec" / "think about" | DESIGN | See Design Protocol |
| "review" / "find flaws" / "adversary" | REVIEW | `.claude/roles/adversary.md` |
| "integrate" | INTEGRATE | `.claude/roles/integrator.md` |
| "continue" / "next" | RESUME | Read SWEEP.md / ADVERSARY-SWEEP.md or current state |
| Unclear | ASK | State what you see, ask what they want |

### Step 3: State assessment

Before acting, one line:

```
Mode: [MODE]. Project: [state]. Role: [role]. Reason: [why].
```

If ambiguous, ask.

## In-session role switching

Say "audit this" → auditor. "Implement" → implementer. "Review" → adversary.
On switch: `Switching to [role]. Previous: [role].`
Read `.claude/roles/[role].md` when switching. Apply its constraints.

## Protocols

### Feature Protocol (diverge → converge → diverge → converge)

```
DESIGN: analyst → spec | architect → interfaces | adversary → gate 1
IMPLEMENT: implementer → BDD+code | auditor → gate 2 | harden until HIGH
REVIEW: adversary → findings | INTEGRATE (if cross-feature): integrator
```

Done = scenarios pass + fidelity HIGH + adversary signed off.

### Bugfix Protocol

```
1. DIAGNOSE: reproduce, check fidelity (was this area LOW?)
2. WRITE FAILING TEST FIRST: must fail before fix, pass after
3. FIX: implement, no regressions
4. AUDIT: new test THOROUGH? Deepen adjacent if area was LOW
5. UPDATE INDEX
```

### Design Protocol

```
1. New domain → analyst | 2. Architecture change → architect | 3. ADR → write it
All: adversary reviews before implementation
```

### Sweep Protocols

**Fidelity sweep** (`.claude/roles/auditor.md`): inventory → chunked assessment → checkpoint.
**Adversary sweep** (`.claude/roles/adversary.md`): attack surface → chunked review → findings index.

Run fidelity first when possible — LOW areas get higher adversary priority.

## Greenfield entry (taba's current state)

```
Empty repo → ANALYST → ARCHITECT → ADVERSARY → IMPLEMENT → diamond workflow
```

With docs (current): read `docs/vision/SYSTEM_VISION.md`, determine which
analyst layers are already covered, continue from there.

## Escalation paths

* Implementer → Architect (interface doesn't work)
* Implementer → Analyst (spec ambiguous)
* Adversary → Architect (structural flaw)
* Adversary → Analyst (spec gap)
* Auditor → Implementer (tests shallow)
* Auditor → Architect (mock diverges from trait contract)
* Integrator → Architect (cross-cutting structural issue)

Escalations go to `specs/escalations/`, must resolve before escalating
phase completes.

## Directory conventions

```
.claude/
├── CLAUDE.md          # This file (workflow router)
├── guidelines/        # Cross-project engineering standards
│   ├── engineering.md
│   ├── rust.md
│   ├── ci.md
│   └── docs.md
├── coding/            # Taba-specific coding standards
│   └── rust.md
├── roles/
│   ├── analyst.md
│   ├── architect.md
│   ├── adversary.md
│   ├── implementer.md
│   ├── integrator.md
│   └── auditor.md
└── commands/
    ├── verify.md
    ├── status.md
    ├── spec-check.md
    └── e2e.md

specs/
├── domain-model.md
├── ubiquitous-language.md
├── invariants.md
├── assumptions.md
├── features/*.feature
├── cross-context/
├── failure-modes.md
├── architecture/
│   ├── module-map.md
│   ├── dependency-graph.md
│   ├── interfaces/
│   ├── data-models/
│   ├── events/
│   ├── error-taxonomy.md
│   └── enforcement-map.md
├── fidelity/
│   ├── INDEX.md
│   ├── SWEEP.md
│   └── gaps.md
├── findings/
│   ├── INDEX.md
│   ├── ADVERSARY-SWEEP.md
│   └── [chunk].md
├── integration/
└── escalations/
```
