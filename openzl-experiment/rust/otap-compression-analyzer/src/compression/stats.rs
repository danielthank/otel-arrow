use std::collections::HashMap;
use std::time::Duration;

/// Statistics for a single payload (IPC layer only)
#[derive(Debug, Clone)]
pub struct PayloadStats {
    pub original_size: u64,
    pub ipc_compressed_size: Option<u64>,
    pub ipc_compression_time: Option<Duration>,
}

impl PayloadStats {
    pub fn new(original_size: u64) -> Self {
        Self {
            original_size,
            ipc_compressed_size: None,
            ipc_compression_time: None,
        }
    }
}

/// Statistics for a batch (gRPC layer)
#[derive(Debug, Clone)]
pub struct BatchStats {
    pub serialized_size: u64,  // Size after IPC compression + protobuf serialization
    pub grpc_compressed_size: Option<u64>,
    pub grpc_compression_time: Option<Duration>,
}

impl BatchStats {
    pub fn new(serialized_size: u64) -> Self {
        Self {
            serialized_size,
            grpc_compressed_size: None,
            grpc_compression_time: None,
        }
    }
}

/// Aggregated IPC compression statistics for a payload type
#[derive(Debug, Default, Clone)]
pub struct IpcCompressionStats {
    pub payload_count: usize,
    pub total_original_size: u64,
    pub total_ipc_compressed_size: u64,
    pub total_ipc_time: Duration,
    pub has_ipc_stats: bool,
}

impl IpcCompressionStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_payload(&mut self, stats: &PayloadStats) {
        self.payload_count += 1;
        self.total_original_size += stats.original_size;

        if let Some(size) = stats.ipc_compressed_size {
            self.total_ipc_compressed_size += size;
            self.has_ipc_stats = true;
        }

        if let Some(time) = stats.ipc_compression_time {
            self.total_ipc_time += time;
        }
    }

    pub fn merge(&mut self, other: &IpcCompressionStats) {
        self.payload_count += other.payload_count;
        self.total_original_size += other.total_original_size;
        self.total_ipc_compressed_size += other.total_ipc_compressed_size;
        self.total_ipc_time += other.total_ipc_time;
        self.has_ipc_stats = self.has_ipc_stats || other.has_ipc_stats;
    }

    pub fn ipc_compression_ratio(&self) -> f64 {
        if self.total_ipc_compressed_size == 0 || !self.has_ipc_stats {
            0.0
        } else {
            self.total_original_size as f64 / self.total_ipc_compressed_size as f64
        }
    }

    pub fn avg_ipc_time(&self) -> Duration {
        if self.payload_count == 0 || !self.has_ipc_stats {
            Duration::ZERO
        } else {
            self.total_ipc_time / self.payload_count as u32
        }
    }

    pub fn ipc_throughput_mbps(&self) -> f64 {
        if self.total_ipc_time.as_secs_f64() == 0.0 || !self.has_ipc_stats {
            0.0
        } else {
            let mb = self.total_original_size as f64 / (1024.0 * 1024.0);
            mb / self.total_ipc_time.as_secs_f64()
        }
    }
}

/// Aggregated gRPC compression statistics
#[derive(Debug, Default, Clone)]
pub struct GrpcCompressionStats {
    pub batch_count: usize,
    pub total_serialized_size: u64,
    pub total_grpc_compressed_size: u64,
    pub total_grpc_time: Duration,
    pub has_grpc_stats: bool,
}

impl GrpcCompressionStats {
    pub fn add_batch(&mut self, stats: &BatchStats) {
        self.batch_count += 1;
        self.total_serialized_size += stats.serialized_size;

        if let Some(size) = stats.grpc_compressed_size {
            self.total_grpc_compressed_size += size;
            self.has_grpc_stats = true;
        }

        if let Some(time) = stats.grpc_compression_time {
            self.total_grpc_time += time;
        }
    }

    pub fn grpc_compression_ratio(&self) -> f64 {
        if self.total_grpc_compressed_size == 0 || !self.has_grpc_stats {
            0.0
        } else {
            self.total_serialized_size as f64 / self.total_grpc_compressed_size as f64
        }
    }

    pub fn avg_grpc_time(&self) -> Duration {
        if self.batch_count == 0 || !self.has_grpc_stats {
            Duration::ZERO
        } else {
            self.total_grpc_time / self.batch_count as u32
        }
    }

    pub fn grpc_throughput_mbps(&self) -> f64 {
        if self.total_grpc_time.as_secs_f64() == 0.0 || !self.has_grpc_stats {
            0.0
        } else {
            let mb = self.total_serialized_size as f64 / (1024.0 * 1024.0);
            mb / self.total_grpc_time.as_secs_f64()
        }
    }
}

/// Container for all statistics grouped by payload type and batch
#[derive(Debug, Default)]
pub struct StatsCollector {
    ipc_stats_by_type: HashMap<String, IpcCompressionStats>,
    grpc_stats: GrpcCompressionStats,
}

impl StatsCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_payload(&mut self, payload_type: String, stats: &PayloadStats) {
        self.ipc_stats_by_type
            .entry(payload_type)
            .or_insert_with(IpcCompressionStats::new)
            .add_payload(stats);
    }

    pub fn add_batch(&mut self, stats: &BatchStats) {
        self.grpc_stats.add_batch(stats);
    }

    pub fn get_overall_ipc_stats(&self) -> IpcCompressionStats {
        let mut overall = IpcCompressionStats::new();
        for stats in self.ipc_stats_by_type.values() {
            overall.merge(stats);
        }
        overall
    }

    pub fn get_grpc_stats(&self) -> &GrpcCompressionStats {
        &self.grpc_stats
    }

    pub fn iter_ipc_sorted(&self) -> Vec<(&String, &IpcCompressionStats)> {
        let mut sorted: Vec<_> = self.ipc_stats_by_type.iter().collect();
        sorted.sort_by_key(|(name, _)| *name);
        sorted
    }

    pub fn total_compression_ratio(&self) -> f64 {
        let ipc_overall = self.get_overall_ipc_stats();
        let grpc_stats = &self.grpc_stats;

        let original_size = ipc_overall.total_original_size;
        let final_size = if grpc_stats.has_grpc_stats {
            grpc_stats.total_grpc_compressed_size
        } else if ipc_overall.has_ipc_stats {
            ipc_overall.total_ipc_compressed_size
        } else {
            original_size
        };

        if final_size == 0 {
            0.0
        } else {
            original_size as f64 / final_size as f64
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
