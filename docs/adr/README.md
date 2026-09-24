# Architecture decisions

The decisions that shaped SchemGen2 and are expensive to reverse, each with
the problem it answered, what was chosen, what it costs and what was turned
down. The roadmap ([roadmap.md](../roadmap.md)) planned them; these records
say what was actually done.

| # | Decision | Status |
|---|---|---|
| [1](0001-python-removal.md) | Voxelize and sample colors in Rust, not Python | Accepted, 2.1.0 |
| [2](0002-api-v2.md) | API v2: a schema, jobs and events, one settings object | Accepted, 2.1.0 |

A new record takes the next number and the same sections — context, decision,
consequences, alternatives considered — and is not rewritten afterwards: when
a decision is replaced, a new record says so and the old one's status points
to it.
