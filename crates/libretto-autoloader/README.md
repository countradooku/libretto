# libretto-autoloader

Ultra-fast PHP autoloader generation for the Libretto package manager.

## Overview

This crate generates Composer-compatible PHP autoloaders with maximum performance:

- **PSR-4 autoloading**: Full support for PSR-4 namespace mapping
- **PSR-0 autoloading**: Legacy PSR-0 support for older packages
- **Classmap generation**: Scan PHP files to build complete class maps
- **File includes**: Support for `files` autoloading
- **Tree-sitter parsing**: Fast, accurate PHP parsing for class discovery

## Features

- **Parallel scanning**: Multi-threaded directory scanning with `rayon`
- **Incremental updates**: Only rescan changed files
- **Optimized classmaps**: Authoritative classmap mode for production
- **APCu caching**: Optional APCu cache integration
- **Zero-copy parsing**: Memory-efficient PHP file analysis

## Autoload Types

| Type | Description | Use Case |
|------|-------------|----------|
| PSR-4 | Namespace-to-directory mapping | Modern packages |
| PSR-0 | Legacy namespace mapping | Older packages |
| Classmap | Direct class-to-file mapping | Maximum performance |
| Files | Always-included files | Helper functions |

## Usage

```rust
use libretto_autoloader::{AutoloaderGenerator, AutoloadConfig};

// Generate autoloader from composer.json
let generator = AutoloaderGenerator::new();

let config = AutoloadConfig::from_composer_json("composer.json")?;
generator.generate(&config, "vendor/autoload.php")?;

// Generate optimized classmap
generator.generate_optimized(&config, "vendor/autoload.php")?;
```

## Generated Files

The generator creates the following files:

```
vendor/
├── autoload.php              # Main entry point
└── composer/
    ├── autoload_real.php     # Core autoloader logic
    ├── autoload_static.php   # Static class maps (optimized)
    ├── autoload_classmap.php # Class-to-file mappings
    ├── autoload_namespaces.php # PSR-0 namespaces
    ├── autoload_psr4.php     # PSR-4 namespaces
    └── autoload_files.php    # Files to include
```

## Optimization Levels

### Standard (default)
- PSR-4/PSR-0 directory scanning at runtime
- Good for development

### Level 1: Optimized (`--optimize-autoloader`, `-o`)
- Converts PSR-4/PSR-0 to classmap
- Faster autoloading, slower generation

### Level 2: Authoritative (`--classmap-authoritative`, `-a`)
- Only loads classes from classmap
- Fastest autoloading
- Classes not in classmap won't be found

### APCu Cache (`--apcu-autoloader`)
- Caches classmap in APCu
- Best for production with APCu extension

## Performance

Compared to Composer's autoloader generation:

| Operation | Composer | Libretto |
|-----------|----------|----------|
| Parse PHP file | ~5ms | ~0.5ms |
| Scan 1000 files | ~5s | ~0.5s |
| Generate classmap | ~2s | ~0.2s |

Performance gains come from:
- **Tree-sitter**: Incremental parsing, no PHP process needed
- **Parallel I/O**: Multi-threaded file scanning
- **Memory mapping**: Efficient large file handling
- **SIMD search**: Fast byte pattern matching with `memchr`

## PHP Parsing

This crate uses `tree-sitter-php` for parsing PHP files to extract:

- Class declarations
- Interface declarations
- Trait declarations
- Enum declarations (PHP 8.1+)
- Namespace declarations

Falls back to regex-based parsing for edge cases.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.