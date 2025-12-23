# Founder Story — Why I Built Vaultless

## Where it started

I didn't start with encryption or attestation. I started with a light switch.

I was experimenting with embedded systems — building a distribution board I could control remotely. The first version worked over Bluetooth. Then I wanted internet control.

That's when I found Blynk and Virtuino. Blynk almost solved the problem, but I wanted something I could customize — something I could build my own application around. If you've used Blynk, you understand the friction.

Eventually, controlling a distribution board felt too small. I moved on to building a pump monitoring system — a basic frontend in HTML, CSS, and JavaScript that could send commands and receive data from a remote node.

But that meant I needed a backend. Something to relay messages between the client and the device. I didn't have the resources or the knowledge to build that infrastructure at the time.

So I searched.

I looked for a platform that would let me send messages between clients without hosting my own server. Something lightweight. Something I could trust with command-and-control traffic for physical systems.

I couldn't find it.

Maybe my research wasn't deep enough. But nothing fit. Everything was either too opinionated, too expensive at scale, or too willing to see my payloads.

That's when the idea took shape:

*What if I built this myself — and made it available to anyone facing the same problem?*

Not just a relay. A platform where messages are encrypted by default. Where clients prove themselves before participating. Where developers control costs, not the other way around.

---

## The deeper problem

Like most developers, I started with the usual real-time tools. They worked — until they didn't. The moment the system became important, the trade-offs became impossible to ignore.

Messages were opaque until they weren't.
Costs were predictable until traffic mattered.
Security existed — but only as configuration, not enforcement.

I realized something uncomfortable:

The platforms I depended on could see everything — and I had no way to prove otherwise.

That wasn't a bug.
That was the model.

---

## The breaking point

I was building systems where:

- Messages carried sensitive meaning
- Devices weren't always online
- Clients couldn't be fully trusted
- Costs needed to be bounded — always

And yet the infrastructure assumed:

- Payload access was acceptable
- Usage would be audited after the fact
- Trust could be implied instead of verified
- Client devices were inherently legitimate

I didn't want a platform that promised security.

I wanted one that couldn't violate it even if it tried.

---

## What didn't exist

There was no real-time platform that:

- Enforced end-to-end encryption by design
- Allowed routing without payload visibility
- Verified that clients were running on legitimate, uncompromised devices
- Provided cryptographic proof of message integrity
- Let developers set hard limits — and trust they'd hold

Everything was optimized for scale.
Nothing was optimized for trust.

---

## So I built Vaultless

Vaultless started with a simple question:

*What if every participant had to prove itself before being trusted?*

From that came everything else.

Messages are encrypted with AES-256-GCM — always. The server routes ciphertext, never plaintext.

Every message can carry an Ed25519 signature. Recipients can verify sender identity cryptographically, not by policy.

Usage is metered in real-time. Costs stop when limits are reached — no surprises, no overages.

Applications are isolated by default. Each has its own keys, quotas, and security policies.

And critically:

Clients must attest to their platform integrity before participating.

iOS devices prove themselves through Apple App Attest. Android through Play Integrity. Browsers through origin binding and rate limiting. IoT through certificate validation.

If a client can't prove it's running on a legitimate device, it doesn't belong.

---

## Client attestation changes everything

Most platforms ask you to trust your clients.

Vaultless requires them to prove themselves.

Every participating client:

- Verifies its device integrity
- Proves it hasn't been tampered with
- Operates within enforced constraints
- Can be locked out after repeated attestation failures

Trust isn't a policy toggle.
It's a cryptographic prerequisite.

---

## What Vaultless is — and isn't

Vaultless isn't a chat platform.
It isn't a UI toolkit.
It isn't built for vanity metrics or engagement graphs.

It's infrastructure for systems where:

- Compromise isn't acceptable
- Overages aren't tolerable
- "We didn't know" isn't an excuse
- Clients can't be blindly trusted

If your system matters, the infrastructure should act like it does.

---

## Why it's priced differently

I didn't want a platform that profits from unpredictability.

So Vaultless charges for:

- Guarantees
- Isolation
- Attestation
- Enforcement

Not for chaos.

You decide your limits.
Vaultless enforces them.
That's the contract.

---

## The promise

Vaultless will never:

- Read your payloads
- Sell insight derived from your traffic
- Bill you after the damage is done
- Treat trust as a feature toggle
- Let unverified clients into your application

If we can't prove what we're doing, we won't do it.

---

## Why I'm still building this

Because infrastructure should be:

- Verifiable
- Predictable
- Accountable

And because trust should never be implicit — not for the platform, and not for the clients using it.

---

Vaultless exists for developers who refuse to build on blind trust — from either side.
