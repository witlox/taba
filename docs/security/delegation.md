# Delegation Tokens

When a service is placed on a node, the author pre-signs a
**delegation token**. The node uses this token to sign spawned
bounded tasks on behalf of the author. The node never holds the
author's private key.

## INV-W4: Delegation model

- Author signs a token binding: `service_id`, `node_id`,
  `trust_domain`, `LC range`, `max_spawns`
- Node validates the token before each spawn
- Spawn count is tracked and checked against `max_spawns`
- Token can be revoked

## INV-W4a: Governance block

Delegation grants **operational authority only**. Spawned tasks
**cannot**:
- Create policy units
- Create governance units
- Participate in multi-party declassification

The `DelegationValidator::check_governance_block` method
enforces this at spawn time.

## Token lifecycle

```
create_token → use (spawn) → revoke
                 ↓
            expired (LC range exceeded)
            exhausted (spawn count exceeded)
            revoked (explicit)
```
