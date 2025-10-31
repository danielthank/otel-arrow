/// Compression methods as defined in the project proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// Method 1a: Baseline - zstd at IPC layer + zstd at gRPC layer
    Method1a,
    /// Method 1b: Baseline without gRPC - zstd at IPC layer only
    Method1b,
    /// Method 2a: OpenZL Drop-In - OpenZL at IPC layer + zstd at gRPC layer
    Method2a,
    /// Method 2b: OpenZL without gRPC - OpenZL at IPC layer only
    Method2b,
    /// Method 3: OpenZL at gRPC layer only (no IPC compression)
    Method3,
    /// Method 4: Format-Aware custom compression
    Method4,
}

impl CompressionMethod {
    pub fn name(&self) -> &'static str {
        match self {
            CompressionMethod::Method1a => "Method 1a (Baseline: zstd IPC + zstd gRPC)",
            CompressionMethod::Method1b => "Method 1b (Baseline: zstd IPC only)",
            CompressionMethod::Method2a => "Method 2a (OpenZL IPC + zstd gRPC)",
            CompressionMethod::Method2b => "Method 2b (OpenZL IPC only)",
            CompressionMethod::Method3 => "Method 3 (OpenZL gRPC only)",
            CompressionMethod::Method4 => "Method 4 (Format-Aware)",
        }
    }

    pub fn has_ipc_compression(&self) -> bool {
        match self {
            CompressionMethod::Method1a | CompressionMethod::Method1b |
            CompressionMethod::Method2a | CompressionMethod::Method2b => true,
            CompressionMethod::Method3 | CompressionMethod::Method4 => false,
        }
    }

    pub fn has_grpc_compression(&self) -> bool {
        match self {
            CompressionMethod::Method1a | CompressionMethod::Method2a |
            CompressionMethod::Method3 | CompressionMethod::Method4 => true,
            CompressionMethod::Method1b | CompressionMethod::Method2b => false,
        }
    }

    pub fn uses_openzl_ipc(&self) -> bool {
        match self {
            CompressionMethod::Method2a | CompressionMethod::Method2b => true,
            _ => false,
        }
    }

    pub fn uses_openzl_grpc(&self) -> bool {
        match self {
            CompressionMethod::Method3 => true,
            _ => false,
        }
    }
}

impl std::str::FromStr for CompressionMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "1a" | "method1a" => Ok(CompressionMethod::Method1a),
            "1b" | "method1b" => Ok(CompressionMethod::Method1b),
            "2a" | "method2a" => Ok(CompressionMethod::Method2a),
            "2b" | "method2b" => Ok(CompressionMethod::Method2b),
            "3" | "method3" => Ok(CompressionMethod::Method3),
            "4" | "method4" => Ok(CompressionMethod::Method4),
            _ => Err(format!("Invalid compression method: {}. Valid values: 1a, 1b, 2a, 2b, 3, 4", s)),
        }
    }
}

impl std::fmt::Display for CompressionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
