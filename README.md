# cw-trace

CDK-aware CloudWatch log + X-Ray trace debugger.

**Thesis:** the debugger that reads your `cdk.out` so you never hunt for a log group again. Point it at a CDK project, navigate your construct tree, jump from a log line to its distributed trace and back.

See [`docs/PRD.md`](docs/PRD.md) for the full product requirements.

## Status

M2 (CLI logs + traces). `graph`, `logs <ConstructPath>`, and `trace <id>` work from the terminal; the Tauri + Svelte desktop app lands in M3.

## Layout

```
crates/cdk-graph/   # cdk.out (tree.json + manifest + templates) → resource graph
crates/synth/       # locate cdk.json, run `cdk synth`, find cdk.out
crates/aws-obs/     # CloudWatch Logs + X-Ray access (M2)
crates/correlate/   # log ↔ trace ID extraction (no AWS calls)
crates/cli/         # `cw-trace` binary (thin CLI over the core)
app/                # Tauri backend + Svelte frontend (M3, not yet scaffolded)
docs/PRD.md         # product requirements
```

## Develop

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cw-trace-cli -- status
cargo run -p cw-trace-cli -- graph --cdk-out crates/cdk-graph/tests/fixtures/sample-cdk-out
# Needs AWS credentials (standard chain: profiles/SSO, --profile/--region):
cargo run -p cw-trace-cli -- logs SampleStack/CreateOrder --cdk-out crates/cdk-graph/tests/fixtures/sample-cdk-out -b3h --limit 50
cargo run -p cw-trace-cli -- trace 1-5f5e1000-abcdef1234567890abcdef12 --cdk-out crates/cdk-graph/tests/fixtures/sample-cdk-out
```

Prerequisites: stable Rust (edition 2024). M2+ needs AWS credentials (profiles/SSO via the standard chain). M3 needs Node.js for the Svelte frontend.

## License

Apache-2.0
