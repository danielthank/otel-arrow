use openzl::{OpenZL, Profile, Compressor, CCtx, DCtx, CParam, MAX_FORMAT_VERSION};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

/// Result type for compression operations
pub type CompressionResult = Result<Vec<u8>, Box<dyn std::error::Error>>;

/// Trait for IPC-layer compression
pub trait IpcCompressor {
    /// Compress data at the IPC layer (Arrow IPC data)
    fn compress(&self, data: &[u8]) -> CompressionResult;
}

/// Zstd IPC compressor (for Method 1a, 1b)
pub struct ZstdIpcCompressor {
    level: i32,
}

impl ZstdIpcCompressor {
    pub fn new(level: i32) -> Self {
        Self { level }
    }
}

impl IpcCompressor for ZstdIpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        let compressed = zstd::encode_all(data, self.level)?;
        Ok(compressed)
    }
}

/// OpenZL IPC compressor (for Method 2a, 2b)
pub struct OpenZLIpcCompressor {
    openzl: OpenZL,
    profile: Profile,
}

impl OpenZLIpcCompressor {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let openzl = OpenZL::new()
            .map_err(|e| format!("Failed to initialize OpenZL: {}", e))?;
        Ok(Self {
            openzl,
            profile: Profile::Generic,
        })
    }
}

impl IpcCompressor for OpenZLIpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        let compressed = self.openzl.compress(data, self.profile)
            .map_err(|e| format!("OpenZL compression failed: {}", e))?;
        Ok(compressed)
    }
}

/// Custom OpenZL IPC compressor with loaded .zl file (for Method 4)
pub struct CustomOpenZLIpcCompressor {
    compressor: Arc<Compressor>,
    cctx: RefCell<CCtx>,
    _dctx: RefCell<DCtx>, // For verification if needed
}

impl CustomOpenZLIpcCompressor {
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

        // Create compression context
        let mut cctx = CCtx::new()
            .map_err(|e| format!("Failed to create CCtx: {}", e))?;

        // Attach loaded compressor
        cctx.attach_compressor(Arc::clone(&compressor))
            .map_err(|e| format!("Failed to attach compressor: {}", e))?;

        // Set format version
        cctx.set_parameter(CParam::FormatVersion, MAX_FORMAT_VERSION)
            .map_err(|e| format!("Failed to set format version: {}", e))?;

        // Create decompression context
        let dctx = DCtx::new()
            .map_err(|e| format!("Failed to create DCtx: {}", e))?;

        Ok(Self {
            compressor,
            cctx: RefCell::new(cctx),
            _dctx: RefCell::new(dctx),
        })
    }
}

impl IpcCompressor for CustomOpenZLIpcCompressor {
    fn compress(&self, data: &[u8]) -> CompressionResult {
        // Use RefCell to get mutable access to CCtx
        let mut cctx = self.cctx.borrow_mut();
        cctx.compress(data)
            .map_err(|e| format!("Custom OpenZL compression failed: {}", e).into())
    }
}
