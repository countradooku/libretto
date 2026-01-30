# libretto-config

Hierarchical configuration management for the Libretto package manager.

## Overview

This crate provides comprehensive configuration management for Libretto, supporting:

- **Hierarchical configuration**: Global, user, and project-level settings
- **Composer compatibility**: Full compatibility with `composer.json` and `auth.json`
- **Secure credential storage**: Integration with system keyring
- **File watching**: Live reload of configuration changes
- **Environment variables**: Override settings via environment

## Features

- Thread-safe configuration access via `DashMap`
- Schema validation with helpful error messages
- Secure storage for authentication tokens and passwords
- Support for private repository authentication
- Platform-specific defaults (paths, line endings, etc.)

## Configuration Hierarchy

Configuration is loaded in order of precedence (highest to lowest):

1. Environment variables (`LIBRETTO_*`, `COMPOSER_*`)
2. Project configuration (`./composer.json`, `./.libretto/config.json`)
3. User configuration (`~/.config/libretto/config.json`, `~/.composer/config.json`)
4. Global defaults

## Usage

```rust
use libretto_config::{Config, ConfigBuilder};

// Load configuration with defaults
let config = Config::load()?;

// Access settings
let cache_dir = config.cache_dir();
let timeout = config.http_timeout();

// Check authentication for a repository
if let Some(auth) = config.auth_for_host("repo.packagist.com") {
    // Use authentication
}
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `LIBRETTO_HOME` | Base directory for Libretto data |
| `LIBRETTO_CACHE_DIR` | Cache directory override |
| `COMPOSER_HOME` | Composer home directory (for compatibility) |
| `COMPOSER_AUTH` | JSON authentication data |

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.