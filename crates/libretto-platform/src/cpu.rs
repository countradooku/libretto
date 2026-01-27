//! CPU feature detection for SIMD and platform-specific optimizations.
//!
//! Provides runtime detection of CPU features including:
//! - x86_64: SSE4.2, AVX, AVX2, AVX-512
//! - ARM64: NEON, SVE

use once_cell::sync::Lazy;

/// Global CPU features (detected once at startup).
static CPU_FEATURES: Lazy<CpuFeatures> = Lazy::new(CpuFeatures::detect);

/// CPU feature flags for SIMD optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFeatures {
    // x86_64 features
    /// SSE4.2 support.
    pub sse42: bool,
    /// AVX support.
    pub avx: bool,
    /// AVX2 support.
    pub avx2: bool,
    /// AVX-512F (foundation) support.
    pub avx512f: bool,
    /// AVX-512BW (byte/word) support.
    pub avx512bw: bool,
    /// AVX-512VL (vector length) support.
    pub avx512vl: bool,
    /// POPCNT instruction support.
    pub popcnt: bool,
    /// BMI1 (bit manipulation) support.
    pub bmi1: bool,
    /// BMI2 support.
    pub bmi2: bool,
    /// LZCNT (leading zero count) support.
    pub lzcnt: bool,
    /// AES-NI support.
    pub aesni: bool,
    /// PCLMULQDQ support (for CRC).
    pub pclmulqdq: bool,

    // ARM64 features
    /// NEON support (always on for AArch64).
    pub neon: bool,
    /// SVE support.
    pub sve: bool,
    /// SVE2 support.
    pub sve2: bool,
    /// AES support (ARM).
    pub aes_arm: bool,
    /// SHA1 support (ARM).
    pub sha1_arm: bool,
    /// SHA2 support (ARM).
    pub sha2_arm: bool,
    /// CRC32 support (ARM).
    pub crc32_arm: bool,

    // General
    /// Number of logical CPUs.
    pub cpu_count: usize,
    /// Cache line size (bytes).
    pub cache_line_size: usize,
}

impl CpuFeatures {
    /// Detect CPU features at runtime.
    #[must_use]
    pub fn detect() -> Self {
        let cpu_count = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(1);

        #[cfg(target_arch = "x86_64")]
        {
            Self::detect_x86_64(cpu_count)
        }

        #[cfg(target_arch = "aarch64")]
        {
            Self::detect_aarch64(cpu_count)
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::default_features(cpu_count)
        }
    }

    /// Get the global CPU features instance.
    #[must_use]
    pub fn get() -> &'static Self {
        &CPU_FEATURES
    }

    #[cfg(target_arch = "x86_64")]
    fn detect_x86_64(cpu_count: usize) -> Self {
        Self {
            sse42: std::arch::is_x86_feature_detected!("sse4.2"),
            avx: std::arch::is_x86_feature_detected!("avx"),
            avx2: std::arch::is_x86_feature_detected!("avx2"),
            avx512f: std::arch::is_x86_feature_detected!("avx512f"),
            avx512bw: std::arch::is_x86_feature_detected!("avx512bw"),
            avx512vl: std::arch::is_x86_feature_detected!("avx512vl"),
            popcnt: std::arch::is_x86_feature_detected!("popcnt"),
            bmi1: std::arch::is_x86_feature_detected!("bmi1"),
            bmi2: std::arch::is_x86_feature_detected!("bmi2"),
            lzcnt: std::arch::is_x86_feature_detected!("lzcnt"),
            aesni: std::arch::is_x86_feature_detected!("aes"),
            pclmulqdq: std::arch::is_x86_feature_detected!("pclmulqdq"),

            neon: false,
            sve: false,
            sve2: false,
            aes_arm: false,
            sha1_arm: false,
            sha2_arm: false,
            crc32_arm: false,

            cpu_count,
            cache_line_size: Self::detect_cache_line_size(),
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn detect_aarch64(cpu_count: usize) -> Self {
        Self {
            sse42: false,
            avx: false,
            avx2: false,
            avx512f: false,
            avx512bw: false,
            avx512vl: false,
            popcnt: true, // Always available on AArch64
            bmi1: false,
            bmi2: false,
            lzcnt: true, // CLZ is always available on AArch64
            aesni: false,
            pclmulqdq: false,

            neon: true, // Always available on AArch64
            sve: std::arch::is_aarch64_feature_detected!("sve"),
            sve2: std::arch::is_aarch64_feature_detected!("sve2"),
            aes_arm: std::arch::is_aarch64_feature_detected!("aes"),
            sha1_arm: std::arch::is_aarch64_feature_detected!("sha2"), // sha2 includes sha1
            sha2_arm: std::arch::is_aarch64_feature_detected!("sha2"),
            crc32_arm: std::arch::is_aarch64_feature_detected!("crc"),

            cpu_count,
            cache_line_size: Self::detect_cache_line_size(),
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn default_features(cpu_count: usize) -> Self {
        Self {
            sse42: false,
            avx: false,
            avx2: false,
            avx512f: false,
            avx512bw: false,
            avx512vl: false,
            popcnt: false,
            bmi1: false,
            bmi2: false,
            lzcnt: false,
            aesni: false,
            pclmulqdq: false,

            neon: false,
            sve: false,
            sve2: false,
            aes_arm: false,
            sha1_arm: false,
            sha2_arm: false,
            crc32_arm: false,

            cpu_count,
            cache_line_size: 64, // Default assumption
        }
    }

    fn detect_cache_line_size() -> usize {
        // Most modern CPUs use 64-byte cache lines
        // ARM big.LITTLE may have 128-byte lines on big cores
        #[cfg(target_arch = "aarch64")]
        {
            // Apple Silicon uses 128-byte cache lines
            #[cfg(target_os = "macos")]
            {
                128
            }
            #[cfg(not(target_os = "macos"))]
            {
                64
            }
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            64
        }
    }

    /// Check if any SIMD is available.
    #[must_use]
    pub const fn has_simd(&self) -> bool {
        self.sse42 || self.avx || self.avx2 || self.neon
    }

    /// Check if wide SIMD (256-bit+) is available.
    #[must_use]
    pub const fn has_wide_simd(&self) -> bool {
        self.avx2 || self.avx512f || self.sve
    }

    /// Check if AVX-512 is fully available.
    #[must_use]
    pub const fn has_avx512(&self) -> bool {
        self.avx512f && self.avx512bw && self.avx512vl
    }

    /// Check if hardware AES is available.
    #[must_use]
    pub const fn has_aes(&self) -> bool {
        self.aesni || self.aes_arm
    }

    /// Check if hardware SHA is available.
    #[must_use]
    pub const fn has_sha(&self) -> bool {
        self.sha2_arm
    }

    /// Get the best available SIMD capability.
    #[must_use]
    pub const fn best_simd_capability(&self) -> SimdCapability {
        if self.has_avx512() {
            SimdCapability::Avx512
        } else if self.avx2 {
            SimdCapability::Avx2
        } else if self.sve2 {
            SimdCapability::Sve2
        } else if self.sve {
            SimdCapability::Sve
        } else if self.avx {
            SimdCapability::Avx
        } else if self.sse42 {
            SimdCapability::Sse42
        } else if self.neon {
            SimdCapability::Neon
        } else {
            SimdCapability::Scalar
        }
    }

    /// Get optimal vector width for this CPU (in bytes).
    #[must_use]
    pub const fn optimal_vector_width(&self) -> usize {
        if self.has_avx512() {
            64 // 512 bits
        } else if self.avx2 || self.sve {
            32 // 256 bits
        } else if self.sse42 || self.neon {
            16 // 128 bits
        } else {
            8 // 64 bits (scalar)
        }
    }

    /// Get optimal number of parallel operations for SIMD.
    #[must_use]
    pub const fn optimal_simd_parallelism(&self) -> usize {
        // Process multiple vectors in parallel to hide latency
        // Typically 2-4 vectors per iteration is optimal
        if self.has_avx512() {
            2 // 2x 512-bit = 128 bytes per iteration
        } else if self.avx2 {
            4 // 4x 256-bit = 128 bytes per iteration
        } else {
            4 // 4x 128-bit = 64 bytes per iteration
        }
    }
}

/// SIMD capability levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SimdCapability {
    /// No SIMD, scalar operations only.
    Scalar,
    /// SSE4.2 (128-bit).
    Sse42,
    /// AVX (256-bit float, 128-bit int).
    Avx,
    /// AVX2 (256-bit).
    Avx2,
    /// AVX-512 (512-bit).
    Avx512,
    /// ARM NEON (128-bit).
    Neon,
    /// ARM SVE (variable width).
    Sve,
    /// ARM SVE2.
    Sve2,
}

impl SimdCapability {
    /// Get the vector width in bits.
    #[must_use]
    pub const fn vector_bits(self) -> usize {
        match self {
            Self::Scalar => 64,
            Self::Sse42 | Self::Neon => 128,
            Self::Avx | Self::Avx2 | Self::Sve => 256,
            Self::Avx512 | Self::Sve2 => 512,
        }
    }

    /// Get the vector width in bytes.
    #[must_use]
    pub const fn vector_bytes(self) -> usize {
        self.vector_bits() / 8
    }

    /// Check if this capability is at least as good as another.
    #[must_use]
    pub const fn is_at_least(self, other: Self) -> bool {
        (self as usize) >= (other as usize)
    }
}

impl std::fmt::Display for SimdCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scalar => write!(f, "Scalar"),
            Self::Sse42 => write!(f, "SSE4.2"),
            Self::Avx => write!(f, "AVX"),
            Self::Avx2 => write!(f, "AVX2"),
            Self::Avx512 => write!(f, "AVX-512"),
            Self::Neon => write!(f, "NEON"),
            Self::Sve => write!(f, "SVE"),
            Self::Sve2 => write!(f, "SVE2"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_features_detect() {
        let features = CpuFeatures::detect();
        assert!(features.cpu_count >= 1);
        assert!(features.cache_line_size >= 32);
    }

    #[test]
    fn simd_capability_ordering() {
        assert!(SimdCapability::Avx2 > SimdCapability::Avx);
        assert!(SimdCapability::Avx512 > SimdCapability::Avx2);
        assert!(SimdCapability::Scalar < SimdCapability::Sse42);
    }

    #[test]
    fn vector_width() {
        assert_eq!(SimdCapability::Sse42.vector_bits(), 128);
        assert_eq!(SimdCapability::Avx2.vector_bits(), 256);
        assert_eq!(SimdCapability::Avx512.vector_bits(), 512);
        assert_eq!(SimdCapability::Neon.vector_bits(), 128);
    }

    #[test]
    fn best_simd_capability() {
        let features = CpuFeatures::detect();
        let best = features.best_simd_capability();
        // Should return something valid
        assert!(best.vector_bits() >= 64);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_features() {
        let features = CpuFeatures::detect();
        // SSE4.2 is available on all x86_64 CPUs made after 2008
        // Most modern CPUs have at least SSE4.2
        if features.sse42 {
            assert!(features.has_simd());
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn arm_features() {
        let features = CpuFeatures::detect();
        // NEON is always available on AArch64
        assert!(features.neon);
        assert!(features.has_simd());
    }
}
