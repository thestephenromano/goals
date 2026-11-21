# Sync integration effort

Goals is the second application for the Matters sync service
([thestephenromano/matters#46](https://github.com/thestephenromano/matters/issues/46)).
That issue's cost model needs the incremental effort of integrating an
application from a different domain, so it is recorded here from the first
commit.

## What is recorded

For every merged change:

- **Sync-specific lines:** code in this repository that exists only to sync
  (store adapter, enrollment and recovery screens, settings), separately from
  the application's own code.
- **Changes needed upstream:** anything the Matters repository had to change
  for this app (the generic sync layer extracted from `matters-core`, client
  or contract changes). #46 requires none to be specific to this app or to
  the relay.
- **Friction:** where the public client, the service contract or the docs
  were not enough on their own.

Resource use per collection (encrypted records, history size, retained
checkpoints, bandwidth) is measured once sync runs, with named hosts and
workloads.

## Log

| Change | Application lines (src + tests) | Sync-specific lines | Upstream changes | Notes |
|---|---|---|---|---|
| goals-core: goals, measures, check-ins, reviews, pace, SQLite storage | 2,525 | 0 | none | Built sync-ready: UUID identities, append-mostly check-ins, a milestone's reached state derived from check-ins rather than stored twice. |

## Known work before sync

The sync client (`matters-sync-client`) moves sealed records and controls;
the device side it drives (`LocalStore`: sealing, receiving in chain order,
membership, enrollment, recovery) is implemented inside `matters-core`
today, tied to Matters' database. Extracting it into a shared crate is the
first upstream change this app needs, and is planned after the Matters
migration and checkpoint work (thestephenromano/matters#243 and #245) lands.
