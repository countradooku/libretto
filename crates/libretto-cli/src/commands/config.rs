//! Config command - manage configuration.

use anyhow::{Context, Result};
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait};
use std::path::PathBuf;

/// Arguments for the config command
#[derive(Args, Debug, Clone)]
pub struct ConfigArgs {
    /// Setting name to get or set
    #[arg(value_name = "KEY")]
    pub key: Option<String>,

    /// Value to set (omit to get current value)
    #[arg(value_name = "VALUE")]
    pub value: Option<String>,

    /// Set config globally
    #[arg(short = 'g', long)]
    pub global: bool,

    /// List all config settings
    #[arg(short = 'l', long)]
    pub list: bool,

    /// Unset the config setting
    #[arg(long)]
    pub unset: bool,

    /// Edit config file in editor
    #[arg(short = 'e', long)]
    pub editor: bool,

    /// Merge with existing auth config
    #[arg(long)]
    pub auth: bool,

    /// Append to existing array values instead of overwriting
    #[arg(short = 'a', long)]
    pub append: bool,

    /// Output raw config value
    #[arg(long)]
    pub absolute: bool,
}

/// Run the config command
pub async fn run(args: ConfigArgs) -> Result<()> {
    use crate::output::{header, info, success};

    // Determine config file path
    let config_path = if args.global {
        get_global_config_path()?
    } else {
        std::env::current_dir()?.join("composer.json")
    };

    // Handle editor mode
    if args.editor {
        return open_in_editor(&config_path);
    }

    // Handle list mode
    if args.list {
        header("Configuration");
        return list_config(&config_path, args.global);
    }

    // Need a key for other operations
    let key = match &args.key {
        Some(k) => k,
        None => {
            header("Configuration");
            return list_config(&config_path, args.global);
        }
    };

    // Handle unset
    if args.unset {
        return unset_config(&config_path, key);
    }

    // Handle get/set
    match &args.value {
        Some(value) => {
            set_config(&config_path, key, value, args.append)?;
            success(&format!("Set {} = {}", key, value));
        }
        None => {
            let value = get_config(&config_path, key)?;
            if args.absolute {
                println!("{value}");
            } else {
                info(&format!("{} = {}", key, value));
            }
        }
    }

    Ok(())
}

fn get_global_config_path() -> Result<PathBuf> {
    let home = std::env::var("COMPOSER_HOME")
        .map(PathBuf::from)
        .or_else(|_| {
            directories::UserDirs::new()
                .map(|d| d.home_dir().join(".composer"))
                .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
        })?;
    Ok(home.join("config.json"))
}

fn open_in_editor(path: &PathBuf) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    // Ensure file exists
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, "{}\n")?;
    }

    std::process::Command::new(&editor)
        .arg(path)
        .status()
        .context(format!("Failed to open editor: {}", editor))?;

    Ok(())
}

fn list_config(path: &PathBuf, global: bool) -> Result<()> {
    use crate::output::table::Table;

    let location = if global { "global" } else { "local" };
    println!("Config file: {} ({})", path.display(), location);
    println!();

    if !path.exists() {
        crate::output::info("No configuration file found");
        return Ok(());
    }

    let content = std::fs::read_to_string(path)?;
    let json: sonic_rs::Value = sonic_rs::from_str(&content)?;

    let config = json.get("config");

    if config.is_none()
        || config
            .map(|c| c.as_object().map(|o| o.is_empty()).unwrap_or(true))
            .unwrap_or(true)
    {
        crate::output::info("No configuration settings found");
        return Ok(());
    }

    let mut table = Table::new();
    table.headers(["Setting", "Value"]);

    if let Some(config) = config.and_then(|c| c.as_object()) {
        let mut entries: Vec<_> = config.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (key, value) in entries {
            let key_str: String = key.to_string();
            let value_str = format_value(value);
            table.row([key_str.as_str(), value_str.as_str()]);
        }
    }

    table.print();

    Ok(())
}

fn get_config(path: &PathBuf, key: &str) -> Result<String> {
    if !path.exists() {
        anyhow::bail!("Configuration file not found: {}", path.display());
    }

    let content = std::fs::read_to_string(path)?;
    let json: sonic_rs::Value = sonic_rs::from_str(&content)?;

    // Support nested keys with dot notation
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = &json;

    // First check in config section
    if let Some(config) = json.get("config") {
        current = config;
    }

    for part in &parts {
        current = current
            .get(*part)
            .ok_or_else(|| anyhow::anyhow!("Key not found: {}", key))?;
    }

    Ok(format_value(current))
}

fn set_config(path: &PathBuf, key: &str, value: &str, append: bool) -> Result<()> {
    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read or create config
    let mut json: sonic_rs::Value = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        sonic_rs::from_str(&content)?
    } else {
        sonic_rs::json!({})
    };

    // Ensure config section exists
    if json.get("config").is_none() {
        json.as_object_mut()
            .unwrap()
            .insert("config", sonic_rs::json!({}));
    }

    let config = json.get_mut("config").unwrap().as_object_mut().unwrap();

    // Parse value
    let parsed_value = parse_config_value(value);

    if append {
        // Append to existing array
        if let Some(existing) = config.get_mut(&key.to_string()) {
            if let Some(arr) = existing.as_array_mut() {
                arr.push(parsed_value);
            } else {
                *existing = sonic_rs::json!([existing.clone(), parsed_value]);
            }
        } else {
            config.insert(key, sonic_rs::json!([parsed_value]));
        }
    } else {
        config.insert(key, parsed_value);
    }

    // Write back
    let output = sonic_rs::to_string_pretty(&json)?;
    std::fs::write(path, format!("{output}\n"))?;

    Ok(())
}

fn unset_config(path: &PathBuf, key: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(path)?;
    let mut json: sonic_rs::Value = sonic_rs::from_str(&content)?;

    if let Some(config) = json.get_mut("config").and_then(|c| c.as_object_mut()) {
        config.remove(&key.to_string());
    }

    let output = sonic_rs::to_string_pretty(&json)?;
    std::fs::write(path, format!("{output}\n"))?;

    crate::output::success(&format!("Unset {}", key));

    Ok(())
}

fn format_value(value: &sonic_rs::Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }
    if let Some(b) = value.as_bool() {
        return b.to_string();
    }
    if let Some(n) = value.as_i64() {
        return n.to_string();
    }
    if let Some(n) = value.as_f64() {
        return n.to_string();
    }
    if let Some(arr) = value.as_array() {
        let items: Vec<String> = arr.iter().map(format_value).collect();
        return format!("[{}]", items.join(", "));
    }
    if let Some(obj) = value.as_object() {
        let items: Vec<String> = obj
            .iter()
            .map(|(k, v)| format!("{}: {}", k, format_value(v)))
            .collect();
        return format!("{{{}}}", items.join(", "));
    }
    if value.is_null() {
        return "null".to_string();
    }
    "unknown".to_string()
}

fn parse_config_value(value: &str) -> sonic_rs::Value {
    // Try parsing as JSON first
    if let Ok(json) = sonic_rs::from_str::<sonic_rs::Value>(value) {
        return json;
    }

    // Try parsing as boolean
    match value.to_lowercase().as_str() {
        "true" | "yes" | "1" => return sonic_rs::json!(true),
        "false" | "no" | "0" => return sonic_rs::json!(false),
        _ => {}
    }

    // Try parsing as number
    if let Ok(n) = value.parse::<i64>() {
        return sonic_rs::json!(n);
    }
    if let Ok(n) = value.parse::<f64>() {
        return sonic_rs::json!(n);
    }

    // Default to string
    sonic_rs::json!(value)
}
