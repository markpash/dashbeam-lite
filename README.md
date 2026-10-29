<div align="center">

# File transfer doesn't need to be complicated

</div>


![DashBeam Header](assets/header.png)

<div align="center">

![DashBeam working demo](assets/demo.gif)

</div>

<div align="center">


[![Discord][badge-discord]](https://discord.gg/xwb7z22Eve)
[![Version][badge-version]](https://github.com/tonyantony300/dashbeam/releases/latest)
![Website][badge-website]
![Platforms][badge-platforms]
[![Sponsor][badge-sponsor]](https://github.com/sponsors/tonyantony300)



</div>

A free and open-source file transfer tool that harnesses the power of cutting-edge peer-to-peer networking, letting you transfer files directly without storing them on cloud servers.

Why rely on WeTransfer, Dropbox, or Google Drive when you can reliably and easily transfer files directly, end-to-end encrypted and without revealing any personal information?

<div align="center">

<br />

<a href="https://www.testmuai.com" rel="nofollow"><picture><source media="(prefers-color-scheme: dark)" srcset="https://www.dashbeam.net/assets/sponsors/testmu-light.svg"><source media="(prefers-color-scheme: light)" srcset="https://www.dashbeam.net/assets/sponsors/testmu-dark.svg"><img src="https://www.dashbeam.net/assets/sponsors/testmu-dark.svg" height="80" alt="TestMuAI"></picture></a>
&emsp;&emsp;
<a href="https://vercel.com/open-source-program"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/vercel-oss-light.svg"><source media="(prefers-color-scheme: light)" srcset="assets/vercel-oss-dark.svg"><img src="assets/vercel-oss-dark.svg" width="254" height="80" alt="Vercel OSS Program"></picture></a>

</div>


## Features

- **Native macOS app** - Built with [GPUI](https://www.gpui.rs) via gpui-kit; no web view, no Electron
- **Any file, any size** - Files or folders, BLAKE3-verified
- **Multi-gigabit speeds** - Saturates fast connections
- **Nearby** - LAN auto-discovery; Pair & Send
- **Paired devices** - Pair once with a code (remote) or Nearby; send without tickets
- **Trusted devices** - Opt a paired device into auto-accept; its files land without a prompt
- **Resumable & multi-peer** - Resume interrupted transfers; share to many at once
- **Dial by key** - Connect by device identity, not IP address
- **Roaming** - Switch Wi-Fi, cellular, or networks mid-transfer without dropping
- **Tickets** - Copy a ticket, paste it on the other side
- **History** - Local transfer log
- **Private** - No accounts, tracking, or ads; direct device-to-device
- **Encrypted** - TLS 1.3 end-to-end; authenticated tickets
- **Self-host relays** - Unthrottled remote transfers via your own relay (Settings → Relay servers)
- **Always reachable** - Menu bar icon, notifications, start at login
- **Lightweight & free** - Small installs; open source, no limits



## Real-world stats


| Metric | Reported |
|--------|--------|
| **Largest transfer** | 452 GB |
| **Fastest large transfer** | 54 GB @ 123 MB/s (~1 Gbps) |
| **High-speed bulk transfer** | 328 GB @ 93 MB/s |
| **Peak speed measured** | 125 MB/s (1 Gbps) |

*Transfer throughput depends on your device, network, and connection path.*



## Installation

This fork is macOS-only (13+) and is built from source with a stable Rust toolchain:

```sh
./scripts/bundle-macos.sh
open target/release/DashBeam.app
```

It interoperates with other DashBeam apps (desktop, Android, CLI) through the same tickets, pairing, and Nearby protocol.

Running into problems? See [Troubleshooting](docs/troubleshooting.md) for common issues and how to collect logs.


## Ways to send
### Nearby devices (Recommended)

Same-network apps appear under **Nearby** on the **Devices** tab and in the **Send to a device** list while you're sharing. Pick a nearby device and press **Send** to pair and start the transfer in one step - you can also pair from the Devices tab without sharing. First contact shows a verification code on both screens. Set who can find you in **Settings**.

Uses [mDNS](https://en.wikipedia.org/wiki/Multicast_DNS) - often blocked on guest Wi-Fi and VPNs. See [Troubleshooting](docs/troubleshooting.md#the-nearby-list-is-empty) if the list stays empty.

### Paired devices

Pair on the **Devices** tab with a pairing code (works remotely, over the internet) or via a Nearby request on the same network. After pairing, **Send** appears next to that device while sharing - no ticket to copy. The receiver gets a prompt, or nothing at all if it turned on **Auto-accept** for you. Closing the window keeps DashBeam running in the menu bar. Manual tickets and the [sendme CLI](https://www.iroh.computer/sendme) still work.

### Share ticket (One time transfer)

While sharing, **Copy ticket** and send it however you like; paste it into **Receive** on the other side. Receive links (`…?ticket=…`) from other DashBeam apps work too. You can also start sharing from a terminal with `DashBeam.app/Contents/MacOS/DashBeam <files…>`.


## Comparison

| | **DashBeam** | **Blip** | **LocalSend** | **Magic Wormhole** | **PairDrop** |
|:---|:---:|:---:|:---:|:---:|:---:|
| Networking | QUIC via Iroh | Unknown | HTTPS/REST over TCP | encrypted TCP | WebRTC/DTLS (SCTP) |
| Works over the internet | ✅ | ✅ | LAN only | ✅ | ✅ |
| Saturates gigabit connections | ✅ | ✅ | LAN only | ✅ | ❌ (SCTP/browser ceiling) |
| Discover devices on LAN | ✅ | ❌ | ✅ | ❌ | ❌ |
| Persistent paired devices | ✅  | ✅ (account) | ❌ | ❌ | ✅ |
| Send folders | ✅ | ✅ | ✅ | ✅ | ✅ (CLI only, not in browser) |
| Resumable transfers | ✅ | ✅ | ❌ | ❌ | ❌ |
| Roaming mid-transfer | ✅ | ❌ | ❌ | ❌ | ❌ |
| Send to multiple receivers | ✅ (broadcast) | ❌ | ✅ | ✅ | ❌ |
| Unlimited file size | ✅ | ✅ | ✅ | ✅ | Limited by browser memory |
| Open source | ✅ | ❌ | ✅ | ✅ | ✅ |
| No account required | ✅ | ❌ | ✅ | ✅ | ✅ |
| End-to-end encryption | ✅ | ✅ | ✅ | ✅ | ✅ |
| Platforms | CLI + desktop + mobile + web | Desktop + mobile (no web/CLI) | Desktop + mobile (no web/CLI) | CLI only | Web/PWA + Android app + CLI |
| The catch | WIP | Closed source; data handling cannot be audited | Same-network only, no resume | CLI-only; GUI front-ends are separate, community-maintained | WebRTC/SCTP throughput ceiling; browser memory limits |

[Know more →](https://www.dashbeam.net/en/compare)

## Under the hood

DashBeam is built on [Iroh](https://www.iroh.computer), a modern peer-to-peer networking stack that simplifies direct device-to-device communication. In practice, that means devices talk over encrypted QUIC, files move with content-addressed blobs, and relays help when a direct path isn’t available.

> **The long version:** [Under the hood](https://dashbeam.net/en/under-the-hood) walks the same architecture as a narrative - fingerprints instead of filenames, names instead of addresses, hole punching, roaming, and what each intermediary can and cannot see.

### The building blocks

| Piece | What it does here |
|-------|-------------------|
| **Blobs** (`iroh-blobs`) | Store and stream file data; every chunk is verified with BLAKE3 |
| **Tickets** | One string that tells a peer *who* to dial and *what* to fetch |
| **Endpoints** | Each device’s Iroh identity (Ed25519 key → endpoint id) |
| **QUIC + TLS 1.3** | Encrypted transport; multiplexing without head-of-line blocking |
| **Relays + hole punching** | Bootstrap connections across NATs; the relay carries data while a direct path is negotiated |
| **Control protocol** (pairing) | Long-lived channel to remember devices and deliver share invites |
| **Local discovery** (mDNS) | Optional LAN advertising so Nearby devices can find each other without a ticket |
| **Receive links** | A ticket wrapped in an ordinary https URL - an App Link on Android, the web app everywhere else |

### Blobs

Files aren’t uploaded to a server. They’re published as **blobs**: opaque byte sequences addressed by a BLAKE3 hash.

- A **link** is that 32-byte hash: if the hash matches, the content matches.
- Folders and large files use a **HashSeq** (a blob that points at other blobs).
- The sender is the **provider**; the receiver is the **requester**. Either side can do both.

Because that hash is a BLAKE3 tree rather than one digest over the whole file, every chunk verifies on its own against the root. That is what makes resume cheap: a receiver coming back after a dropped connection knows exactly which chunks it already holds *and has verified*, so it asks only for the gap. Nothing already received is re-sent, and nothing arriving is taken on trust.

### Tickets

A share **ticket** is a single token that packs:

1. The sender’s endpoint id (so you know you’re talking to the right device)
2. Enough address / relay info to dial them
3. The blob hash to download

You only connect to people you share a ticket with: no broadcasting your IP to strangers. That’s the default “cozy network” model Iroh encourages, vs. flooding discovery to the whole swarm.

Tickets that carry only an endpoint id lean on public-key discovery (Pkarr) to find current addresses. Those records are signed by the endpoint key itself, so a tampered one fails verification rather than misdirecting you - the lookup service is trusted for **availability, not integrity**.


### Connecting across networks

When two devices need to meet:

1. Each registers with a public (or self-hosted) **relay** so peers can find a path through firewalls and NATs.
2. Iroh tries **QUIC hole punching** to upgrade to a direct peer-to-peer link.
3. If a direct path works, traffic moves over to it. If not, the relay stays in the path as a fallback UDP hop.

Those steps overlap rather than queue. The relay is already carrying your data while hole punching negotiates, so nothing waits on the direct path to succeed - the transfer just gets faster if and when it does. Roughly one transfer in ten never gets a direct path at all (symmetric NAT, locked-down corporate networks) and rides the relay start to finish.

Either way, the payload is end-to-end encrypted. Relays see ciphertext, not your files. [More on Iroh relays →](https://docs.iroh.computer/about/faq)

### Roaming across networks

A connection is bound to the peer’s key, not to its IP address, so changing networks does not end it. Switch from Wi-Fi to cellular mid-transfer and iroh notices the address change, learns the new candidates, and republishes them to the peer. The relay carries the data throughout, and hole punching simply runs again on the new path.

Addresses are disposable hints, not identity. That is the practical payoff of naming devices by key instead of by location.

### QUIC & encryption

QUIC (UDP-based, same foundation as HTTP/3) brings TLS 1.3 into the transport. For DashBeam that buys encryption and authentication, multiple streams with shared congestion control, and fast reconnects when you’ve talked to a peer before.

### Paired devices

Pairing doesn’t replace tickets; it delivers them for you.

1. Devices exchange a short **pairing code** (the host’s endpoint id) over a dedicated control ALPN.
2. Each side proves identity by signing connection-bound keying material with its device secret, then remembers the peer locally.
3. A persistent control connection keeps presence (online/offline).
4. When you share, DashBeam still creates a normal one-time blob ticket; choosing a paired device ships that ticket as an in-app **invite** instead of making you copy-paste it.

Manual tickets and the [sendme CLI](https://www.iroh.computer/sendme) keep working exactly as before.

### Nearby (local discovery)

On the same local network, DashBeam can advertise and browse peers with mDNS (desktop and Android; not the web app).

1. When discoverability is **Everyone**, the device publishes enough metadata for others to show its name in Nearby.
2. **Paired only** still announces presence without exposing the display name to strangers on the LAN.
3. **Off** stops advertising; you can still browse and send to others who remain discoverable.
4. First-contact file invites show a short verification code derived from both devices' public keys so each side can confirm they're talking to the intended peer before accepting.
5. Accepting a Nearby pair request or file invite creates the same local paired-device records as code-based pairing.

### Self-hosting relays and discovery

For how to run your own iroh relay and discovery server, configure DashBeam to use them, and how mixed public/self-hosted setups behave, see [`infra/README.md`](infra/README.md) (relay: [`infra/relay/README.md`](infra/relay/README.md#using-self-hosted-relays-with-dashbeam), discovery: [`infra/dns/README.md`](infra/dns/README.md)).

For the illustrated version of everything above - including a full account of what a relay operator, your ISP, and the lookup service each learn from a transfer - read [Under the hood](https://dashbeam.net/en/under-the-hood).


## Development

See [CONTRIBUTING.md](CONTRIBUTING.md#development-setup) for prerequisites, local setup, build instructions, and testing.

## Join our [Discord](https://discord.gg/xwb7z22Eve) to contribute

The best way to contribute is to join our Discord and say hi. Introduce yourself and share what skills or interests you have - whether that’s coding, testing, design, or something else. You can also raise issues, suggest fixes, or pitch ideas. Maintainers are there to guide you every step of the way.

It’s the best place to get context, align on direction, and collaborate with the [community](https://discord.gg/xwb7z22Eve).

## License

AGPL-3.0

## Privacy Policy

See [PRIVACY.md](PRIVACY.md) for information about how DashBeam handles your data and privacy.

[![Sponsor](https://img.shields.io/badge/sponsor-30363D?style=for-the-badge&logo=GitHub-Sponsors&logoColor=#EA4AAA)](https://github.com/sponsors/tonyantony300) [![Buy Me Coffee](https://img.shields.io/badge/Buy%20Me%20Coffee-FF5A5F?style=for-the-badge&logo=coffee&logoColor=FFFFFF)](https://buymeacoffee.com/tny_antny)


## Contributors

<a href="https://github.com/tonyantony300/dashbeam/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=tonyantony300/dashbeam" />
</a>


## Contact

Reach me [here](https://www.dashbeam.net/en/contact) for suggestions, feedback or media related communication.


Thank you for checking out this project! If you find it useful, consider giving it a star and helping spread the word.

<br>







<!-- <div align="center" style="color: gray;"></div> -->

[badge-website]: https://img.shields.io/badge/website-dashbeam.net-orange
[badge-version]: https://img.shields.io/badge/version-0.7.2-blue
[badge-discord]: https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white
[badge-platforms]: https://img.shields.io/badge/platform-macOS-green
[badge-sponsor]: https://img.shields.io/badge/sponsor-ff69b4


