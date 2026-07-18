# Contributing

Use small changes with tests and an ADR for significant dependencies or architecture. Do not expand product scope silently or add a crate without a demonstrated ownership boundary.

Before submitting a change:

```bash
./scripts/check.sh
```

Commits should explain intent. Source content used by tests must be synthetic and safe to publish. See [development guidance](docs/development.md) and the [threat model](docs/threat-model.md).

