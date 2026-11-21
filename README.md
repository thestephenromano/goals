# Goals

One home for your goals: anything you can reach. You state what done looks
like and why it matters, log progress in seconds, and see honestly whether
each goal is on track.

Progress is measured in one of three ways:

- **Milestones:** ordered checkpoints, each reached or not, each with an
  optional date.
- **A number:** a total that check-ins add up (run 500 km, save $10,000),
  or a level where the latest check-in is the current value (weigh 75 kg).
- **Done or not:** for goals you would rather not break down.

Goals holds outcomes and evidence of progress. The work that gets you there
belongs in a task manager; [Matters](https://github.com/thestephenromano/matters)
projects can say which goal they serve.

## Status

Under construction. The MVP's scope is
[thestephenromano/matters#223](https://github.com/thestephenromano/matters/issues/223).
Goals is also the second application for the Matters sync service
([#46](https://github.com/thestephenromano/matters/issues/46)): it syncs end to
end encrypted through the public sync client and the documented service
contract, the way any other application would.

## Layout

- `crates/goals-core`: goals, measures, check-ins, reviews, the pace rules
  and SQLite storage. No user interface.

## Licence

MIT OR Apache-2.0, at your option.
