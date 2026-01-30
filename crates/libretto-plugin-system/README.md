# libretto-plugin-system

Plugin system for the Libretto package manager, providing Composer plugin compatibility.

## Overview

This crate implements the plugin system for Libretto, enabling:

- **Composer plugin compatibility**: Run existing Composer plugins
- **Event system**: Hook into package manager lifecycle events
- **Capability system**: Extend Libretto's functionality
- **Plugin isolation**: Safe execution of third-party code

## Features

- Support for Composer 2.x plugin API
- Event-driven architecture with pub/sub pattern
- Plugin capability declarations
- Dependency injection for plugin services
- Sandboxed PHP execution for plugin scripts

## Event Types

Libretto fires events at various points in the package management lifecycle:

| Event | Description |
|-------|-------------|
| `pre-install-cmd` | Before the install command is executed |
| `post-install-cmd` | After the install command is executed |
| `pre-update-cmd` | Before the update command is executed |
| `post-update-cmd` | After the update command is executed |
| `pre-autoload-dump` | Before the autoloader is dumped |
| `post-autoload-dump` | After the autoloader is dumped |
| `pre-package-install` | Before a package is installed |
| `post-package-install` | After a package is installed |
| `pre-package-update` | Before a package is updated |
| `post-package-update` | After a package is updated |
| `pre-package-uninstall` | Before a package is uninstalled |
| `post-package-uninstall` | After a package is uninstalled |

## Plugin Capabilities

Plugins can declare capabilities to extend Libretto:

- **CommandProvider**: Add custom CLI commands
- **RepositoryFactory**: Support custom repository types
- **InstallerFactory**: Custom package installers
- **PreFileDownloadEvent**: Modify download requests
- **PostFileDownloadEvent**: Process downloaded files

## Usage

```rust
use libretto_plugin_system::{PluginManager, PluginConfig};

// Create a plugin manager
let manager = PluginManager::new(PluginConfig::default());

// Load plugins from composer.json
manager.load_from_manifest(&composer_json)?;

// Fire an event
manager.dispatch_event(Event::PreInstallCmd)?;

// Execute plugin scripts
manager.run_scripts("post-install-cmd")?;
```

## Plugin Configuration

Plugins are configured in `composer.json`:

```json
{
    "require": {
        "vendor/my-plugin": "^1.0"
    },
    "config": {
        "allow-plugins": {
            "vendor/my-plugin": true
        }
    }
}
```

## Security

- Plugins must be explicitly allowed in `config.allow-plugins`
- Plugin code runs in isolated PHP processes
- Network and filesystem access can be restricted
- Plugin signatures can be verified (optional)

## Compatibility

This plugin system aims for compatibility with Composer 2.x plugins while providing
performance improvements through Rust-native implementations where possible.

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.