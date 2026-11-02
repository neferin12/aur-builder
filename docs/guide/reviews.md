# Release reviews and SSO

New packaging revisions wait for approval **before building**. The web UI requires
OIDC login over HTTPS. Published pacman downloads keep their separate repository
credentials.

## Configure OIDC

Register a confidential authorization-code client with your identity provider.
Enable PKCE and register the exact redirect URI `https://builder.your-domain/auth/callback`.
Configure the provider to include a string-array groups claim in the signed ID token.
A group returned only by the userinfo endpoint does not grant administrator access.

```dotenv
AB_WEB_URL=https://builder.your-domain
AB_OIDC_ISSUER=https://identity.your-domain/realms/builder
AB_OIDC_CLIENT_ID=aur-builder
AB_OIDC_CLIENT_SECRET=replace-with-client-secret
AB_OIDC_GROUPS_CLAIM=groups
AB_OIDC_SCOPES=profile
AB_OIDC_ADMIN_GROUP=aur-builder-admins
AB_SESSION_SECONDS=3600
```

`openid` is always requested. Add provider-specific scopes to the space-separated
`AB_OIDC_SCOPES` value if needed (for example `profile groups`).

The issuer must match the provider's discovery document. Group membership matches
exactly, including any group path prefix. Restart `web` after configuration changes.
The UI fails startup if OIDC is missing or discovery fails. Use an HTTPS reverse
proxy; session cookies are Secure and HttpOnly. HTTP access cannot establish a session.
Sessions expire after one hour by default (configurable from 60 to 86400 seconds).
Group membership is refreshed at login; it can remain effective for a session's lifetime.
When a later login changes administrator membership, all of that user's previous
sessions are revoked so an old cookie cannot inherit new admin privileges. Session
IDs are stored as hashes. Sessions and pending login flows are bound to the issuer,
client credentials, web origin, group mapping, scopes, and session lifetime; changing
those settings requires signing in again. All web replicas must use the same settings.
The external web origin is normalized, including hostname casing and default HTTPS port.
Invalid session lifetime values fail startup rather than silently using a default.

OIDC authorization, token, discovery and signing-key requests require HTTPS, and the
HTTP client does not follow redirects. The application verifies signatures, issuer,
audience, nonce, expiry, the authorized-party (`azp`) claim when present, and rejects
issue times more than 60 seconds in the future. Additional untrusted token audiences
are rejected. All mutation routes require both an exact same-origin request and a
session CSRF token. Unlisted routes receive no authorization by default.

Signing keys are cached for at most five minutes, with a discovery refresh on an
unknown key or invalid signature. Refresh attempts are limited to once a minute.
If refresh fails after the cache expires, new logins fail closed; existing sessions
retain their normal expiry. Providers should overlap old and new signing keys while
rotating them. A new provider signing algorithm may require registration/configuration
changes in addition to key rotation.

Sign out revokes the local browser session and shows a signed-out page. It does not
end the identity provider's own session; selecting **Sign in again** may use SSO without
a password prompt. Signing in again rotates the browser's previous local session.
Authentication responses, including errors and redirects, disable caching and referrer
forwarding; the UI also denies framing to protect approval controls.

## Manage users

Sign in with an account in the configured administrator group. Other users appear
in **User management** after their first login and initially have **no permissions**.
Administrators can independently grant global permissions to:

- View packages, logs, and reviews.
- Rebuild approved releases (also requires view).
- Approve or reject release candidates (also requires view).

Administrator status comes exclusively from OIDC group membership. The UI cannot
grant it. Disabling an account blocks even an OIDC administrator; you cannot disable
your own account. Saving access changes revokes the affected user's sessions.
Users are identified by issuer and subject, not by their display name or email.
Permission changes and release decisions are recorded in the database audit tables.

## Review releases

Open **Release reviews** and select a pending candidate. Review the packaging diff
against the last approved revision, including PKGBUILD, `.SRCINFO`, patches, and
other tracked repository files. Choose unified or side-by-side mode and expand
unchanged context as needed. Exact-content renames and file mode changes are shown.
First releases show tracked files as additions and always require human approval.

Approving atomically records the decision and queues an immutable source commit.
Workers check approval before building; publication checks approval and attempt
identity again. Rebuilds reuse approved source and settings. Changing repository,
subfolder, options, or configured environment creates a new candidate. Environment
values remain hidden in the UI; check them in deployment configuration when flagged.
Repository URL usernames, passwords, query values and fragments are hidden in the
review metadata as well. Stored source snapshots themselves are not secret-filtered;
do not commit credentials to packaging files or put them in build options.

Rejection requires a comment. A newer candidate supersedes a pending candidate;
a stale decision cannot enqueue a build. Rejected releases remain in history.
The server dispatches queued approvals on its next check, using `sleepduration`.
Returning upstream to an earlier revision also supersedes pending candidates and
creates a fresh review. An old approved, rejected, or superseded candidate is never
reactivated merely because its source fingerprint appears again.

Text snapshots are retained in SQL with file hashes and modes. Binary files,
submodules, files larger than 512 KiB, or contents beyond the 4 MiB snapshot budget
are explicitly marked for outside review. Their contents are not stored in SQL.
An unavailable approved Git commit causes a build failure rather than a fallback
to the latest branch. Keep source history available on your Git server.
Downloaded application sources and independently resolved AUR dependencies are
outside the packaging review; a commit pin does not freeze those downloads.

## Optional OpenAI review

The Rust server uses the community `async-openai` SDK. AI is disabled by default.
Set a model that supports strict JSON Schema output:

```dotenv
AB_AI_MODE=feedback
AB_AI_MODEL=your-supported-model
OPENAI_API_KEY=your-api-key
AB_AI_MAX_BYTES=200000
AB_AI_TIMEOUT_SECONDS=90
```

| Mode | Behavior |
| --- | --- |
| `disabled` | No API calls |
| `feedback` | Show persisted AI findings above the diff; human approval required |
| `auto_approve` | Approve an eligible update when the AI explicitly recommends approval |

Automatic approval requires an approved baseline and complete input. First releases,
binary/oversized input, changed environment settings, missing baselines, refusals,
invalid responses, and API failures require human review. Each request has a timeout
and at most three attempts. The AI cannot override a completed human decision or
approve a superseded candidate.

Packaging snapshots, repository text and build options are sent to OpenAI. Configured
environment values are excluded. Keep secrets out of committed packaging files.
The review uses strict structured output and treats repository instructions as
untrusted text, but an AI recommendation still requires your trust in the chosen model.
The [OpenAI structured output documentation](https://developers.openai.com/api/docs/guides/structured-outputs)
describes supported schema behavior and refusals.

Results retain the model, policy mode, revision, input hash, timestamp, findings, and
errors. Restart the server after changing policy. Existing results are retained;
switching modes does not automatically re-review or retroactively approve them.

## Human review notifications

Run the existing `notifier` service with the SMTP settings (`AB_SMTP_HOST`,
`AB_SMTP_USER`, `AB_SMTP_PASS`, `AB_SMTP_FROM`, `AB_SMTP_TO`, and `AB_MAILLOGO`).
Review alerts use the same recipient as build-result emails and include the package,
version, revision, reason, and a direct link to the review using `AB_WEB_URL`.
The server also needs `AB_WEB_URL` to construct those links.

First releases and releases in disabled or feedback-only AI mode trigger an alert
without waiting for AI. In automatic approval mode, an update triggers an alert when
AI requests human attention, fails, cannot review the full input, or automatic approval
cannot complete. Successfully approved updates do not trigger review alerts.

Alerts are recorded durably once per candidate and published through the existing
`notifications` queue, so regular polling and service restarts do not repeatedly
notify. Already approved, rejected, or superseded candidates are skipped before
publication. Existing pending candidates are picked up after upgrading. Broker
failures and SMTP failures retry; delivery is at least once, so a crash after sending
but before recording acknowledgement can produce a duplicate. An email already
queued may refer to a release that was reviewed or superseded afterward.

## Security review and validation

Regression tests cover OIDC signature and claim validation, key rotation, login-state
replay and configuration binding, default-denied access, CSRF checks on mutation
routes, hashed session storage, privilege-change revocation, logout, escaped source
rendering, source reverts, and upgrades from the earlier review schema.

These controls follow [OpenID Connect token validation](https://openid.net/specs/openid-connect-core-1_0.html#IDTokenValidation)
and [OWASP session management guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).
Live identity-provider, SMTP, RabbitMQ, Garage, and PostgreSQL deployment checks are
still necessary for your environment; the automated default tests use SQLite and
local mock identity/AI providers. No production credentials are used by these tests.
