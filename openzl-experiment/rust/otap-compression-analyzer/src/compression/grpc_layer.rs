use openzl::{OpenZL, Profile, Compressor, CCtx, CParam, MAX_FORMAT_VERSION};
use std::path::Path;
use std::sync::Arc;

/// Result type for compression operations
pub type CompressionResult = Result<Vec<u8>, Box<dyn std::error::Error>>;

/// Trait for gRPC-layer compression
pub trait GrpcCompressor {
    /// Compress data at the gRPC layer
    fn compress(&self, data: &[u8]) -> CompressionResult;
}

/// Zstd gRPC compressor (for Method 1a, 2a)
pub struct ZstdGrpcCompressor {
    level: i32,
}

impl ZstdGrpcCompressor {
    pub fn new(level: i32) -> Self {
        Self { level }
    }
}

impl GrpcCompressor for ZstdGrpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        let compressed = zstd::encode_all(data, self.level)?;
        Ok(compressed)
    }
}

/// OpenZL gRPC compressor (for Method 3)
pub struct OpenZLGrpcCompressor {
    openzl: OpenZL,
    profile: Profile,
}

impl OpenZLGrpcCompressor {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let openzl = OpenZL::new()
            .map_err(|e| format!("Failed to initialize OpenZL: {}", e))?;
        Ok(Self {
            openzl,
            profile: Profile::Generic,
        })
    }
}

impl GrpcCompressor for OpenZLGrpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        let compressed = self.openzl.compress(data, self.profile)
            .map_err(|e| format!("OpenZL compression failed: {}", e))?;
        Ok(compressed)
    }
}

/// Custom OpenZL gRPC compressor with loaded .zl file (for Method 4)
pub struct CustomOpenZLGrpcCompressor {
    compressor: Arc<Compressor>,
}

impl CustomOpenZLGrpcCompressor {
    /// Create a new custom OpenZL compressor by loading a .zl file
    ///
    /// # Arguments
    ///
    /// * `compressor_path` - Path to the .zl file containing trained compressor
    pub fn new(compressor_path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        // Load compressor from .zl file
        let compressor = Compressor::load_from_file(compressor_path)
            .map_err(|e| format!("Failed to load compressor: {}", e))?;
        let compressor = Arc::new(compressor);

        Ok(Self {
            compressor,
        })
    }
}

impl GrpcCompressor for CustomOpenZLGrpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        // Create fresh CCtx for each compression (matching C++ reference implementation)
        // This is necessary because OpenZL CCtx maintains state that must be reset between compressions
        let mut cctx = CCtx::new()
            .map_err(|e| format!("Failed to create CCtx: {}", e))?;

        // Attach compressor
        cctx.attach_compressor(Arc::clone(&self.compressor))
            .map_err(|e| format!("Failed to attach compressor: {}", e))?;

        // Set format version (required before compression)
        cctx.set_parameter(CParam::FormatVersion, MAX_FORMAT_VERSION)
            .map_err(|e| format!("Failed to set format version: {}", e))?;

        // Compress
        cctx.compress(data)
            .map_err(|e| format!("Custom OpenZL compression failed: {}", e).into())
    }
}
