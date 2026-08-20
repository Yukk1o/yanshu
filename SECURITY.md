# Security policy

## Experimental status

Yanshu is experimental, AI-assisted software. It has not received sufficient independent human security review and may contain serious bugs, semantic inconsistencies, denial-of-service conditions, or data-loss defects.

No current release is supported for production, critical infrastructure, authorization decisions, financial decisions, or sensitive data. Passing the repository test suite is not a security certification.

## Reporting a vulnerability

Use GitHub private vulnerability reporting when it is available for this repository. If private reporting is unavailable, open a minimal public issue requesting a private contact channel; do not include exploit details, credentials, personal data, or secrets in that issue.

Please include, through the private channel:

- the affected revision and platform;
- the violated trust-boundary assumption;
- the smallest safe reproducer;
- expected and observed diagnostics;
- whether the issue crosses a capability, fuel, content-hash, package, or promotion boundary.

Do not test vulnerabilities against systems or data you do not own or have explicit permission to assess.

## Security invariants

Changes must preserve these invariants:

- no guest `eval`, ambient host access, arbitrary FFI, filesystem, network, thread, or dynamic-library access;
- no undeclared or runtime-only capability;
- no unmetered guest-controlled work;
- no executable use of generated review projections;
- no first-party Rust `unsafe` code, block, function, trait, or implementation;
- no credential, authentication header, provider configuration, or unredacted sensitive payload in logs or repository history.

The complete contributor contract is maintained in [docs/ai-agent-guide.md](docs/ai-agent-guide.md).
