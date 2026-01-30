# libretto-cli

Command-line interface for the Libretto package manager.

## Overview

This crate provides the main CLI binary for Libretto, a high-performance
Composer-compatible package manager for PHP written in Rust.

## Installation

```bash
# From source
cargo install libretto-cli

# Or build from this repository
cargo build --release -p libretto-cli
```

## Commands

| Command | Description |
|---------|-------------|
| `install` | Install dependencies from composer.json/composer.lock |
| `update` | Update dependencies to latest versions |
| `require` | Add a package to dependencies |
| `remove` | Remove a package from dependencies |
| `search` | Search for packages on Packagist |
| `show` | Show package information |
| `init` | Initialize a new composer.json |
| `validate` | Validate composer.json |
| `dump-autoload` | Regenerate PHP autoloader |
| `audit` | Check for security vulnerabilities |
| `cache:clear` | Clear the package cache |
| `cache:list` | List cached packages |

## Usage

```bash
# Install dependencies
libretto install

# Install with dev dependencies
libretto install

# Install without dev dependencies
libretto install --no-dev

# Update all packages
libretto update

# Update specific packages
libretto update symfony/console monolog/monolog

# Add a new dependency
libretto require symfony/console:^6.0

# Add a dev dependency
libretto require --dev phpunit/phpunit:^10.0

# Remove a package
libretto remove symfony/console

# Search for packages
libretto search http client

# Show package info
libretto show symfony/console

# Audit for vulnerabilities
libretto audit

# Clear cache
libretto cache:clear
```

## Flags

### Global Flags

| Flag | Description |
|------|-------------|
| `-v`, `--verbose` | Increase verbosity |
| `-q`, `--quiet` | Suppress output |
| `--no-progress` | Disable progress bars |
| `--no-ansi` | Disable ANSI colors |

### Install Flags

| Flag | Description |
|------|-------------|
| `--no-dev` | Skip dev dependencies |
| `--prefer-dist` | Prefer distribution archives |
| `--prefer-source` | Prefer VCS sources |
| `--dry-run` | Show what would be installed |
| `--ignore-platform-reqs` | Ignore platform requirements |
| `--optimize-autoloader` | Optimize autoloader for production |
| `--classmap-authoritative` | Use classmap-only autoloading |
| `--audit` | Run security audit after install |
| `--fail-on-audit` | Fail if vulnerabilities found |
| `--verify-checksums` | Verify package checksums |

## Performance Features

Libretto CLI implements several optimizations:

- **Parallel resolution**: Resolve dependencies concurrently
- **HTTP/2 multiplexing**: Efficient network utilization
- **Adaptive concurrency**: Scale downloads based on CPU cores
- **Content-addressable storage**: Deduplicate packages across projects
- **Hardlink installation**: Instant installs from cache

## Environment Variables

| Variable | Description |
|----------|-------------|
| `LIBRETTO_HOME` | Base directory for Libretto data |
| `LIBRETTO_CACHE_DIR` | Cache directory |
| `COMPOSER_HOME` | Composer home (for compatibility) |
| `COMPOSER_AUTH` | Authentication JSON |
| `NO_COLOR` | Disable colored output |

## Composer Compatibility

Libretto is designed as a drop-in replacement for Composer:

- Reads `composer.json` and `composer.lock`
- Generates Composer-compatible autoloader
- Supports the same CLI interface
- Works with Packagist and private repositories

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.