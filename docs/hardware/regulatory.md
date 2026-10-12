# Regulatory checklist per region/profile

A `RadioProfile` ships with `tx_allowed: false` until every item below is
resolved and documented for that band. This gate is deliberate: UCR is a
protocol, not a licence to transmit.

## Per-profile requirements

- [ ] Band plan confirmed (frequency, occupied bandwidth vs channel plan).
- [ ] Power limit confirmed (EIRP, antenna assumptions, PA clamp value).
- [ ] Duty-cycle or dwell-time limit encoded (`duty_cycle_permille`,
      polite spectrum access / LBT-AFA where required).
- [ ] Listen-before-talk requirement checked (e.g. ETSI EN 300 220
      polite spectrum access vs duty-cycle alternative).
- [ ] Encryption permissible in the band/service (some licence-free
      services restrict obscuring of transmissions — e.g. amateur rules
      do NOT apply here since UCR uses ISM, but verify per region).
- [ ] Identification requirements — if any — mapped to callsign/beacon
      behaviour.
- [ ] Labelling/user-information obligations documented.

## Profiles table

| Profile    | Band      | Status   | Notes |
|------------|-----------|----------|-------|
| SIM        | n/a       | allowed  | simulator only |
| AU ISM 915 | 915–928   | RX-only  | ACMA LIPD class licence review needed |
| EU ISM 868 | 868.0–868.6 | RX-only | ETSI duty-cycle (1%) + polite access |
| US ISM 915 | 902–928   | RX-only  | FCC Part 15 — FHSS or DTS rules apply; single-channel LoRa needs care |

> Until a profile's row reads "allowed", nodes must not transmit on it.
> Receive-only operation and the simulator are unrestricted.

## Contributing

If you can authoritatively complete a row (links to legislation, band
plans, measurement notes), open a PR editing this file and flipping
`tx_allowed` for that profile in `crates/ucr-protocol/src/profile.rs`.
