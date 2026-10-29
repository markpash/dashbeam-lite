# Security Policy

## Reporting a vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.** A public report tells everyone — including potential attackers — about the issue before it's fixed.

### Preferred: GitHub private vulnerability report

1. Go to the repo's **Security** tab → **Report a vulnerability**
2. Fill in the form — only the maintainers will see it
3. We'll work with you privately and credit you in the fix

Direct link: [Report a vulnerability](https://github.com/tonyantony300/dashbeam/security/advisories/new)

### Alternative: email

If you can't use GitHub's reporter, email **tnyantny@protonmail.com** with:

- Description of the vulnerability
- Steps to reproduce
- Affected versions (app, relay)
- Potential impact
- Any suggested fix

## What to expect

- Acknowledgement within **48 hours**
- Targeted fix within **14 days** for critical issues
- A coordinated disclosure timeline if the fix requires more time
- Credit in the release notes (unless you prefer to stay anonymous)

## Scope

In scope:

- The macOS app (`app/`) and release binaries
- P2P transfer engine (`engine/`) and protocol
- Self-hostable relay and discovery infra (`infra/relay/`, `infra/dns/`)

Out of scope:

- Vulnerabilities in third-party dependencies — please report those upstream (e.g. Iroh, GPUI)
- Issues that require physical access to an unlocked device
- Social engineering of users
- Public relay operators you do not control (report infra bugs in DashBeam's relay code; operator misconfiguration is out of scope)
