# Security policy

Fundamental runs the parts of a system that guard people's data: sign-in, authorization, encryption, secrets and the database. Thank you for reporting a problem responsibly.

## Reporting a vulnerability

**Do not open a public issue.** Report privately through GitHub instead:

1. Go to the [Security tab](https://github.com/sounds-global/fundamental/security) of this repository.
2. Choose **Report a vulnerability**, and describe the problem, how to reproduce it, and its impact.

Only the maintainers can see the report. We aim to acknowledge it within 3 working days, and to agree with you on a fix and a disclosure date. We credit reporters in the advisory unless you prefer otherwise.

## What counts

Report anything that breaks a guarantee the framework makes, for example:

- a request that reaches an action, a page or a record its authorization should refuse;
- a session, passkey, token or signed value accepted when it is forged, expired or revoked;
- a secret, key or `Secret` value that ends up in a log, an error, a response or a trace;
- an encrypted field readable without its key, or a key that can be extracted;
- SQL or CQL injection, or a migration that destroys data without the lint refusing it;
- a way past the security headers, the request budget or the outbound-host checks.

## Supported versions

Fundamental is in early development and has no releases yet. Fixes are made on `main`.
