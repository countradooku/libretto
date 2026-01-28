//! Packagist API response types.

use libretto_core::{
    Author, Dependency, Package, PackageId, PackageSource, PackageType, Version, VersionConstraint,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// Packages field can be either an empty array or a map.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PackagesField {
    /// Empty array.
    Array(Vec<sonic_rs::Value>),
    /// Map of packages.
    Map(HashMap<String, HashMap<String, PackageVersionJson>>),
}

impl Default for PackagesField {
    fn default() -> Self {
        Self::Array(Vec::new())
    }
}

/// HashMap field that can be "__unset" in minified metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MapOrUnset<K, V>
where
    K: std::cmp::Eq + std::hash::Hash,
{
    /// Map of values.
    Map(HashMap<K, V>),
    /// Special "__unset" marker from Composer metadata minifier.
    Unset(String),
}

impl<K, V> Default for MapOrUnset<K, V>
where
    K: std::cmp::Eq + std::hash::Hash,
{
    fn default() -> Self {
        Self::Map(HashMap::new())
    }
}

impl<K, V> MapOrUnset<K, V>
where
    K: std::cmp::Eq + std::hash::Hash + Clone,
    V: Clone,
{
    /// Get as map, returning empty map if unset.
    #[must_use]
    pub fn as_map(&self) -> HashMap<K, V> {
        match self {
            Self::Map(m) => m.clone(),
            Self::Unset(_) => HashMap::new(),
        }
    }

    /// Check if this field is unset.
    #[must_use]
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Unset(_))
    }
}

impl<'de, K, V> Deserialize<'de> for MapOrUnset<K, V>
where
    K: std::cmp::Eq + std::hash::Hash + Deserialize<'de>,
    V: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};

        struct MapOrUnsetVisitor<K, V>(std::marker::PhantomData<(K, V)>);

        impl<'de, K, V> Visitor<'de> for MapOrUnsetVisitor<K, V>
        where
            K: std::cmp::Eq + std::hash::Hash + Deserialize<'de>,
            V: Deserialize<'de>,
        {
            type Value = MapOrUnset<K, V>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map or the string \"__unset\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if value == "__unset" {
                    Ok(MapOrUnset::Unset(value.to_string()))
                } else {
                    Err(E::custom("expected \"__unset\" string"))
                }
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                HashMap::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(MapOrUnset::Map)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                // We only allow empty arrays (which PHP/Packagist uses for empty maps)
                if seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom(
                        "expected empty array for empty map, got non-empty array",
                    ));
                }

                Ok(MapOrUnset::Map(HashMap::new()))
            }
        }

        deserializer.deserialize_any(MapOrUnsetVisitor(std::marker::PhantomData))
    }
}

/// Type alias for extra field.
pub type ExtraField = MapOrUnset<String, sonic_rs::Value>;

/// Vec field that can be "__unset" in minified metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum VecOrUnset<T> {
    /// Vector of values.
    Vec(Vec<T>),
    /// Special "__unset" marker from Composer metadata minifier.
    Unset(String),
}

impl<T> Default for VecOrUnset<T> {
    fn default() -> Self {
        Self::Vec(Vec::new())
    }
}

impl<T: Clone> VecOrUnset<T> {
    /// Get as vec, returning empty vec if unset.
    #[must_use]
    pub fn as_vec(&self) -> Vec<T> {
        match self {
            Self::Vec(v) => v.clone(),
            Self::Unset(_) => Vec::new(),
        }
    }

    /// Check if this field is unset.
    #[must_use]
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Unset(_))
    }

    /// Check if this vec is empty (either actually empty or unset).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Vec(v) => v.is_empty(),
            Self::Unset(_) => true,
        }
    }
}

impl<'de, T> Deserialize<'de> for VecOrUnset<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};

        struct VecOrUnsetVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T> Visitor<'de> for VecOrUnsetVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = VecOrUnset<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a sequence or the string \"__unset\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if value == "__unset" {
                    Ok(VecOrUnset::Unset(value.to_string()))
                } else {
                    Err(E::custom("expected \"__unset\" string"))
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                Vec::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
                    .map(VecOrUnset::Vec)
            }
        }

        deserializer.deserialize_any(VecOrUnsetVisitor(std::marker::PhantomData))
    }
}

/// Value that can be a struct or the special "__unset" marker in minified metadata.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ValueOrUnset<T> {
    /// Actual value.
    Value(T),
    /// Special "__unset" marker from Composer metadata minifier.
    Unset(String),
}

impl<T: Default> Default for ValueOrUnset<T> {
    fn default() -> Self {
        Self::Value(T::default())
    }
}

impl<T: Clone> ValueOrUnset<T> {
    /// Get the value, returning default if unset.
    #[must_use]
    pub fn value_or_default(&self) -> T
    where
        T: Default,
    {
        match self {
            Self::Value(v) => v.clone(),
            Self::Unset(_) => T::default(),
        }
    }

    /// Check if this field is unset.
    #[must_use]
    pub fn is_unset(&self) -> bool {
        matches!(self, Self::Unset(_))
    }
}

impl<'de, T> Deserialize<'de> for ValueOrUnset<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};

        struct ValueOrUnsetVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T> Visitor<'de> for ValueOrUnsetVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = ValueOrUnset<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a value or the string \"__unset\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if value == "__unset" {
                    Ok(ValueOrUnset::Unset(value.to_string()))
                } else {
                    Err(E::custom("expected \"__unset\" string"))
                }
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(ValueOrUnset::Value)
            }
        }

        deserializer.deserialize_any(ValueOrUnsetVisitor(std::marker::PhantomData))
    }
}

/// Root packages.json response.
#[derive(Debug, Clone, Deserialize)]
pub struct PackagesJson {
    /// Provider includes for incremental metadata.
    #[serde(default, rename = "provider-includes")]
    pub provider_includes: HashMap<String, ProviderInclude>,
    /// Metadata URL pattern.
    #[serde(rename = "metadata-url")]
    pub metadata_url: Option<String>,
    /// Available packages (direct) - can be empty array or map.
    #[serde(default)]
    pub packages: PackagesField,
    /// Search URL.
    pub search: Option<String>,
    /// Notification URL.
    #[serde(rename = "notify-batch")]
    pub notify_batch: Option<String>,
    /// Whether minified.
    #[serde(default)]
    pub minified: Option<String>,
}

/// Provider include entry.
#[derive(Debug, Clone, Deserialize)]
pub struct ProviderInclude {
    /// SHA256 hash of the file.
    pub sha256: String,
}

/// Package metadata response (p2 API).
#[derive(Debug, Clone, Deserialize)]
pub struct PackageMetadataResponse {
    /// Package versions by name.
    pub packages: HashMap<String, Vec<PackageVersionJson>>,
    /// Minified indicator.
    #[serde(default)]
    pub minified: Option<String>,
}

/// Package version JSON from Packagist API.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageVersionJson {
    /// Package name.
    #[serde(default)]
    pub name: String,
    /// Version string.
    pub version: String,
    /// Normalized version.
    #[serde(default, rename = "version_normalized")]
    pub version_normalized: Option<String>,
    /// Package description.
    #[serde(default)]
    pub description: String,
    /// Package type (library, project, etc.).
    #[serde(default, rename = "type")]
    pub package_type: Option<String>,
    /// Keywords/tags.
    #[serde(default)]
    pub keywords: VecOrUnset<String>,
    /// Homepage URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// License(s).
    #[serde(default)]
    pub license: LicenseValue,
    /// Authors.
    #[serde(default)]
    pub authors: VecOrUnset<AuthorJson>,
    /// Repository/source info.
    #[serde(default)]
    pub source: Option<SourceJson>,
    /// Distribution info.
    #[serde(default)]
    pub dist: Option<DistJson>,
    /// Required dependencies.
    #[serde(default)]
    pub require: Option<MapOrUnset<String, String>>,
    /// Development dependencies.
    #[serde(default, rename = "require-dev")]
    pub require_dev: Option<MapOrUnset<String, String>>,
    /// Suggested packages.
    #[serde(default)]
    pub suggest: Option<MapOrUnset<String, String>>,
    /// Conflicting packages.
    #[serde(default)]
    pub conflict: Option<MapOrUnset<String, String>>,
    /// Provided virtual packages.
    #[serde(default)]
    pub provide: Option<MapOrUnset<String, String>>,
    /// Replaced packages.
    #[serde(default)]
    pub replace: Option<MapOrUnset<String, String>>,
    /// Autoload configuration.
    #[serde(default)]
    pub autoload: ValueOrUnset<AutoloadJson>,
    /// Dev autoload configuration.
    #[serde(default, rename = "autoload-dev")]
    pub autoload_dev: ValueOrUnset<AutoloadJson>,
    /// Extra metadata.
    #[serde(default)]
    pub extra: ExtraField,
    /// Time (release date).
    #[serde(default)]
    pub time: Option<String>,
    /// Whether abandoned.
    #[serde(default)]
    pub abandoned: AbandonedValue,
    /// Notification URL.
    #[serde(default, rename = "notification-url")]
    pub notification_url: Option<String>,
    /// Minimum stability.
    #[serde(default, rename = "minimum-stability")]
    pub minimum_stability: Option<String>,
    /// Support info.
    #[serde(default)]
    pub support: MapOrUnset<String, String>,
    /// Funding info.
    #[serde(default)]
    pub funding: VecOrUnset<FundingJson>,
}

/// License value can be a string or array of strings.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(untagged)]
pub enum LicenseValue {
    /// Single license.
    Single(String),
    /// Multiple licenses.
    Multiple(Vec<String>),
    /// No license specified.
    #[default]
    None,
}

impl LicenseValue {
    /// Convert to vector of licenses.
    #[must_use]
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s.clone()],
            Self::Multiple(v) => v.clone(),
            Self::None => vec![],
        }
    }
}

/// Abandoned value can be boolean or replacement package name.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AbandonedValue {
    /// Boolean abandoned flag.
    Boolean(bool),
    /// Replacement package name.
    Replacement(String),
    /// Not abandoned.
    #[default]
    None,
}

impl AbandonedValue {
    /// Check if package is abandoned.
    #[must_use]
    pub fn is_abandoned(&self) -> bool {
        match self {
            Self::Boolean(b) => *b,
            Self::Replacement(_) => true,
            Self::None => false,
        }
    }

    /// Get replacement package if specified.
    #[must_use]
    pub fn replacement(&self) -> Option<&str> {
        match self {
            Self::Replacement(s) => Some(s),
            _ => None,
        }
    }
}

/// Author information.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AuthorJson {
    /// Author name.
    #[serde(default)]
    pub name: String,
    /// Author email.
    #[serde(default)]
    pub email: Option<String>,
    /// Author homepage.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Author role.
    #[serde(default)]
    pub role: Option<String>,
}

/// Source (VCS) information.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceJson {
    /// VCS type (git, svn, etc.).
    #[serde(rename = "type")]
    pub source_type: String,
    /// Repository URL.
    pub url: String,
    /// Reference (branch, tag, commit).
    pub reference: String,
}

/// Distribution (archive) information.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DistJson {
    /// Archive type (zip, tar).
    #[serde(rename = "type")]
    pub archive_type: String,
    /// Download URL.
    pub url: String,
    /// SHA checksum.
    #[serde(default)]
    pub shasum: Option<String>,
    /// Reference.
    #[serde(default)]
    pub reference: Option<String>,
}

/// Autoload configuration.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AutoloadJson {
    /// PSR-4 autoloading.
    #[serde(default, rename = "psr-4")]
    pub psr4: MapOrUnset<String, PsrValue>,
    /// PSR-0 autoloading.
    #[serde(default, rename = "psr-0")]
    pub psr0: MapOrUnset<String, PsrValue>,
    /// Classmap.
    #[serde(default)]
    pub classmap: ClassmapOrUnset,
    /// Files to include.
    #[serde(default)]
    pub files: VecOrUnset<String>,
    /// Excluded from classmap.
    #[serde(default, rename = "exclude-from-classmap")]
    pub exclude_from_classmap: VecOrUnset<String>,
}

/// PSR autoload value can be string or array.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PsrValue {
    /// Multiple paths.
    Multiple(Vec<String>),
    /// Single path.
    Single(String),
}

impl PsrValue {
    /// Convert to vector of paths.
    #[must_use]
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s.clone()],
            Self::Multiple(v) => v.clone(),
        }
    }
}

/// Classmap value can be string, array of strings, or nested array (legacy format).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ClassmapValue {
    /// Single path.
    Single(String),
    /// Array of paths.
    Array(Vec<String>),
    /// Nested array (legacy Laravel 4.x format).
    NestedArray(Vec<Vec<String>>),
}

impl ClassmapValue {
    /// Convert to vector of paths, flattening nested arrays.
    #[must_use]
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Single(s) => vec![s.clone()],
            Self::Array(v) => v.clone(),
            Self::NestedArray(nested) => nested.iter().flatten().cloned().collect(),
        }
    }
}

/// Classmap field that can be various formats or "__unset".
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum ClassmapOrUnset {
    /// Classmap values.
    Values(Vec<ClassmapValue>),
    /// Special "__unset" marker.
    Unset(String),
}

impl Default for ClassmapOrUnset {
    fn default() -> Self {
        Self::Values(Vec::new())
    }
}

impl ClassmapOrUnset {
    /// Convert to flat vector of paths.
    #[must_use]
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            Self::Values(vals) => vals.iter().flat_map(|v| v.to_vec()).collect(),
            Self::Unset(_) => Vec::new(),
        }
    }

    /// Check if empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Values(v) => v.is_empty(),
            Self::Unset(_) => true,
        }
    }
}

impl<'de> Deserialize<'de> for ClassmapOrUnset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, Visitor};

        struct ClassmapOrUnsetVisitor;

        impl<'de> Visitor<'de> for ClassmapOrUnsetVisitor {
            type Value = ClassmapOrUnset;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an array or the string \"__unset\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                if value == "__unset" {
                    Ok(ClassmapOrUnset::Unset(value.to_string()))
                } else {
                    Err(E::custom("expected \"__unset\" string"))
                }
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                Vec::<ClassmapValue>::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
                    .map(ClassmapOrUnset::Values)
            }
        }

        deserializer.deserialize_any(ClassmapOrUnsetVisitor)
    }
}

/// Funding information.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FundingJson {
    /// Funding type (github, patreon, etc.).
    #[serde(rename = "type")]
    pub funding_type: String,
    /// Funding URL.
    pub url: String,
}

/// Search response.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    /// Search results.
    pub results: Vec<SearchResult>,
    /// Total results.
    #[serde(default)]
    pub total: u64,
    /// Next page URL.
    #[serde(default)]
    pub next: Option<String>,
}

/// Individual search result.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResult {
    /// Package name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Package URL on Packagist.
    #[serde(default)]
    pub url: String,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Download count.
    #[serde(default)]
    pub downloads: u64,
    /// Favorites/stars count.
    #[serde(default)]
    pub favers: u64,
    /// Whether abandoned.
    #[serde(default)]
    pub abandoned: AbandonedValue,
}

/// Security advisory response.
#[derive(Debug, Clone, Deserialize)]
pub struct SecurityAdvisoriesResponse {
    /// Advisories by package name.
    pub advisories: HashMap<String, Vec<SecurityAdvisory>>,
}

/// Individual security advisory.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityAdvisory {
    /// Advisory ID.
    #[serde(rename = "advisoryId")]
    pub advisory_id: String,
    /// Package name.
    #[serde(rename = "packageName")]
    pub package_name: String,
    /// Title.
    pub title: String,
    /// Link to advisory.
    pub link: String,
    /// CVE ID if available.
    #[serde(default)]
    pub cve: Option<String>,
    /// Affected versions (Composer constraint).
    #[serde(rename = "affectedVersions")]
    pub affected_versions: String,
    /// Advisory sources.
    #[serde(default)]
    pub sources: Vec<AdvisorySource>,
    /// When reported.
    #[serde(rename = "reportedAt")]
    pub reported_at: String,
    /// Composer repository.
    #[serde(default, rename = "composerRepository")]
    pub composer_repository: Option<String>,
    /// Severity (critical, high, medium, low).
    #[serde(default)]
    pub severity: Option<String>,
}

/// Advisory source.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdvisorySource {
    /// Source name.
    pub name: String,
    /// Remote ID.
    #[serde(rename = "remoteId")]
    pub remote_id: String,
}

/// Changes response for tracking updates.
#[derive(Debug, Clone, Deserialize)]
pub struct ChangesResponse {
    /// Actions to perform.
    #[serde(default)]
    pub actions: Vec<ChangeAction>,
    /// Current timestamp.
    #[serde(default)]
    pub timestamp: Option<u64>,
    /// Error message if any.
    #[serde(default)]
    pub error: Option<String>,
}

/// Individual change action.
#[derive(Debug, Clone, Deserialize)]
pub struct ChangeAction {
    /// Action type (update, delete, resync).
    #[serde(rename = "type")]
    pub action_type: String,
    /// Package name.
    pub package: String,
    /// Unix timestamp.
    pub time: u64,
}

/// Package list response.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PackageListResponse {
    /// Simple list of package names.
    Simple {
        #[serde(rename = "packageNames")]
        package_names: Vec<String>,
    },
    /// With additional fields.
    WithFields {
        #[serde(default, rename = "package")]
        packages: HashMap<String, PackageListEntry>,
    },
}

/// Package list entry with fields.
#[derive(Debug, Clone, Deserialize)]
pub struct PackageListEntry {
    /// Package type.
    #[serde(default, rename = "type")]
    pub package_type: Option<String>,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Abandoned status.
    #[serde(default)]
    pub abandoned: AbandonedValue,
}

/// Popular packages response.
#[derive(Debug, Clone, Deserialize)]
pub struct PopularPackagesResponse {
    /// Popular packages.
    pub packages: Vec<PopularPackage>,
    /// Total count.
    #[serde(default)]
    pub total: u64,
    /// Next page URL.
    #[serde(default)]
    pub next: Option<String>,
}

/// Popular package entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PopularPackage {
    /// Package name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Package URL.
    #[serde(default)]
    pub url: String,
    /// Download count.
    #[serde(default)]
    pub downloads: u64,
    /// Favorites count.
    #[serde(default)]
    pub favers: u64,
}

/// Statistics response.
#[derive(Debug, Clone, Deserialize)]
pub struct StatisticsResponse {
    /// Total stats.
    pub totals: TotalStats,
}

/// Total statistics.
#[derive(Debug, Clone, Deserialize)]
pub struct TotalStats {
    /// Total downloads.
    pub downloads: u64,
}

// Conversion functions

impl PackageVersionJson {
    /// Convert to core Package type.
    ///
    /// # Arguments
    /// * `package_id` - The package identifier to use.
    ///
    /// # Returns
    /// `Option<Package>` - The converted package, or `None` if version parsing fails.
    #[must_use]
    pub fn to_package(&self, package_id: &PackageId) -> Option<Package> {
        // Parse version, stripping leading 'v'
        let version_str = self.version.trim_start_matches('v');
        let version = Version::parse(version_str).ok()?;

        let mut pkg = Package::new(package_id.clone(), version);
        pkg.description = self.description.clone();
        pkg.license = self.license.to_vec();

        // Parse package type
        pkg.package_type = match self.package_type.as_deref() {
            Some("library") | None => PackageType::Library,
            Some("project") => PackageType::Project,
            Some("metapackage") => PackageType::Metapackage,
            Some("composer-plugin") => PackageType::ComposerPlugin,
            _ => PackageType::Library,
        };

        // Parse dependencies
        let require = self
            .require
            .as_ref()
            .map(|x| x.as_map())
            .unwrap_or_default();
        for (name, constraint) in &require {
            if let Some(dep_id) = PackageId::parse(name) {
                pkg.require.push(Dependency::new(
                    dep_id,
                    VersionConstraint::new(constraint.clone()),
                ));
            }
        }

        let require_dev = self
            .require_dev
            .as_ref()
            .map(|x| x.as_map())
            .unwrap_or_default();
        for (name, constraint) in &require_dev {
            if let Some(dep_id) = PackageId::parse(name) {
                pkg.require_dev.push(Dependency::dev(
                    dep_id,
                    VersionConstraint::new(constraint.clone()),
                ));
            }
        }

        // Parse dist
        if let Some(ref dist) = self.dist {
            if let Ok(url) = Url::parse(&dist.url) {
                pkg.dist = Some(PackageSource::Dist {
                    url,
                    archive_type: dist.archive_type.clone(),
                    shasum: dist.shasum.clone(),
                });
            }
        }

        // Parse source
        if let Some(ref source) = self.source {
            if let Ok(url) = Url::parse(&source.url) {
                pkg.source = Some(PackageSource::Git {
                    url,
                    reference: source.reference.clone(),
                });
            }
        }

        // Parse authors
        for author in &self.authors.as_vec() {
            pkg.authors.push(Author {
                name: author.name.clone(),
                email: author.email.clone(),
                homepage: author.homepage.clone(),
            });
        }

        // Convert autoload
        let autoload = self.autoload.value_or_default();
        for (namespace, paths) in &autoload.psr4.as_map() {
            let mut path_map = HashMap::new();
            for path in paths.to_vec() {
                path_map.insert(namespace.clone(), path);
            }
            pkg.autoload.insert("psr-4".to_string(), path_map);
        }

        Some(pkg)
    }
}

/// Expand minified package versions using Composer metadata minifier algorithm.
///
/// See: <https://github.com/composer/metadata-minifier>
pub fn expand_minified_versions(versions: &[PackageVersionJson]) -> Vec<PackageVersionJson> {
    if versions.is_empty() {
        return vec![];
    }

    let mut expanded = Vec::with_capacity(versions.len());
    let mut last: Option<PackageVersionJson> = None;

    for version in versions {
        let mut expanded_version = version.clone();

        if let Some(ref prev) = last {
            // Inherit missing fields from previous version
            if expanded_version.name.is_empty() {
                expanded_version.name = prev.name.clone();
            }
            if expanded_version.description.is_empty() {
                expanded_version.description = prev.description.clone();
            }
            if expanded_version.package_type.is_none() {
                expanded_version.package_type = prev.package_type.clone();
            }
            if expanded_version.homepage.is_none() {
                expanded_version.homepage = prev.homepage.clone();
            }
            if expanded_version.authors.is_empty() {
                expanded_version.authors = prev.authors.clone();
            }
            if matches!(expanded_version.license, LicenseValue::None) {
                expanded_version.license = prev.license.clone();
            }
            if expanded_version.keywords.is_empty() {
                expanded_version.keywords = prev.keywords.clone();
            }
            if expanded_version.support.is_unset() {
                expanded_version.support = prev.support.clone();
            }
            if expanded_version.funding.is_empty() {
                expanded_version.funding = prev.funding.clone();
            }
            if expanded_version.autoload.is_unset() {
                expanded_version.autoload = prev.autoload.clone();
            }
            if expanded_version.autoload_dev.is_unset() {
                expanded_version.autoload_dev = prev.autoload_dev.clone();
            }
            // CRITICAL: Inherit dependency fields for proper resolution
            // If field is None (missing from JSON), inherit from previous.
            // If field is Some (present but empty or unset), use it as is.
            if expanded_version.require.is_none() {
                expanded_version.require = prev.require.clone();
            }
            if expanded_version.require_dev.is_none() {
                expanded_version.require_dev = prev.require_dev.clone();
            }
            if expanded_version.conflict.is_none() {
                expanded_version.conflict = prev.conflict.clone();
            }
            if expanded_version.provide.is_none() {
                expanded_version.provide = prev.provide.clone();
            }
            if expanded_version.replace.is_none() {
                expanded_version.replace = prev.replace.clone();
            }
            if expanded_version.suggest.is_none() {
                expanded_version.suggest = prev.suggest.clone();
            }
        }

        last = Some(expanded_version.clone());
        expanded.push(expanded_version);
    }

    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_license_value() {
        let single: LicenseValue = sonic_rs::from_str(r#""MIT""#).unwrap();
        assert_eq!(single.to_vec(), vec!["MIT"]);

        let multiple: LicenseValue = sonic_rs::from_str(r#"["MIT", "Apache-2.0"]"#).unwrap();
        assert_eq!(multiple.to_vec(), vec!["MIT", "Apache-2.0"]);
    }

    #[test]
    fn test_abandoned_value() {
        let not_abandoned: AbandonedValue = sonic_rs::from_str("false").unwrap();
        assert!(!not_abandoned.is_abandoned());

        let abandoned: AbandonedValue = sonic_rs::from_str("true").unwrap();
        assert!(abandoned.is_abandoned());

        let replacement: AbandonedValue = sonic_rs::from_str(r#""symfony/console""#).unwrap();
        assert!(replacement.is_abandoned());
        assert_eq!(replacement.replacement(), Some("symfony/console"));
    }

    #[test]
    fn test_psr_value() {
        let single: PsrValue = sonic_rs::from_str(r#""src/""#).unwrap();
        assert_eq!(single.to_vec(), vec!["src/"]);

        let multiple: PsrValue = sonic_rs::from_str(r#"["src/", "lib/"]"#).unwrap();
        assert_eq!(multiple.to_vec(), vec!["src/", "lib/"]);
    }

    #[test]
    fn test_package_version_to_package() {
        let json = r#"{
            "name": "vendor/package",
            "version": "1.0.0",
            "description": "Test package",
            "type": "library",
            "license": "MIT",
            "require": {
                "php": ">=8.0",
                "psr/log": "^3.0"
            }
        }"#;

        let version: PackageVersionJson = sonic_rs::from_str(json).unwrap();
        let package_id = PackageId::parse("vendor/package").unwrap();
        let package = version.to_package(&package_id).unwrap();

        assert_eq!(package.version.to_string(), "1.0.0");
        assert_eq!(package.description, "Test package");
        assert_eq!(package.license, vec!["MIT"]);
        // PHP constraint is not a real package dependency
        assert_eq!(package.require.len(), 1);
    }

    #[test]
    fn test_expand_minified() {
        let minified: Vec<PackageVersionJson> = sonic_rs::from_str(
            r#"[
            {
                "name": "vendor/pkg",
                "version": "1.0.0",
                "description": "A package",
                "license": "MIT"
            },
            {
                "version": "1.1.0"
            }
        ]"#,
        )
        .unwrap();

        let expanded = expand_minified_versions(&minified);
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[1].name, "vendor/pkg");
        assert_eq!(expanded[1].description, "A package");
    }

    #[test]
    fn test_map_or_unset_empty_array() {
        use sonic_rs::from_str;

        #[derive(Debug, Deserialize)]
        struct TestStruct {
            #[serde(default)]
            pub require: Option<MapOrUnset<String, String>>,
        }

        // Case 1: Empty Array [] - Common in PHP/Packagist for empty maps
        let json_arr = r#"{ "require": [] }"#;
        let res_arr: Result<TestStruct, _> = from_str(json_arr);

        match res_arr {
            Ok(v) => {
                // We expect it to FAIL or return None if it can't handle [],
                // OR return Map(empty) if it can.
                println!("Parsed []: {:?}", v.require);
                // If it returns None, that triggers INHERITANCE in our main logic,
                // which is WRONG if the intent was "empty dependencies".
                // Use assertions to verify behavior.
            }
            Err(e) => println!("Error parsing []: {}", e),
        }

        // Case 2: Object {}
        let json_obj = r#"{ "require": {} }"#;
        let res_obj: Result<TestStruct, _> = from_str(json_obj);
        match res_obj {
            Ok(v) => println!("Parsed {{}}: {:?}", v.require),
            Err(e) => println!("Error parsing {{}}: {}", e),
        }
    }
}
