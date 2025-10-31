mod collect;
mod analyze;
mod compression;

use std::path::PathBuf;
use clap::Parser;
use compression::CompressionMethod;

/// OTAP Compression Analyzer CLI
#[derive(Parser, Debug)]
#[command(name = "otap-compression-analyzer")]
#[command(about = "Tool for collecting and analyzing OTAP Arrow payloads for OpenZL compression training", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser, Debug)]
enum Commands {
    /// Collect Arrow payloads and save them to disk, organized by payload type
    Collect {
        /// Output directory for collected payloads
        #[arg(short, long, default_value = "./collected_payloads")]
        output_dir: PathBuf,

        /// Server bind address
        #[arg(short, long, default_value = "0.0.0.0:4317")]
        addr: String,
    },
    /// Analyze collected payloads and compare compression methods
    Analyze {
        /// Input directory containing collected payloads
        #[arg(short, long, default_value = "./collected_payloads")]
        input_dir: PathBuf,

        /// Compression method to evaluate (1a, 1b, 2a, 2b, 3, 4)
        #[arg(short, long, default_value = "1a")]
        method: CompressionMethod,

        /// Zstd compression level (1-22, default 5)
        #[arg(long, default_value = "5")]
        zstd_level: i32,

        /// Path to trained OpenZL compressor file (.zl) for Method 4
        #[arg(long, value_name = "FILE")]
        compressor_file: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Collect { output_dir, addr } => {
            collect::run_collect_mode(output_dir, addr).await?;
        }
        Commands::Analyze { input_dir, method, zstd_level, compressor_file } => {
            analyze::run_analyze_mode(input_dir, method, zstd_level, compressor_file).await?;
        }
    }

    Ok(())
}
