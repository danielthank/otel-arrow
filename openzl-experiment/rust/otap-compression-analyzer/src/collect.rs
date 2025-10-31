use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use tokio::fs;
use tokio::signal;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use uuid::Uuid;

use otel_arrow_rust::proto::opentelemetry::arrow::v1::{
    arrow_traces_service_server::{ArrowTracesService, ArrowTracesServiceServer},
    arrow_logs_service_server::{ArrowLogsService, ArrowLogsServiceServer},
    arrow_metrics_service_server::{ArrowMetricsService, ArrowMetricsServiceServer},
    BatchArrowRecords,
    BatchStatus,
};

#[derive(Clone)]
pub struct CollectorService {
    output_dir: PathBuf,
    shutdown_flag: Arc<AtomicBool>,
}

async fn process_arrow_stream(
    output_dir: PathBuf,
    stream_id: String,
    shutdown_flag: Arc<AtomicBool>,
    signal_type: &str,
    mut stream: tonic::Streaming<BatchArrowRecords>,
    tx: tokio::sync::mpsc::Sender<Result<BatchStatus, Status>>,
) {
    // Create stream-specific directory
    let stream_dir = output_dir.join(&stream_id);
    if let Err(e) = fs::create_dir_all(&stream_dir).await {
        println!("ERROR: Failed to create stream directory {}: {}", stream_dir.display(), e);
        return;
    }

    let mut batch_counter = 1u64;

    while let Some(batch_records) = stream.message().await.transpose() {
        if shutdown_flag.load(Ordering::Relaxed) {
            println!("Shutdown signal received, stopping {} stream processing.", signal_type);
            break;
        }

        let batch_records = match batch_records {
            Ok(br) => br,
            Err(e) => {
                println!("ERROR: Received an error from {} client stream: {}", signal_type, e);
                let _ = tx.send(Err(e)).await;
                return;
            }
        };

        println!("[{}] --> Received BatchArrowRecords in stream (batch_id: {})", signal_type, batch_records.batch_id);

        // Encode entire BatchArrowRecords to protobuf bytes
        let encoded_bytes = match batch_records.encode_to_vec() {
            bytes if !bytes.is_empty() => bytes,
            _ => {
                println!("    -> ERROR: Failed to encode BatchArrowRecords");
                continue;
            }
        };

        // Generate filename with zero-padded counter
        let filename = format!("{:04}.batch", batch_counter);
        let filepath = stream_dir.join(&filename);

        // Write protobuf bytes to file
        match fs::write(&filepath, &encoded_bytes).await {
            Ok(_) => {
                println!(
                    "    -> ✓ Wrote {} bytes to: {}",
                    encoded_bytes.len(),
                    filepath.display()
                );
                batch_counter += 1;
            }
            Err(e) => {
                println!(
                    "    -> ERROR: Failed to write file {}: {}",
                    filepath.display(),
                    e
                );
            }
        }

        let status = BatchStatus {
            batch_id: batch_records.batch_id,
            status_code: 0, // STATUS_CODE_OK
            status_message: String::new(),
        };

        if tx.send(Ok(status)).await.is_err() {
            println!("WARN: {} client disconnected, could not send batch status.", signal_type);
            return;
        }
    }
    println!("[{}] Client closed the stream.", signal_type);
}

#[tonic::async_trait]
impl ArrowTracesService for CollectorService {
    type ArrowTracesStream = ReceiverStream<Result<BatchStatus, Status>>;

    async fn arrow_traces(
        &self,
        request: Request<Streaming<BatchArrowRecords>>,
    ) -> Result<Response<Self::ArrowTracesStream>, Status> {
        // Generate stream ID: timestamp_uuid
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let stream_id = format!("traces/{}_{}", timestamp, Uuid::new_v4().simple());

        println!("New OTel-Arrow Trace Stream Connected! stream_id: {}", stream_id);
        let stream = request.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let output_dir = self.output_dir.clone();
        let shutdown_flag = self.shutdown_flag.clone();

        tokio::spawn(async move {
            process_arrow_stream(output_dir, stream_id, shutdown_flag, "TRACES", stream, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl ArrowLogsService for CollectorService {
    type ArrowLogsStream = ReceiverStream<Result<BatchStatus, Status>>;

    async fn arrow_logs(
        &self,
        request: Request<Streaming<BatchArrowRecords>>,
    ) -> Result<Response<Self::ArrowLogsStream>, Status> {
        // Generate stream ID: timestamp_uuid
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let stream_id = format!("logs/{}_{}", timestamp, Uuid::new_v4().simple());

        println!("New OTel-Arrow Logs Stream Connected! stream_id: {}", stream_id);
        let stream = request.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let output_dir = self.output_dir.clone();
        let shutdown_flag = self.shutdown_flag.clone();

        tokio::spawn(async move {
            process_arrow_stream(output_dir, stream_id, shutdown_flag, "LOGS", stream, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tonic::async_trait]
impl ArrowMetricsService for CollectorService {
    type ArrowMetricsStream = ReceiverStream<Result<BatchStatus, Status>>;

    async fn arrow_metrics(
        &self,
        request: Request<Streaming<BatchArrowRecords>>,
    ) -> Result<Response<Self::ArrowMetricsStream>, Status> {
        // Generate stream ID: timestamp_uuid
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let stream_id = format!("metrics/{}_{}", timestamp, Uuid::new_v4().simple());

        println!("New OTel-Arrow Metrics Stream Connected! stream_id: {}", stream_id);
        let stream = request.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let output_dir = self.output_dir.clone();
        let shutdown_flag = self.shutdown_flag.clone();

        tokio::spawn(async move {
            process_arrow_stream(output_dir, stream_id, shutdown_flag, "METRICS", stream, tx).await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

pub async fn run_collect_mode(output_dir: PathBuf, addr_str: String) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting OTAP Compression Analyzer - Collect Mode");
    println!("Output directory: {}", output_dir.display());
    println!("Server address: {}", addr_str);
    println!("Press Ctrl+C to stop collection\n");

    // Create output directory
    fs::create_dir_all(&output_dir).await?;

    let addr = addr_str.parse()?;
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = shutdown_flag.clone();

    // Setup Ctrl+C handler
    tokio::spawn(async move {
        signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        println!("\n\nReceived Ctrl+C, shutting down gracefully...");
        shutdown_flag_clone.store(true, Ordering::Relaxed);
    });

    let collector_service = CollectorService {
        output_dir,
        shutdown_flag: shutdown_flag.clone(),
    };

    println!("OTel-Arrow Collector Server listening on {}", addr);

    let trace_layer = TraceLayer::new_for_grpc()
        .make_span_with(DefaultMakeSpan::new().include_headers(true).level(Level::INFO))
        .on_response(DefaultOnResponse::new().include_headers(true).level(Level::INFO));

    Server::builder()
        .layer(trace_layer)
        .add_service(ArrowTracesServiceServer::new(collector_service.clone()))
        .add_service(ArrowLogsServiceServer::new(collector_service.clone()))
        .add_service(ArrowMetricsServiceServer::new(collector_service))
        .serve_with_shutdown(addr, async move {
            // Wait for shutdown signal
            while !shutdown_flag.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        })
        .await?;

    println!("Server shut down successfully.");
    Ok(())
}
