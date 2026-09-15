# Operational Modes

taba nodes operate in one of three modes:

## Normal

All operations permitted: authoring, composition, placement,
archive, compaction. This is the default state.

## Degraded

Entered when a trigger condition is met:
- **Erasure threshold exceeded** (INV-R4): too many node failures
- **Memory limit exceeded** (INV-R6): graph exceeds configured limit
- **WAL failure** (FM-07): disk full, corruption, I/O error
- **Operator-triggered**: manual `taba node degraded`

In Degraded mode:
- ❌ Authoring frozen (no new units)
- ❌ Composition frozen (no new placements)
- ❌ Placement frozen
- ✅ Drain and evacuation only
- ✅ Gossip continues (membership updates, health)

Transitions to Normal: **operator override only** (not automatic).

## Recovery

Entered from Degraded when the trigger condition is resolved:
- Erasure re-coding completes
- Compaction frees enough memory
- WAL is repaired or rotated
- Operator clears the degraded state

In Recovery mode:
- ✅ Placement throttled (re-building capacity)
- ✅ Re-coding in progress
- ❌ Full-speed operations not yet resumed

Transitions to Normal: **automatic** when recovery completes.

## State machine

```
Normal → Degraded (any trigger)
Degraded → Recovery (trigger resolved)
Recovery → Normal (recovery complete)
Degraded → Normal (operator override only)
```

Invalid transitions return `NodeError::InvalidModeTransition`.

## Monitoring

```sh
taba status    # Shows current operational mode
```

The health reporter (`HealthReporter`) tracks:
- Units running vs failed
- WAL size in bytes
- Graph memory vs limit
- Memory pressure percentage (0-100)

Compaction triggers at 80% of memory limit (INV-R6).
Degraded mode triggers at 100%.
