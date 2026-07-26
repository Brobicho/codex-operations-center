# Security policy

## Reporting

Please report vulnerabilities privately through GitHub Security Advisories for this repository. Do not open a public issue containing an exploitable vulnerability or sensitive Codex session data.

## Local data

Codex Operations Center processes local Codex thread metadata and lifecycle events. Captured events remain on the local machine under the application data directory. The project does not include telemetry or a remote collector.

The installer:

- downloads release assets exclusively over HTTPS;
- verifies the published SHA-256 checksum;
- does not require root privileges;
- does not bypass Codex hook trust review;
- records the exact integration it owns so uninstallation preserves unrelated hooks.

Users should inspect one-line installation scripts before execution when required by their security policy.
