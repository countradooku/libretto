# libretto-audit

Security auditing for the Libretto package manager.

## Overview

This crate provides comprehensive security auditing capabilities for Libretto:

- **Vulnerability checking**: Query security advisories from Packagist and other sources
- **Checksum verification**: Validate package integrity with SHA-1, SHA-256, and BLAKE3
- **Signature verification**: Ed25519 and PGP signature validation
- **Trust chain management**: PKI and Web of Trust models for signature verification
- **Credential management**: Secure storage of authentication tokens via system keyring
- **Audit logging**: Comprehensive logging of security-relevant operations

## Features

- **Advisory database**: Fetch and cache security advisories from Packagist
- **Severity levels**: Critical, High, Medium, Low, and Unknown classifications
- **Version range matching**: Identify vulnerable versions using Composer constraints
- **Suggested updates**: Recommend safe versions when vulnerabilities are found
- **Secure file operations**: Platform-specific secure file handling

## Usage

```rust
use libretto_audit::{Auditor, AuditConfig, Severity};

// Create an auditor
let auditor = Auditor::new(AuditConfig::default())?;

// Audit packages from composer.lock
let results = auditor.audit_lockfile("./composer.lock").await?;

// Check results
for advisory in &results.advisories {
    if advisory.severity >= Severity::High {
        println!(
            "CRITICAL: {} {} - {}",
            advisory.package,
            advisory.affected_versions,
            advisory.title
        );
    }
}
```

## CLI Integration

The audit functionality is exposed via CLI commands:

```bash
# Audit current project
libretto audit

# Audit with suggested version updates
libretto audit --suggest-versions

# Fail if vulnerabilities found
libretto audit --fail-on-audit

# Audit only from lockfile (no network for package resolution)
libretto audit --locked
```

## Security Features

### Integrity Verification

| Algorithm | Use Case | Performance |
|-----------|----------|-------------|
| SHA-1 | Legacy Composer dist checksums | Fast |
| SHA-256 | Modern integrity verification | Fast |
| BLAKE3 | Content-addressable storage | Very fast (SIMD) |

### Signature Verification

- **Ed25519**: Fast elliptic curve signatures
- **PGP (via Sequoia)**: OpenPGP signature verification
- **Trust chains**: Support for PKI and Web of Trust models

### Credential Management

- System keyring integration (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- Git credential helper support (`git credential fill/approve/reject`)
- Secure memory handling with `zeroize`

## Configuration

```rust
use libretto_audit::AuditConfig;

let config = AuditConfig {
    // Fail installation if vulnerabilities found
    fail_on_vulnerabilities: true,
    
    // Minimum severity to report
    min_severity: Severity::Medium,
    
    // Enable signature verification
    verify_signatures: true,
    
    // Cache advisory data (seconds)
    cache_ttl: 3600,
};
```

## Performance

The auditor is optimized for speed:

- **Concurrent fetching**: Parallel advisory lookups with semaphore control
- **Response caching**: DashMap-based caching for repeated queries
- **Target**: Audit 500 packages in <200ms
- **Target**: Signature verification in <10ms

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.