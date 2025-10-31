//! Safe Rust wrapper for OpenZL compression
//!
//! This implementation uses direct FFI bindings to the OpenZL C library for optimal performance.
//!
//! # Architecture Note
//!
//! OpenZL uses a graph-based compression model where users either:
//! 1. Use pre-built profiles/graphs (like `ZL_GRAPH_COMPRESS_GENERIC`)
//! 2. Train custom compressors for specific data formats
//!
//! This wrapper uses simple one-pass compression for rapid prototyping.
//! A future version could support custom trained compressors.

use openzl_sys as ffi;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpenZLError {
    #[error("Compression failed with error code: {0}")]
    CompressionFailed(u32),

    #[error("Decompression failed with error code: {0}")]
    DecompressionFailed(u32),

    #[error("Failed to get decompressed size, error code: {0}")]
    DecompressSizeFailed(u32),

    #[error("Buffer too small: need {needed} bytes, have {available}")]
    BufferTooSmall { needed: usize, available: usize },

    #[error("Compressor creation failed")]
    CompressorCreateFailed,

    #[error("Compressor deserialization failed with error code: {0}")]
    DeserializationFailed(u32),

    #[error("Failed to create compression context")]
    CCtxCreateFailed,

    #[error("Failed to create decompression context")]
    DCtxCreateFailed,

    #[error("Failed to attach compressor with error code: {0}")]
    AttachCompressorFailed(u32),

    #[error("Failed to set parameter with error code: {0}")]
    SetParameterFailed(u32),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, OpenZLError>;

/// OpenZL compression profile/graph
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Generic compression (uses ZL_GRAPH_COMPRESS_GENERIC)
    Generic,
}

/// Calculate the maximum size needed for compression output
///
/// This is a very conservative upper bound from OpenZL: `(size * 2) + 512 + 8`
#[inline]
fn compress_bound(size: usize) -> usize {
    (size * 2) + 512 + 8
}

/// Check if a ZL_Report contains an error
#[inline]
unsafe fn is_error(report: ffi::ZL_Report) -> bool {
    // ZL_isError checks if _code != ZL_ErrorCode_no_error
    // SAFETY: Accessing union field _code is safe as it's always initialized
    unsafe { report._code != ffi::ZL_ErrorCode_ZL_ErrorCode_no_error }
}

/// Extract the value from a successful ZL_Report
#[inline]
unsafe fn get_value(report: ffi::ZL_Report) -> usize {
    // SAFETY: _value field is valid when is_error() returns false
    unsafe { report._value._value }
}

/// Extract the error code from a failed ZL_Report
#[inline]
unsafe fn get_error_code(report: ffi::ZL_Report) -> u32 {
    // SAFETY: Accessing union field _code is safe as it's always initialized
    unsafe { report._code }
}

/// Graph function for compressing serial (raw byte) data
///
/// This uses the built-in generic compression graph for serial data.
extern "C" fn serial_graph_fn(compressor: *mut ffi::ZL_Compressor) -> ffi::ZL_GraphID {
    unsafe {
        // Set format version (required by OpenZL)
        let report = ffi::ZL_Compressor_setParameter(
            compressor,
            ffi::ZL_CParam_ZL_CParam_formatVersion,
            ffi::ZL_MAX_FORMAT_VERSION as i32,
        );

        // Check if setting parameter failed
        if is_error(report) {
            // Return invalid graph ID on error
            return ffi::ZL_GraphID { gid: u32::MAX };
        }

        // Return the built-in generic compression graph ID
        // This is the standard graph for general-purpose compression
        ffi::ZL_GraphID {
            gid: ffi::ZL_StandardGraphID_ZL_StandardGraphID_compress_generic,
        }
    }
}

/// OpenZL compressor
pub struct OpenZL;

impl OpenZL {
    /// Create a new OpenZL instance
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// Compress data using the specified profile
    ///
    /// # Arguments
    ///
    /// * `data` - Input data to compress
    /// * `profile` - Compression profile to use
    ///
    /// # Returns
    ///
    /// Compressed data as bytes
    pub fn compress(&self, data: &[u8], _profile: Profile) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            // Calculate maximum compressed size
            let bound = compress_bound(data.len());
            let mut compressed = vec![0u8; bound];

            // Compress using graph function with generic compression
            let report = ffi::ZL_compress_usingGraphFn(
                compressed.as_mut_ptr() as *mut c_void,
                compressed.len(),
                data.as_ptr() as *const c_void,
                data.len(),
                Some(serial_graph_fn),
            );

            if is_error(report) {
                return Err(OpenZLError::CompressionFailed(get_error_code(report)));
            }

            let compressed_size = get_value(report);
            compressed.truncate(compressed_size);
            Ok(compressed)
        }
    }

    /// Decompress OpenZL-compressed data
    ///
    /// # Arguments
    ///
    /// * `compressed_data` - Compressed data
    ///
    /// # Returns
    ///
    /// Decompressed data as bytes
    pub fn decompress(&self, compressed_data: &[u8]) -> Result<Vec<u8>> {
        if compressed_data.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            // Get decompressed size
            let size_report = ffi::ZL_getDecompressedSize(
                compressed_data.as_ptr() as *const c_void,
                compressed_data.len(),
            );

            if is_error(size_report) {
                return Err(OpenZLError::DecompressSizeFailed(get_error_code(
                    size_report,
                )));
            }

            let decompressed_size = get_value(size_report);
            let mut decompressed = vec![0u8; decompressed_size];

            // Decompress
            let report = ffi::ZL_decompress(
                decompressed.as_mut_ptr() as *mut c_void,
                decompressed.len(),
                compressed_data.as_ptr() as *const c_void,
                compressed_data.len(),
            );

            if is_error(report) {
                return Err(OpenZLError::DecompressionFailed(get_error_code(report)));
            }

            Ok(decompressed)
        }
    }
}

impl Default for OpenZL {
    fn default() -> Self {
        Self::new().expect("Failed to initialize OpenZL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_decompress_roundtrip() {
        let openzl = OpenZL::new().expect("Failed to create OpenZL");

        let original_data =
            b"Hello, OpenZL! This is a test of compression and decompression.".repeat(100);

        // Compress
        let compressed = openzl
            .compress(&original_data, Profile::Generic)
            .expect("Compression failed");

        println!(
            "Original: {} bytes, Compressed: {} bytes, Ratio: {:.2}x",
            original_data.len(),
            compressed.len(),
            original_data.len() as f64 / compressed.len() as f64
        );

        // Verify compression actually reduced size (for repeated data)
        assert!(
            compressed.len() < original_data.len(),
            "Compressed size {} should be less than original {}",
            compressed.len(),
            original_data.len()
        );

        // Decompress
        let decompressed = openzl.decompress(&compressed).expect("Decompression failed");

        // Verify data integrity
        assert_eq!(
            original_data,
            decompressed.as_slice(),
            "Decompressed data doesn't match original"
        );
    }

    #[test]
    fn test_small_data() {
        let openzl = OpenZL::new().expect("Failed to create OpenZL");

        let original_data = b"Small data";

        let compressed = openzl
            .compress(original_data, Profile::Generic)
            .expect("Compression failed");

        let decompressed = openzl.decompress(&compressed).expect("Decompression failed");

        assert_eq!(original_data, decompressed.as_slice());
    }

    #[test]
    fn test_empty_data() {
        let openzl = OpenZL::new().expect("Failed to create OpenZL");

        let original_data = b"";

        let compressed = openzl
            .compress(original_data, Profile::Generic)
            .expect("Compression failed");

        assert_eq!(compressed.len(), 0);

        let decompressed = openzl.decompress(&compressed).expect("Decompression failed");

        assert_eq!(original_data, decompressed.as_slice());
    }
}

// ============================================================================
// Stateful Compression with Custom Compressors
// ============================================================================

/// A loaded/trained OpenZL compressor
///
/// This type represents a compressor that can be:
/// - Created fresh (with default configuration)
/// - Loaded from a .zl file (trained/serialized compressor)
/// - Deserialized from bytes
///
/// The compressor can then be attached to a CCtx for stateful compression.
pub struct Compressor {
    ptr: NonNull<ffi::ZL_Compressor>,
}

impl Compressor {
    /// Create a new empty compressor
    pub fn new() -> Result<Self> {
        unsafe {
            let ptr = ffi::ZL_Compressor_create();
            NonNull::new(ptr)
                .map(|ptr| Compressor { ptr })
                .ok_or(OpenZLError::CompressorCreateFailed)
        }
    }

    /// Load a compressor from a .zl file
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the .zl file containing a serialized compressor
    ///
    /// # Example
    ///
    /// ```no_run
    /// use openzl::Compressor;
    /// let compressor = Compressor::load_from_file("trained.zl").unwrap();
    /// ```
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::deserialize(&bytes)
    }

    /// Deserialize a compressor from bytes
    ///
    /// # Arguments
    ///
    /// * `bytes` - Serialized compressor data (contents of a .zl file)
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        unsafe {
            // Create compressor
            let compressor_ptr = ffi::ZL_Compressor_create();
            if compressor_ptr.is_null() {
                return Err(OpenZLError::CompressorCreateFailed);
            }

            // Register OTAP graphs BEFORE deserialization
            let graph_id = ffi::otap_registerOtapCompressionGraph(compressor_ptr);
            if graph_id.gid == 0 || graph_id.gid == u32::MAX {
                ffi::ZL_Compressor_free(compressor_ptr);
                return Err(OpenZLError::CompressorCreateFailed);
            }

            // Create deserializer
            let deserializer = ffi::ZL_CompressorDeserializer_create();
            if deserializer.is_null() {
                ffi::ZL_Compressor_free(compressor_ptr);
                return Err(OpenZLError::CompressorCreateFailed);
            }

            // Deserialize
            let report = ffi::ZL_CompressorDeserializer_deserialize(
                deserializer,
                compressor_ptr,
                bytes.as_ptr() as *const c_void,
                bytes.len(),
            );

            // Free deserializer (no longer needed)
            ffi::ZL_CompressorDeserializer_free(deserializer);

            if is_error(report) {
                ffi::ZL_Compressor_free(compressor_ptr);
                return Err(OpenZLError::DeserializationFailed(get_error_code(report)));
            }

            Ok(Compressor {
                ptr: NonNull::new_unchecked(compressor_ptr),
            })
        }
    }

    /// Get the raw pointer (for internal use)
    #[inline]
    pub(crate) fn as_ptr(&self) -> *mut ffi::ZL_Compressor {
        self.ptr.as_ptr()
    }
}

// Compressor is Send + Sync because OpenZL compressor objects are thread-safe
unsafe impl Send for Compressor {}
unsafe impl Sync for Compressor {}

impl Drop for Compressor {
    fn drop(&mut self) {
        unsafe {
            ffi::ZL_Compressor_free(self.ptr.as_ptr());
        }
    }
}

/// Compression parameter types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CParam {
    /// Format version parameter
    FormatVersion,
}

impl CParam {
    fn to_ffi(&self) -> u32 {
        match self {
            CParam::FormatVersion => ffi::ZL_CParam_ZL_CParam_formatVersion,
        }
    }
}

/// Maximum format version supported
pub const MAX_FORMAT_VERSION: i32 = ffi::ZL_MAX_FORMAT_VERSION as i32;

/// Compression context for stateful compression
///
/// A compression context allows you to:
/// - Attach a loaded/trained compressor
/// - Set compression parameters
/// - Compress data using the attached compressor
///
/// # Example
///
/// ```no_run
/// use openzl::{Compressor, CCtx, CParam, MAX_FORMAT_VERSION};
/// use std::sync::Arc;
///
/// let compressor = Arc::new(Compressor::load_from_file("trained.zl").unwrap());
/// let mut cctx = CCtx::new().unwrap();
/// cctx.attach_compressor(compressor).unwrap();
/// cctx.set_parameter(CParam::FormatVersion, MAX_FORMAT_VERSION).unwrap();
///
/// let data = b"Hello, world!";
/// let compressed = cctx.compress(data).unwrap();
/// ```
pub struct CCtx {
    ptr: NonNull<ffi::ZL_CCtx>,
    _compressor: Option<Arc<Compressor>>,
}

impl CCtx {
    /// Create a new compression context
    pub fn new() -> Result<Self> {
        unsafe {
            let ptr = ffi::ZL_CCtx_create();
            NonNull::new(ptr)
                .map(|ptr| CCtx {
                    ptr,
                    _compressor: None,
                })
                .ok_or(OpenZLError::CCtxCreateFailed)
        }
    }

    /// Attach a compressor to this context
    ///
    /// The compressor will be referenced and kept alive for the lifetime of this context.
    pub fn attach_compressor(&mut self, compressor: Arc<Compressor>) -> Result<()> {
        unsafe {
            let report = ffi::ZL_CCtx_refCompressor(self.ptr.as_ptr(), compressor.as_ptr());

            if is_error(report) {
                return Err(OpenZLError::AttachCompressorFailed(get_error_code(report)));
            }

            self._compressor = Some(compressor);
            Ok(())
        }
    }

    /// Set a compression parameter
    pub fn set_parameter(&mut self, param: CParam, value: i32) -> Result<()> {
        unsafe {
            let report = ffi::ZL_CCtx_setParameter(self.ptr.as_ptr(), param.to_ffi(), value);

            if is_error(report) {
                return Err(OpenZLError::SetParameterFailed(get_error_code(report)));
            }

            Ok(())
        }
    }

    /// Compress data using the attached compressor
    ///
    /// # Panics
    ///
    /// Panics if no compressor has been attached via `attach_compressor`.
    pub fn compress(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let bound = compress_bound(data.len());
            let mut compressed = vec![0u8; bound];

            let report = ffi::ZL_CCtx_compress(
                self.ptr.as_ptr(),
                compressed.as_mut_ptr() as *mut c_void,
                compressed.len(),
                data.as_ptr() as *const c_void,
                data.len(),
            );

            if is_error(report) {
                return Err(OpenZLError::CompressionFailed(get_error_code(report)));
            }

            let compressed_size = get_value(report);
            compressed.truncate(compressed_size);
            Ok(compressed)
        }
    }
}

// CCtx is Send because OpenZL contexts are thread-safe (though not Sync - not shared between threads)
unsafe impl Send for CCtx {}

impl Drop for CCtx {
    fn drop(&mut self) {
        unsafe {
            ffi::ZL_CCtx_free(self.ptr.as_ptr());
        }
    }
}

/// Decompression context
///
/// # Example
///
/// ```no_run
/// use openzl::DCtx;
///
/// let mut dctx = DCtx::new().unwrap();
/// let decompressed = dctx.decompress(&compressed_data).unwrap();
/// ```
pub struct DCtx {
    ptr: NonNull<ffi::ZL_DCtx>,
}

impl DCtx {
    /// Create a new decompression context
    pub fn new() -> Result<Self> {
        unsafe {
            let ptr = ffi::ZL_DCtx_create();
            NonNull::new(ptr)
                .map(|ptr| DCtx { ptr })
                .ok_or(OpenZLError::DCtxCreateFailed)
        }
    }

    /// Decompress data
    pub fn decompress(&mut self, compressed: &[u8]) -> Result<Vec<u8>> {
        if compressed.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            // Get decompressed size
            let size_report = ffi::ZL_getDecompressedSize(
                compressed.as_ptr() as *const c_void,
                compressed.len(),
            );

            if is_error(size_report) {
                return Err(OpenZLError::DecompressSizeFailed(get_error_code(
                    size_report,
                )));
            }

            let decompressed_size = get_value(size_report);
            let mut decompressed = vec![0u8; decompressed_size];

            // Decompress
            let report = ffi::ZL_DCtx_decompress(
                self.ptr.as_ptr(),
                decompressed.as_mut_ptr() as *mut c_void,
                decompressed.len(),
                compressed.as_ptr() as *const c_void,
                compressed.len(),
            );

            if is_error(report) {
                return Err(OpenZLError::DecompressionFailed(get_error_code(report)));
            }

            Ok(decompressed)
        }
    }
}

// DCtx is Send because OpenZL contexts are thread-safe (though not Sync - not shared between threads)
unsafe impl Send for DCtx {}

impl Drop for DCtx {
    fn drop(&mut self) {
        unsafe {
            ffi::ZL_DCtx_free(self.ptr.as_ptr());
        }
    }
}
