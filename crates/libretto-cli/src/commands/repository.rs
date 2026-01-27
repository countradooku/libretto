//! Repository command - manage repositories.

use anyhow::Result;
use clap::{Args, Subcommand};
use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait};

/// Arguments for the repository command
#[derive(Args, Debug, Clone)]
pub struct RepositoryArgs {
    #[command(subcommand)]
    pub action: RepositoryAction,

    /// Set config globally
    #[arg(short = 'g', long)]
    pub global: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum RepositoryAction {
    /// Add a repository
    Add {
        /// Repository name
        #[arg(required = true)]
        name: String,

        /// Repository URL or path
        #[arg(required = true)]
        url: String,

        /// Repository type (composer, vcs, path, artifact)
        #[arg(short = 't', long, default_value = "composer")]
        repo_type: String,
    },

    /// Remove a repository
    Remove {
        /// Repository name
        #[arg(required = true)]
        name: String,
    },

    /// List configured repositories
    List,

    /// Enable a repository
    Enable {
        /// Repository name
        #[arg(required = true)]
        name: String,
    },

    /// Disable a repository
    Disable {
        /// Repository name
        #[arg(required = true)]
        name: String,
    },
}

/// Run the repository command
pub async fn run(args: RepositoryArgs) -> Result<()> {
    use crate::output::{header, success};

    let config_path = if args.global {
        get_global_config_path()?
    } else {
        std::env::current_dir()?.join("composer.json")
    };

    match args.action {
        RepositoryAction::Add {
            name,
            url,
            repo_type,
        } => {
            header("Adding repository");
            add_repository(&config_path, &name, &url, &repo_type)?;
            success(&format!(
                "Added repository '{}' ({}: {})",
                name, repo_type, url
            ));
        }

        RepositoryAction::Remove { name } => {
            header("Removing repository");
            remove_repository(&config_path, &name)?;
            success(&format!("Removed repository '{}'", name));
        }

        RepositoryAction::List => {
            header("Configured repositories");
            list_repositories(&config_path)?;
        }

        RepositoryAction::Enable { name } => {
            header("Enabling repository");
            toggle_repository(&config_path, &name, true)?;
            success(&format!("Enabled repository '{}'", name));
        }

        RepositoryAction::Disable { name } => {
            header("Disabling repository");
            toggle_repository(&config_path, &name, false)?;
            success(&format!("Disabled repository '{}'", name));
        }
    }

    Ok(())
}

fn get_global_config_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("COMPOSER_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|_| {
            directories::UserDirs::new()
                .map(|d| d.home_dir().join(".composer"))
                .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
        })?;
    Ok(home.join("config.json"))
}

fn read_config(path: &std::path::PathBuf) -> Result<sonic_rs::Value> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        Ok(sonic_rs::from_str(&content)?)
    } else {
        Ok(sonic_rs::json!({}))
    }
}

fn write_config(path: &std::path::PathBuf, config: &sonic_rs::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = sonic_rs::to_string_pretty(config)?;
    std::fs::write(path, format!("{content}\n"))?;
    Ok(())
}

fn add_repository(
    config_path: &std::path::PathBuf,
    name: &str,
    url: &str,
    repo_type: &str,
) -> Result<()> {
    let mut config = read_config(config_path)?;

    // Ensure repositories section exists
    if config.get("repositories").is_none() {
        config
            .as_object_mut()
            .unwrap()
            .insert("repositories", sonic_rs::json!({}));
    }

    let repos = config.get_mut("repositories").unwrap();

    // Handle both array and object formats
    if repos.is_array() {
        // Array format - add as object with name key
        repos.as_array_mut().unwrap().push(sonic_rs::json!({
            "type": repo_type,
            "url": url,
            "name": name
        }));
    } else {
        // Object format - use name as key
        repos.as_object_mut().unwrap().insert(
            name,
            sonic_rs::json!({
                "type": repo_type,
                "url": url
            }),
        );
    }

    write_config(config_path, &config)
}

fn remove_repository(config_path: &std::path::PathBuf, name: &str) -> Result<()> {
    let mut config = read_config(config_path)?;

    if let Some(repos) = config.get_mut("repositories") {
        if repos.is_array() {
            if let Some(arr) = repos.as_array_mut() {
                arr.retain(|r| r.get("name").and_then(|n| n.as_str()) != Some(name));
            }
        } else if let Some(obj) = repos.as_object_mut() {
            obj.remove(&name.to_string());
        }
    }

    write_config(config_path, &config)
}

fn list_repositories(config_path: &std::path::PathBuf) -> Result<()> {
    use crate::output::table::Table;

    let config = read_config(config_path)?;

    // Add default packagist
    let mut repos: Vec<(String, String, String, bool)> = vec![(
        "packagist.org".to_string(),
        "composer".to_string(),
        "https://repo.packagist.org".to_string(),
        true,
    )];

    // Check if packagist is disabled
    let packagist_disabled = config
        .get("repositories")
        .and_then(|r| {
            if r.is_object() {
                r.get("packagist.org")
                    .or_else(|| r.get("packagist"))
                    .and_then(|p| {
                        if p.as_bool().is_some() {
                            Some(!p.as_bool().unwrap_or(true))
                        } else {
                            None
                        }
                    })
            } else {
                None
            }
        })
        .unwrap_or(false);

    if packagist_disabled {
        repos[0].3 = false;
    }

    // Add configured repositories
    if let Some(repositories) = config.get("repositories") {
        if repositories.is_array() {
            if let Some(arr) = repositories.as_array() {
                for repo in arr {
                    let name = repo
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unnamed");
                    let repo_type = repo
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("composer");
                    let url = repo.get("url").and_then(|u| u.as_str()).unwrap_or("");
                    let enabled = !repo
                        .get("enabled")
                        .and_then(|e| e.as_bool())
                        .map(|e| !e)
                        .unwrap_or(false);

                    repos.push((
                        name.to_string(),
                        repo_type.to_string(),
                        url.to_string(),
                        enabled,
                    ));
                }
            }
        } else if repositories.is_object() {
            if let Some(obj) = repositories.as_object() {
                for (name, repo) in obj {
                    if repo.as_bool().is_some() {
                        // Disabling a default repo
                        continue;
                    }

                    let repo_type = repo
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("composer");
                    let url = repo.get("url").and_then(|u| u.as_str()).unwrap_or("");
                    let enabled = !repo
                        .get("enabled")
                        .and_then(|e| e.as_bool())
                        .map(|e| !e)
                        .unwrap_or(false);

                    repos.push((
                        name.to_string(),
                        repo_type.to_string(),
                        url.to_string(),
                        enabled,
                    ));
                }
            }
        }
    }

    if repos.is_empty() {
        crate::output::info("No repositories configured");
        return Ok(());
    }

    let mut table = Table::new();
    table.headers(["Name", "Type", "URL", "Status"]);

    for (name, repo_type, url, enabled) in &repos {
        let status = if *enabled { "enabled" } else { "disabled" };
        let status_cell = if *enabled {
            table.success_cell(status)
        } else {
            table.dim_cell(status)
        };

        table.styled_row(vec![
            comfy_table::Cell::new(name),
            comfy_table::Cell::new(repo_type),
            comfy_table::Cell::new(url),
            status_cell,
        ]);
    }

    table.print();

    Ok(())
}

fn toggle_repository(config_path: &std::path::PathBuf, name: &str, enable: bool) -> Result<()> {
    let mut config = read_config(config_path)?;

    // Special handling for packagist
    if name == "packagist" || name == "packagist.org" {
        if config.get("repositories").is_none() {
            config
                .as_object_mut()
                .unwrap()
                .insert("repositories", sonic_rs::json!({}));
        }

        let repos = config.get_mut("repositories").unwrap();
        if !repos.is_object() {
            // Convert to object if it's an array
            let arr: Vec<sonic_rs::Value> = repos
                .as_array()
                .map(|a| a.iter().cloned().collect())
                .unwrap_or_default();
            *repos = sonic_rs::json!({});
            for r in arr {
                if let Some(n) = r.get("name").and_then(|n| n.as_str()) {
                    repos.as_object_mut().unwrap().insert(n, r.clone());
                }
            }
        }

        repos
            .as_object_mut()
            .unwrap()
            .insert("packagist.org", sonic_rs::json!(enable));

        return write_config(config_path, &config);
    }

    // For other repositories
    if let Some(repos) = config.get_mut("repositories") {
        if repos.is_array() {
            if let Some(arr) = repos.as_array_mut() {
                for repo in arr {
                    if repo.get("name").and_then(|n| n.as_str()) == Some(name) {
                        repo.as_object_mut()
                            .unwrap()
                            .insert("enabled", sonic_rs::json!(enable));
                        break;
                    }
                }
            }
        } else if let Some(obj) = repos.as_object_mut() {
            if let Some(repo) = obj.get_mut(&name.to_string()) {
                repo.as_object_mut()
                    .unwrap()
                    .insert("enabled", sonic_rs::json!(enable));
            }
        }
    }

    write_config(config_path, &config)
}
