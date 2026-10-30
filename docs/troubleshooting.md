# Troubleshooting

- [I can't connect to the other device](#i-cant-connect-to-the-other-device)
- [A transfer stalls or fails partway](#a-transfer-stalls-or-fails-partway)
- [A paired device shows as Offline](#a-paired-device-shows-as-offline)
- [The Nearby list is empty](#the-nearby-list-is-empty)
- [I don't get notifications](#i-dont-get-notifications)
- [Collecting logs](#collecting-logs)
- [Reporting a bug](#reporting-a-bug)

---

## I can't connect to the other device

**The sender must keep the app open.** Files are served directly from the sending
device; there's no copy on a server. If the sender quits DashBeam Lite, sleeps their Mac, or
loses network, the transfer can't proceed. Closing the window is fine: DashBeam Lite keeps
running in the menu bar.

**Both sides need compatible relay settings** (**Settings → Relay servers**):

- **Disabled** relays only work on your local network.
- If either side uses **Custom** relays, both sides generally need the same relay URLs.
- If a custom relay requires an auth token, it must match `access.shared_token` on the
  server.

**Check the ticket was copied completely.** Tickets are long; a truncated paste fails to
parse.

**Restrictive networks.** Corporate, university, and guest Wi-Fi often block the
peer-to-peer traffic DashBeam Lite relies on. Try both devices on a different network.

---

## A transfer stalls or fails partway

**Speed depends on both devices' connections**, and on whether a direct connection could
be established. When NAT traversal fails, traffic falls back to an encrypted relay, which
is slower.

**Don't let either machine sleep** mid-transfer.

**Check free disk space** and that the download folder (**Settings → Download folder**)
still exists and is writable.

---

## A paired device shows as Offline

- **Offline.** The other device isn't running DashBeam, or can't be reached right now.
  It stays paired; send a ticket in the meantime.
- **Re-pair required / Unpaired.** Forget it and pair again on the **Devices** tab.
- **Offline on both sides despite both being open.** Usually a relay mismatch; see
  [I can't connect](#i-cant-connect-to-the-other-device).

Pairing codes expire after three minutes. Press **Show my pairing code** for a fresh one.
Turn on **Start DashBeam Lite when I sign in** so paired devices can see you without opening
the window first.

---

## The Nearby list is empty

Nearby uses [mDNS](https://en.wikipedia.org/wiki/Multicast_DNS) (Bonjour), which only
works when devices can multicast to each other on the same subnet.

- **macOS Local Network permission.** The first time DashBeam Lite looks for nearby devices,
  macOS asks for permission. If you declined, enable DashBeam Lite under **System Settings →
  Privacy & Security → Local Network**.
- **Tunnel-all VPNs hide the LAN.** Disconnect, or split-tunnel local subnets.
- **Guest and managed Wi-Fi often block peer-to-peer** and multicast.

Tickets and paired devices don't need mDNS and keep working. **Settings → Who can see
this device nearby** only controls whether others can see you.

---

## I don't get notifications

System notifications (incoming files, finished transfers) only work when DashBeam Lite runs
as an app bundle, not via `cargo run`. Build one with `cargo xtask bundle`, then
allow notifications for DashBeam Lite in **System Settings → Notifications**.

---

## Collecting logs

DashBeam Lite logs to standard error. Quit it from the menu bar icon first, then:

```bash
RUST_LOG=debug "/Applications/DashBeam Lite.app/Contents/MacOS/dashbeam-lite" 2>&1 | tee dashbeam-lite.log
```

Read the log before sharing it: it contains IP addresses, device identifiers, relay URLs,
your device name, and the names of files you shared.

---

## Reporting a bug

Include the DashBeam Lite version (shown at the bottom of Settings), your macOS version,
whether it ever worked, `dashbeam-lite.log`, and for transfer problems the other device's
platform and your relay settings.
