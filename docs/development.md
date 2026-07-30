# Development

## Toolchain

Rust edition 2024, MSRV 1.85 (provisional). Run:

```bash
./scripts/check.sh
```

## Local operational smoke test

```bash
TMP=$(mktemp -d)
cargo run -p omnisem-cli -- --data-root "$TMP" init
mkdir -p "$TMP/corpus"
printf '# Note\n\nHello.\n' > "$TMP/corpus/note.md"
cargo run -p omnisem-cli -- --data-root "$TMP" root add "$TMP/corpus" --name corpus
cargo run -p omnisem-cli -- --data-root "$TMP" index
cargo run -p omnisem-cli -- --data-root "$TMP" status
cargo run -p omnisem-cli -- --data-root "$TMP" changes
```

Never use private user documents as fixtures.

## Exit codes

See [cli.md](cli.md). Map typed core errors at the CLI boundary; library code returns `Result` values.
