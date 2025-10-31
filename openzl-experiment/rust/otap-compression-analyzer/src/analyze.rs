use std::path::PathBuf;

use prost::Message;
use tokio::fs;

use otel_arrow_rust::proto::opentelemetry::arrow::v1::{
    ArrowPayloadType, BatchArrowRecords,
};

use crate::compression::{
    BatchStats, CompressionMethod, GrpcCompressor, IpcCompressor, OpenZLGrpcCompressor,
    OpenZLIpcCompressor, PayloadStats, StatsCollector, ZstdGrpcCompressor, ZstdIpcCompressor,
    CustomOpenZLGrpcCompressor,
};
use crate::compression::stats::format_size;

pub async fn run_analyze_mode(
    input_dir: PathBuf,
    method: CompressionMethod,
    zstd_level: i32,
    compressor_file: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting OTAP Compression Analyzer - Analyze Mode");
    println!("Input directory: {}", input_dir.display());
    println!("Compression method: {}", method);
    println!("Zstd level: {}", zstd_level);
    if let Some(ref path) = compressor_file {
        println!("Compressor file: {}", path.display());
    }
    println!();

    if !input_dir.exists() {
        return Err(format!("Input directory does not exist: {}", input_dir.display()).into());
    }

    // Validate: Method 4 requires compressor_file
    if method == CompressionMethod::Method4 && compressor_file.is_none() {
        return Err("Method 4 (Format-Aware) requires --compressor-file argument".into());
    }

    // Initialize compressors based on method
    let ipc_compressor: Option<Box<dyn IpcCompressor + Send>> = if method.has_ipc_compression() {
        if method.uses_openzl_ipc() {
            // Methods 2a, 2b: Generic OpenZL
            Some(Box::new(OpenZLIpcCompressor::new()?))
        } else {
            // Methods 1a, 1b: Zstd
            Some(Box::new(ZstdIpcCompressor::new(zstd_level)))
        }
    } else {
        None
    };

    let grpc_compressor: Option<Box<dyn GrpcCompressor + Send>> =
        if method.has_grpc_compression() {
            if method == CompressionMethod::Method4 {
                // Method 4: Custom format-aware compressor at gRPC layer
                let path = compressor_file.as_ref().unwrap();
                Some(Box::new(CustomOpenZLGrpcCompressor::new(path)?))
            } else if method.uses_openzl_grpc() {
                // Method 3: Generic OpenZL at gRPC layer
                Some(Box::new(OpenZLGrpcCompressor::new()?))
            } else {
                // Methods 1a, 2a: Zstd at gRPC layer
                Some(Box::new(ZstdGrpcCompressor::new(zstd_level)))
            }
        } else {
            None
        };

    let mut stats_collector = StatsCollector::new();

    // Recursively find all .batch files
    println!("Scanning for .batch files...");
    let batch_files = find_batch_files(&input_dir).await?;
    println!("Found {} .batch files\n", batch_files.len());

    if batch_files.is_empty() {
        return Err("No .batch files found in input directory".into());
    }

    // Process each batch file
    for (idx, batch_path) in batch_files.iter().enumerate() {
        if idx % 10 == 0 {
            println!("Processing file {}/{}", idx + 1, batch_files.len());
        }

        // Read and decode protobuf
        let encoded = fs::read(&batch_path).await?;
        let batch_records = BatchArrowRecords::decode(&encoded[..])?;

        // Clone batch for modification
        let mut modified_batch = batch_records.clone();

        // IPC layer compression (per-payload)
        for (original_payload, modified_payload) in batch_records.arrow_payloads.iter()
            .zip(modified_batch.arrow_payloads.iter_mut())
        {
            let payload_type_name = payload_type_to_string(original_payload.r#type);
            let arrow_data = &original_payload.record;

            if arrow_data.is_empty() {
                continue;
            }

            // Create payload stats
            let mut payload_stats = PayloadStats::new(arrow_data.len() as u64);

            // IPC layer compression (if enabled)
            if let Some(ref compressor) = ipc_compressor {
                let start = std::time::Instant::now();
                let compressed = compressor.compress(arrow_data)?;
                let duration = start.elapsed();

                payload_stats.ipc_compressed_size = Some(compressed.len() as u64);
                payload_stats.ipc_compression_time = Some(duration);

                // Replace payload record with compressed data
                modified_payload.record = compressed;
            }

            // Add IPC stats
            stats_collector.add_payload(payload_type_name, &payload_stats);
        }

        // gRPC layer compression (per-batch)
        if let Some(ref compressor) = grpc_compressor {
            // Serialize the modified batch with IPC-compressed payloads
            let serialized = modified_batch.encode_to_vec();
            let serialized_size = serialized.len() as u64;

            // Create batch stats
            let mut batch_stats = BatchStats::new(serialized_size);

            // Compress at gRPC layer
            let start = std::time::Instant::now();
            let compressed = compressor.compress(&serialized)?;
            let duration = start.elapsed();

            batch_stats.grpc_compressed_size = Some(compressed.len() as u64);
            batch_stats.grpc_compression_time = Some(duration);

            // Add batch stats
            stats_collector.add_batch(&batch_stats);
        }
    }

    // Print results
    print_results(&stats_collector, method, zstd_level);

    Ok(())
}

/// Recursively find all .batch files in the directory tree
async fn find_batch_files(dir: &PathBuf) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut batch_files = Vec::new();
    find_batch_files_recursive(dir, &mut batch_files).await?;
    batch_files.sort();
    Ok(batch_files)
}

fn find_batch_files_recursive<'a>(
    dir: &'a PathBuf,
    batch_files: &'a mut Vec<PathBuf>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<(), Box<dyn std::error::Error>>> + 'a>,
> {
    Box::pin(async move {
        let mut entries = fs::read_dir(dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectory
                find_batch_files_recursive(&path, batch_files).await?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("batch") {
                batch_files.push(path);
            }
        }

        Ok(())
    })
}

fn payload_type_to_string(type_id: i32) -> String {
    match ArrowPayloadType::try_from(type_id) {
        Ok(payload_type) => format!("{:?}", payload_type),
        Err(_) => format!("Unknown({})", type_id),
    }
}

fn print_results(stats_collector: &StatsCollector, method: CompressionMethod, _zstd_level: i32) {
    println!("\n{}", "=".repeat(80));
    println!("COMPRESSION ANALYSIS RESULTS");
    println!("Method: {}", method);
    println!("{}\n", "=".repeat(80));

    // Print per-type IPC stats
    for (payload_type, stats) in stats_collector.iter_ipc_sorted() {
        println!("Payload Type: {}", payload_type);
        println!("  Payloads: {}", stats.payload_count);
        println!("  Original Size: {}", format_size(stats.total_original_size));

        if stats.has_ipc_stats {
            println!();
            println!("  IPC Layer Compression:");
            println!(
                "    Compressed Size: {}",
                format_size(stats.total_ipc_compressed_size)
            );
            println!("    Compression Ratio: {:.2}x", stats.ipc_compression_ratio());
            println!(
                "    Avg Time: {:.2}ms",
                stats.avg_ipc_time().as_secs_f64() * 1000.0
            );
            println!("    Throughput: {:.2} MB/s", stats.ipc_throughput_mbps());
        }

        println!("\n{}\n", "-".repeat(80));
    }

    // Print overall summary
    let overall_ipc_stats = stats_collector.get_overall_ipc_stats();
    let grpc_stats = stats_collector.get_grpc_stats();

    println!("OVERALL SUMMARY");
    println!("{}", "=".repeat(80));
    println!("Total Payloads: {}", overall_ipc_stats.payload_count);
    println!(
        "Total Original Size: {}",
        format_size(overall_ipc_stats.total_original_size)
    );

    if overall_ipc_stats.has_ipc_stats {
        println!();
        println!("IPC Layer Compression:");
        println!(
            "  Total Compressed: {}",
            format_size(overall_ipc_stats.total_ipc_compressed_size)
        );
        println!(
            "  Compression Ratio: {:.2}x",
            overall_ipc_stats.ipc_compression_ratio()
        );
        println!(
            "  Avg Time: {:.2}ms",
            overall_ipc_stats.avg_ipc_time().as_secs_f64() * 1000.0
        );
        println!(
            "  Throughput: {:.2} MB/s",
            overall_ipc_stats.ipc_throughput_mbps()
        );
    }

    if grpc_stats.has_grpc_stats {
        println!();
        println!("gRPC Layer Compression:");
        println!("  Total Batches: {}", grpc_stats.batch_count);
        println!(
            "  Total Serialized Size: {}",
            format_size(grpc_stats.total_serialized_size)
        );
        println!(
            "  Total Compressed: {}",
            format_size(grpc_stats.total_grpc_compressed_size)
        );
        println!(
            "  Compression Ratio: {:.2}x",
            grpc_stats.grpc_compression_ratio()
        );
        println!(
            "  Avg Time: {:.2}ms",
            grpc_stats.avg_grpc_time().as_secs_f64() * 1000.0
        );
        println!(
            "  Throughput: {:.2} MB/s",
            grpc_stats.grpc_throughput_mbps()
        );
    }

    println!();
    println!(
        "Total Compression Ratio: {:.2}x",
        stats_collector.total_compression_ratio()
    );
    println!("{}", "=".repeat(80));
}
