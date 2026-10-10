# Security policy

## Reporting a vulnerability
Please **do not** open a public issue for security problems. Use GitHub's private
vulnerability reporting instead: the "Security" tab of this repository →
"Report a vulnerability".

Include the affected version, platform, a description, and a proof of concept if you
have one, such as a sample file. Mark sample files as malicious in the report.

We aim to acknowledge reports within 7 days and to ship fixes for critical issues as
fast as possible. Reporters are credited unless they ask not to be.

## Scope
Everything in this repository: the app, CLI, MCP server and bridge, import worker,
updater, installers and release workflows. See `docs/THREAT_MODEL.md` for the threat
model and `docs/SECURITY_REVIEWS.md` for past reviews.

## Supported versions
Only the latest stable release and the current nightly receive security fixes.
