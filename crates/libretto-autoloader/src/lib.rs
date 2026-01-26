//! PHP autoloader generation for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use libretto_core::{Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;
use walkdir::WalkDir;

/// PSR-4 autoload configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Psr4Config {
    /// Namespace to directory mappings.
    #[serde(flatten)]
    pub mappings: HashMap<String, Vec<String>>,
}

/// PSR-0 autoload configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Psr0Config {
    /// Namespace to directory mappings.
    #[serde(flatten)]
    pub mappings: HashMap<String, Vec<String>>,
}

/// Classmap configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClassmapConfig {
    /// Directories/files to scan.
    pub paths: Vec<String>,
}

/// Files to always include.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FilesConfig {
    /// Files to include.
    pub files: Vec<String>,
}

/// Complete autoload configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AutoloadConfig {
    /// PSR-4 autoloading.
    #[serde(default, rename = "psr-4")]
    pub psr4: Psr4Config,
    /// PSR-0 autoloading.
    #[serde(default, rename = "psr-0")]
    pub psr0: Psr0Config,
    /// Classmap autoloading.
    #[serde(default)]
    pub classmap: ClassmapConfig,
    /// Files to include.
    #[serde(default)]
    pub files: FilesConfig,
}

/// Autoloader generator.
#[derive(Debug)]
pub struct AutoloaderGenerator {
    vendor_dir: PathBuf,
    classmap: HashMap<String, PathBuf>,
    psr4_map: HashMap<String, Vec<PathBuf>>,
    files: Vec<PathBuf>,
}

impl AutoloaderGenerator {
    /// Create new generator.
    #[must_use]
    pub fn new(vendor_dir: PathBuf) -> Self {
        Self {
            vendor_dir,
            classmap: HashMap::new(),
            psr4_map: HashMap::new(),
            files: Vec::new(),
        }
    }

    /// Add autoload configuration from a package.
    pub fn add_package(&mut self, package_dir: &Path, config: &AutoloadConfig) {
        for (namespace, dirs) in &config.psr4.mappings {
            let paths: Vec<PathBuf> = dirs.iter().map(|d| package_dir.join(d)).collect();
            self.psr4_map
                .entry(namespace.clone())
                .or_default()
                .extend(paths);
        }

        for path in &config.classmap.paths {
            let full_path = package_dir.join(path);
            if full_path.exists() {
                self.scan_for_classes(&full_path);
            }
        }

        for file in &config.files.files {
            let full_path = package_dir.join(file);
            if full_path.exists() {
                self.files.push(full_path);
            }
        }
    }

    /// Generate autoloader files.
    ///
    /// # Errors
    /// Returns error if generation fails.
    pub fn generate(&self) -> Result<()> {
        let autoload_dir = self.vendor_dir.join("composer");
        std::fs::create_dir_all(&autoload_dir).map_err(|e| Error::io(&autoload_dir, e))?;

        self.generate_autoload_real(&autoload_dir)?;
        self.generate_autoload_static(&autoload_dir)?;
        self.generate_autoload_psr4(&autoload_dir)?;
        self.generate_autoload_classmap(&autoload_dir)?;
        self.generate_autoload_files(&autoload_dir)?;
        self.generate_autoload(&self.vendor_dir)?;

        info!(
            psr4_namespaces = self.psr4_map.len(),
            classmap_entries = self.classmap.len(),
            files = self.files.len(),
            "autoloader generated"
        );

        Ok(())
    }

    fn scan_for_classes(&mut self, path: &Path) {
        let class_pattern =
            Regex::new(r"(?m)^\s*(?:abstract\s+|final\s+)?(?:class|interface|trait|enum)\s+(\w+)")
                .expect("valid regex");
        let namespace_pattern = Regex::new(r"(?m)^\s*namespace\s+([\w\\]+)").expect("valid regex");

        let walker = if path.is_file() {
            WalkDir::new(path).max_depth(0)
        } else {
            WalkDir::new(path)
        };

        for entry in walker.into_iter().filter_map(std::result::Result::ok) {
            let file_path = entry.path();
            if file_path.extension().is_some_and(|e| e == "php") {
                if let Ok(contents) = std::fs::read_to_string(file_path) {
                    let namespace = namespace_pattern
                        .captures(&contents)
                        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));

                    for cap in class_pattern.captures_iter(&contents) {
                        if let Some(class_name) = cap.get(1) {
                            let fqcn = match &namespace {
                                Some(ns) => format!("{}\\{}", ns, class_name.as_str()),
                                None => class_name.as_str().to_string(),
                            };
                            self.classmap.insert(fqcn, file_path.to_path_buf());
                        }
                    }
                }
            }
        }
    }

    fn generate_autoload_real(&self, dir: &Path) -> Result<()> {
        let path = dir.join("autoload_real.php");
        let content = r#"<?php

// autoload_real.php @generated by Libretto

class ComposerAutoloaderInit
{
    private static $loader;

    public static function loadClassLoader($class)
    {
        if ('Composer\Autoload\ClassLoader' === $class) {
            require __DIR__ . '/ClassLoader.php';
        }
    }

    public static function getLoader()
    {
        if (null !== self::$loader) {
            return self::$loader;
        }

        spl_autoload_register(array('ComposerAutoloaderInit', 'loadClassLoader'), true, true);
        self::$loader = $loader = new \Composer\Autoload\ClassLoader(\dirname(__DIR__));
        spl_autoload_unregister(array('ComposerAutoloaderInit', 'loadClassLoader'));

        require __DIR__ . '/autoload_static.php';
        call_user_func(\Composer\Autoload\ComposerStaticInit::getInitializer($loader));

        $loader->register(true);

        $includeFiles = \Composer\Autoload\ComposerStaticInit::$files;
        foreach ($includeFiles as $fileIdentifier => $file) {
            composerRequire($fileIdentifier, $file);
        }

        return $loader;
    }
}

function composerRequire($fileIdentifier, $file)
{
    if (empty($GLOBALS['__composer_autoload_files'][$fileIdentifier])) {
        $GLOBALS['__composer_autoload_files'][$fileIdentifier] = true;
        require $file;
    }
}
"#;
        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }

    fn generate_autoload_static(&self, dir: &Path) -> Result<()> {
        let path = dir.join("autoload_static.php");

        let mut psr4_entries = String::new();
        for (namespace, paths) in &self.psr4_map {
            let escaped_ns = namespace.replace('\\', "\\\\");
            let paths_php: Vec<String> = paths
                .iter()
                .map(|p| {
                    format!(
                        "__DIR__ . '/../{}'",
                        p.strip_prefix(&self.vendor_dir).unwrap_or(p).display()
                    )
                })
                .collect();
            psr4_entries.push_str(&format!(
                "        '{}' => array({}),\n",
                escaped_ns,
                paths_php.join(", ")
            ));
        }

        let mut classmap_entries = String::new();
        for (class, file_path) in &self.classmap {
            let escaped_class = class.replace('\\', "\\\\");
            let relative = file_path
                .strip_prefix(&self.vendor_dir)
                .unwrap_or(file_path);
            classmap_entries.push_str(&format!(
                "        '{}' => __DIR__ . '/../{}',\n",
                escaped_class,
                relative.display()
            ));
        }

        let mut files_entries = String::new();
        for (i, file_path) in self.files.iter().enumerate() {
            let relative = file_path
                .strip_prefix(&self.vendor_dir)
                .unwrap_or(file_path);
            files_entries.push_str(&format!(
                "        '{:x}' => __DIR__ . '/../{}',\n",
                i,
                relative.display()
            ));
        }

        let content = format!(
            r#"<?php

// autoload_static.php @generated by Libretto

namespace Composer\Autoload;

class ComposerStaticInit
{{
    public static $files = array(
{files_entries}    );

    public static $prefixLengthsPsr4 = array();

    public static $prefixDirsPsr4 = array(
{psr4_entries}    );

    public static $classMap = array(
{classmap_entries}    );

    public static function getInitializer(ClassLoader $loader)
    {{
        return \Closure::bind(function () use ($loader) {{
            $loader->prefixLengthsPsr4 = ComposerStaticInit::$prefixLengthsPsr4;
            $loader->prefixDirsPsr4 = ComposerStaticInit::$prefixDirsPsr4;
            $loader->classMap = ComposerStaticInit::$classMap;
        }}, null, ClassLoader::class);
    }}
}}
"#
        );

        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }

    fn generate_autoload_psr4(&self, dir: &Path) -> Result<()> {
        let path = dir.join("autoload_psr4.php");

        let mut entries = String::new();
        for (namespace, paths) in &self.psr4_map {
            let escaped_ns = namespace.replace('\\', "\\\\");
            let paths_php: Vec<String> = paths
                .iter()
                .map(|p| {
                    format!(
                        "$vendorDir . '/{}'",
                        p.strip_prefix(&self.vendor_dir).unwrap_or(p).display()
                    )
                })
                .collect();
            entries.push_str(&format!(
                "    '{}' => array({}),\n",
                escaped_ns,
                paths_php.join(", ")
            ));
        }

        let content = format!(
            r#"<?php

// autoload_psr4.php @generated by Libretto

$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);

return array(
{entries});
"#
        );

        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }

    fn generate_autoload_classmap(&self, dir: &Path) -> Result<()> {
        let path = dir.join("autoload_classmap.php");

        let mut entries = String::new();
        for (class, file_path) in &self.classmap {
            let escaped_class = class.replace('\\', "\\\\");
            let relative = file_path
                .strip_prefix(&self.vendor_dir)
                .unwrap_or(file_path);
            entries.push_str(&format!(
                "    '{}' => $vendorDir . '/../{}',\n",
                escaped_class,
                relative.display()
            ));
        }

        let content = format!(
            r#"<?php

// autoload_classmap.php @generated by Libretto

$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);

return array(
{entries});
"#
        );

        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }

    fn generate_autoload_files(&self, dir: &Path) -> Result<()> {
        let path = dir.join("autoload_files.php");

        let mut entries = String::new();
        for (i, file_path) in self.files.iter().enumerate() {
            let relative = file_path
                .strip_prefix(&self.vendor_dir)
                .unwrap_or(file_path);
            entries.push_str(&format!(
                "    '{:x}' => $vendorDir . '/../{}',\n",
                i,
                relative.display()
            ));
        }

        let content = format!(
            r#"<?php

// autoload_files.php @generated by Libretto

$vendorDir = dirname(__DIR__);
$baseDir = dirname($vendorDir);

return array(
{entries});
"#
        );

        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }

    fn generate_autoload(&self, vendor_dir: &Path) -> Result<()> {
        let path = vendor_dir.join("autoload.php");
        let content = r#"<?php

// autoload.php @generated by Libretto

if (PHP_VERSION_ID < 80000) {
    echo 'Libretto requires PHP 8.0 or higher.' . PHP_EOL;
    exit(1);
}

require_once __DIR__ . '/composer/autoload_real.php';

return ComposerAutoloaderInit::getLoader();
"#;
        std::fs::write(&path, content).map_err(|e| Error::io(&path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoload_config_default() {
        let config = AutoloadConfig::default();
        assert!(config.psr4.mappings.is_empty());
        assert!(config.classmap.paths.is_empty());
    }

    #[test]
    fn generator_creation() {
        let gen = AutoloaderGenerator::new(PathBuf::from("/tmp/vendor"));
        assert!(gen.classmap.is_empty());
    }
}
