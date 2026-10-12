<!-- Generated from config/templates/SECURITY.md and config/product.toml by `pnpm product:generate`. Do not edit: change those and run it again. -->
# Security policy

## Reporting a vulnerability

Report vulnerabilities in Wirecatch privately, never in a public issue, pull request or discussion:

- by email to TODO-PRODUCT-SECURITY-EMAIL@placeholder.invalid, or
- through GitHub's private vulnerability reporting: https://github.com/ehsan18t/wirecatch-oss-dm/security/advisories/new

Say what you found, how to reproduce it, and what an attacker could do with it. We confirm that we received it, and agree with you when the fix and the report are made public.

## What is in scope

The desktop app, the browser extension, the browser bridge, the `wirecatch` command-line downloader and the `wirecatch-engine` crate, as released from https://github.com/ehsan18t/wirecatch-oss-dm. Anything that handles cookies, credentials, paths from outside, downloaded executables or another process's messages is held to the hostile case (docs/ENGINEERING.md, section 11).

## Supported versions

Fixes go into the latest release. Update to it before reporting, and say which version you use.
