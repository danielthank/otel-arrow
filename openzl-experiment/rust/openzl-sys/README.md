# openzl-sys

Low-level FFI bindings to Meta's [OpenZL](https://github.com/facebook/openzl) compression library.

## Overview

`openzl-sys` provides raw, unsafe Rust bindings to the OpenZL C API. OpenZL is a format-aware compression framework that uses graph-based compression models to achieve high compression ratios on structured data.

**Note:** These are low-level unsafe bindings. Most users should use the safe `openzl` wrapper crate instead.

## Prerequisites

### 1. Build OpenZL

OpenZL must be built and installed before using these bindings:

```bash
# Clone OpenZL repository
git clone https://github.com/facebook/openzl.git /tmp/openzl-source
cd /tmp/openzl-source

# Build OpenZL (requires CMake 3.20.2+, C++17 compiler, and Git)
mkdir build-install && cd build-install
cmake .. -DCMAKE_BUILD_TYPE=Release

make -j$(nproc)
sudo make install
```

**Note:**
- We use `build-install` instead of `build` because the OpenZL repository already contains a `build/` directory for configuration files.

This will install OpenZL to `/usr/local` with the following structure:
```
/usr/local/
├── include/openzl/  # Headers
└── lib64/          # Libraries (may be lib/ on some systems)
    ├── libopenzl.a
    ├── libopenzl_cpp.a
    └── libzstd.a    # Bundled Zstd dependency
```

### 2. Environment Variables (Optional)

By default, `openzl-sys` looks for OpenZL in `/usr/local`. To use a different location:

```bash
export OPENZL_ROOT=/path/to/openzl
```

## Building

Add to your `Cargo.toml`:

```toml
[dependencies]
openzl-sys = { path = "../openzl-sys" }
```

Build the crate:

```bash
cargo build
```

The build script will:
1. Generate Rust bindings from OpenZL headers using `bindgen`
2. Statically link against `libopenzl.a` and `libzstd.a`
3. Dynamically link the C++ standard library

## Usage

**Warning:** This crate provides raw unsafe FFI bindings. You are responsible for:
- Memory safety
- Proper error handling
- Managing object lifetimes
- Understanding OpenZL's API requirements

### Basic Example

```rust
use openzl_sys as ffi;
use std::ffi::c_void;

unsafe {
    // Example: Get OpenZL version
    let version = ffi::ZL_versionNumber();
    println!("OpenZL version: {}", version);

    // Example: Compress data using graph function
    extern "C" fn graph_fn(compressor: *mut ffi::ZL_Compressor) -> ffi::ZL_GraphID {
        // Set format version
        let report = ffi::ZL_Compressor_setParameter(
            compressor,
            ffi::ZL_CParam_ZL_CParam_formatVersion,
            ffi::ZL_MAX_FORMAT_VERSION as i32,
        );

        if report._code != ffi::ZL_ErrorCode_ZL_ErrorCode_no_error {
            return ffi::ZL_GraphID { gid: u32::MAX };
        }

        // Return generic compression graph
        ffi::ZL_GraphID {
            gid: ffi::ZL_StandardGraphID_ZL_StandardGraphID_compress_generic,
        }
    }

    // Prepare data
    let data = b"Hello, OpenZL!";
    let bound = (data.len() * 2) + 512 + 8;
    let mut compressed = vec![0u8; bound];

    // Compress
    let report = ffi::ZL_compress_usingGraphFn(
        compressed.as_mut_ptr() as *mut c_void,
        compressed.len(),
        data.as_ptr() as *const c_void,
        data.len(),
        Some(graph_fn),
    );

    if report._code != ffi::ZL_ErrorCode_ZL_ErrorCode_no_error {
        panic!("Compression failed: {}", report._code);
    }

    let compressed_size = report._value._value;
    compressed.truncate(compressed_size);

    println!("Compressed {} bytes to {} bytes", data.len(), compressed_size);

    // Decompress
    let size_report = ffi::ZL_getDecompressedSize(
        compressed.as_ptr() as *const c_void,
        compressed.len(),
    );

    if size_report._code != ffi::ZL_ErrorCode_ZL_ErrorCode_no_error {
        panic!("Failed to get decompressed size");
    }

    let decompressed_size = size_report._value._value;
    let mut decompressed = vec![0u8; decompressed_size];

    let report = ffi::ZL_decompress(
        decompressed.as_mut_ptr() as *mut c_void,
        decompressed.len(),
        compressed.as_ptr() as *const c_void,
        compressed.len(),
    );

    if report._code != ffi::ZL_ErrorCode_ZL_ErrorCode_no_error {
        panic!("Decompression failed");
    }

    assert_eq!(data, decompressed.as_slice());
}
```

## Available Bindings

The bindings include all OpenZL public API functions and types:

### Core Functions
- `ZL_compress_usingGraphFn` - Compress with custom graph function
- `ZL_compress_usingCompressor` - Compress with pre-created compressor
- `ZL_decompress` - Decompress data
- `ZL_getDecompressedSize` - Get original size from compressed data

### Graph Functions
- `ZL_Compressor_registerFieldLZGraph` - Register FieldLZ graph (structured data)
- Standard graph IDs like `ZL_StandardGraphID_compress_generic`

### Type System
- `ZL_TypedRef_createSerial` - Create reference for raw bytes
- `ZL_TypedRef_createStruct` - Create reference for structured data
- `ZL_TypedRef_createNumeric` - Create reference for numeric arrays
- `ZL_TypedRef_createString` - Create reference for variable-length strings
- `ZL_TypedRef_free` - Free typed reference

### Error Handling
- `ZL_Report` - Result/error type (union)
- `ZL_ErrorCode` - Error codes
- `ZL_isError` - Check if report contains error

### Constants
- `ZL_MAX_FORMAT_VERSION` - Maximum supported format version
- `ZL_StandardGraphID_*` - Standard graph identifiers

## OpenZL Concepts

### Graph-Based Compression
OpenZL uses a DAG (Directed Acyclic Graph) of compression codecs. You either:
1. Use pre-built graphs (e.g., `compress_generic`)
2. Register custom graphs with specific codec combinations
3. Train custom compressors for specific data formats

### Type System
OpenZL has a type system for inputs:
- `ZL_Type_serial` - Raw bytes (use generic compression)
- `ZL_Type_struct` - Fixed-size fields (use FieldLZ or similar)
- `ZL_Type_numeric` - Numeric arrays
- `ZL_Type_string` - Variable-length strings

**Important:** The graph must match the input type, or you'll get error 43 (`inputType_unsupported`).

### Format Version
OpenZL requires setting the format version:
```rust
ffi::ZL_Compressor_setParameter(
    compressor,
    ffi::ZL_CParam_ZL_CParam_formatVersion,
    ffi::ZL_MAX_FORMAT_VERSION as i32,
);
```

## Testing

Run the basic bindings test:

```bash
cargo test
```

## Regenerating Bindings

To regenerate bindings after OpenZL updates:

```bash
cargo clean
cargo build
```

The `build.rs` script will automatically run `bindgen` with the current headers.

## Resources

- [OpenZL GitHub](https://github.com/facebook/openzl)
- [OpenZL Documentation](https://github.com/facebook/openzl/tree/main/doc)
- [Safe wrapper: `openzl` crate](../openzl/README.md)

## License

Apache-2.0 (matching the parent OTel-Arrow project)

The OpenZL library itself is licensed under the Apache-2.0 license by Meta.
