# UCR Security Model

## Goals

- **Authenticity** — every originator packet is attributable to a
  self-certifying identity; forged traffic is dropped, never relayed.
- **Confidentiality** — DMs and group traffic are end-to-end encrypted;
  relays see only routing metadata.
- **No trusted infrastructure** — no servers, no directory, no CA.
  Identity is a hash of a public key.

## Non-goals (explicit)

- **Metadata privacy.** A mesh floods routing info by design; observers
  learn *that* nodes talk, timing, and rough density — though not *what*.
  Padding/onion routing is a future research item.
- **Physical-layer jamming resistance** beyond standard spread-spectrum
  properties of the chosen profile.
- **Anonymity.** NodeIds are persistent pseudonyms. Rotating identities
  are possible but break contacts/verification.

## Identity

`NodeId = BLAKE3(ed25519_pubkey)[..16]`. Any party holding `pubkey` can
prove it owns `node_id` (`check_binding`). Beacons and handshakes carry
the pubkey; unknown senders are queried via `KeyRequest`/`KeyReply` and
the reply is bound-checked before use.

128-bit ids: ~2^64 work to find a colliding keypair — adequate for
community scale, and the full pubkey is always verified anyway.

## Packet authenticity

Ed25519 signatures cover immutable end-to-end fields + payload. Hop
fields (`relay`, `next_hop`, `ttl`, `hops`) are unsigned — mutating them
is normal relaying, not forgery, and cannot alter payload or endpoints.

Signature verification outcomes: `Verified`, `Unverified` (key unknown →
KeyRequest path), `Unsigned`, `Invalid`. Unverified traffic is delivered
to the app *marked* but is never relayed where policy requires auth —
and never accepted for session establishment.

## Sessions (DMs)

Ephemeral X25519 → forward secrecy: compromising a long-term key does
not decrypt recorded traffic. ChaCha20-Poly1305 AEAD with `session_id`
as associated data.

Handshake binding: `SessionInit`/`SessionAccept` ride in *signed*
packets, so the ephemeral keys are anchored to the long-term identities.
The derived `session_id` is checked on completion — a mismatched or
stale accept can never install a session.

Handshake availability: inits are retransmitted identically on a paced
schedule (`timestamp_ms` is bound into derivation — mutations would
break legitimate accepts).

## Groups

Symmetric key per group, distributed to invitees inside their E2E
session, rotated (`epoch++`) when a member is removed. Membership is
the founder's signed invite + the member's signed accept.

## Emergency traffic

Signed, priority-queued, flooded within a wide scope. Origin rate-limited
per node (`emergency_cooldown_ms`). Receivers relay emergencies
regardless of sender-block state — but *display* them only with
signature verification status attached. A forged emergency is possible
only with a stolen key, and is attributable.

## Store-and-forward exposure

Relays hold ciphertext at rest for the queue TTL. This is traffic-
analysis-visible and storage-bound; payloads remain encrypted. Cleartext
app data (channel text, morse) is public by design — relay-held copies
grant no extra authority.

## Implementation notes / known gaps

- `ucr-storage` keeps group keys and identity secrets unencrypted in the
  local redb file — filesystem protection is assumed; an encrypted store
  is roadmaped.
- No replay protection within a session beyond `(src, packet_id)` dedup
  and the handshake window — nonce counters are per-session fresh.
- Blocking is local-only and enforced at the app boundary, not RF.
- Rate limits are per-node policy, not consensus — a malicious node can
  flood; spectrum rules and neighbour-level penalties (future) apply.
- The `serial`/`tcp` transports carry plaintext *frames* — wire
  confidentiality lives at the UCR payload layer by design.
