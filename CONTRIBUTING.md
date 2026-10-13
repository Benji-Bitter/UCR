# Contributing to UCR

UCR is an open protocol project — contributions of code, documentation,
regulatory research, field-test reports, and protocol review are all
welcome.

## Getting started

```bash
cargo build --workspace
cargo test --workspace                    # full suite (~3 min; mesh tests dominate)
cargo test -p ucr-core --test mesh        # just the e2e mesh suite
cargo test -p ucr-crypto                  # fast crypto unit tests
cargo run -p ucr-cli --bin ucr-sim -- run --nodes 5 --duration 90
```

Requires stable Rust (1.93+ verified). `cargo fmt --all` before
committing; `cargo clippy --workspace` should stay clean.

## Where things live

- Wire format & spec: `crates/ucr-protocol` (`no_std` — firmware shares it)
- Routing decisions: `crates/ucr-routing` (pure: returns `RouteAction`s)
- Crypto: `crates/ucr-identity`, `crates/ucr-crypto`
- Node runtime/app API: `crates/ucr-core`
- RF medium: `crates/ucr-simulator`
- Docs: `docs/` — keep in sync with behaviour changes
- Radio firmware: `firmware/ucr-modem` (ESP32-S3 + SX1262) — the bring-up
  walkthrough and modem contract are in `docs/hardware/first-node.md`.
  First-on-air validation, board pin maps (Heltec, T-Beam), and ports to
  other radios are all high-value contributions.

1. **The simulator is the testbed.** New protocol behaviour gets a
   `crates/ucr-core/tests/mesh.rs` scenario. Don't weaken the medium's
   realism to make a test pass — tune pacing or add retries instead.
2. **Signatures stay end-to-end.** Never sign hop-mutable fields
   (`relay`, `next_hop`, `ttl`, `hops`) or strip originator signatures
   when relaying unmutated payloads.
3. **Lock discipline** (`ucr-core`): never call a method that locks `X`
   while holding `X`'s guard; never evaluate a second `.lock()` while a
   scrutinee/guard temporary is alive. See
   `docs/architecture/overview.md#lock-discipline`.
4. **`tx_allowed` stays honest.** Don't flip a radio profile's transmit
   gate without the corresponding
   `docs/hardware/regulatory.md` checklist being completed.
5. Keep `ucr-protocol` `no_std`-clean — no `std` imports, no
   allocations outside `alloc`.

## Pull requests

- Describe the scenario your change affects (topology, timing, loss).
- Include sim evidence: `ucr-sim run` output or a mesh test name.
- Keep commits scoped; protocol-visible changes need a doc update.
- Sign-off: Apache-2.0 licence applies to all contributions.
