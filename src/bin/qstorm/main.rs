mod app;
mod tui;
mod ui;

use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "qstorm")]
#[command(about = "Vector search load testing tool", long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "qstorm.yaml")]
    config: PathBuf,

    /// Path to queries file (YAML with list of text queries to embed)
    #[arg(short, long)]
    queries: PathBuf,

    /// Run in headless mode (no TUI, just output results)
    #[arg(long)]
    headless: bool,

    /// Number of bursts to run (0 = continuous until stopped)
    #[arg(short, long, default_value = "0")]
    bursts: usize,

    /// Output format for headless mode
    #[arg(long, default_value = "json")]
    output: OutputFormat,
}

#[derive(Clone, Copy, Default, clap::ValueEnum)]
enum OutputFormat {
    #[default]
    Json,
    Csv,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    // Load configuration
    let config = qstorm::Config::from_file(&cli.config)?;

    // Validate queries file exists
    if !cli.queries.exists() {
        return Err(anyhow!("Queries file not found: {}", cli.queries.display()));
    }

    let queries_path = cli.queries.to_string_lossy().to_string();

    if cli.headless {
        run_headless(config, &queries_path, cli.bursts, cli.output).await
    } else {
        run_tui(config, &queries_path).await
    }
}

async fn run_headless(
    config: qstorm::Config,
    queries_path: &str,
    burst_count: usize,
    output: OutputFormat,
) -> Result<()> {
    eprintln!("Loading and embedding queries...");
    let mut app = app::App::new(config)?;
    app.load_and_embed_queries(queries_path).await?;
    eprintln!("Embedded {} queries", app.query_count());

    eprintln!("Connecting to provider...");
    app.connect().await?;

    eprintln!("Running warmup...");
    app.warmup().await?;

    eprintln!("Starting benchmark...");
    let count = if burst_count == 0 {
        usize::MAX
    } else {
        burst_count
    };

    // Print CSV header
    if matches!(output, OutputFormat::Csv) {
        println!("timestamp,qps,p50_ms,p90_ms,p99_ms,success,failure");
    }

    for _ in 0..count {
        let metrics = app.run_burst().await?;

        match output {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string(&metrics)?);
            }
            OutputFormat::Csv => {
                println!(
                    "{},{:.2},{:.2},{:.2},{:.2},{},{}",
                    metrics.timestamp,
                    metrics.qps,
                    metrics.latency.p50_us as f64 / 1000.0,
                    metrics.latency.p90_us as f64 / 1000.0,
                    metrics.latency.p99_us as f64 / 1000.0,
                    metrics.success_count,
                    metrics.failure_count,
                );
            }
        }
    }

    app.disconnect().await?;
    Ok(())
}

async fn run_tui(config: qstorm::Config, queries_path: &str) -> Result<()> {
    let mut app = app::App::new(config)?;

    // Load and embed queries before starting TUI
    eprintln!("Loading and embedding queries (this may take a moment)...");
    app.load_and_embed_queries(queries_path).await?;
    eprintln!("Embedded {} queries. Starting TUI...", app.query_count());

    let mut terminal = tui::init()?;
    let result = tui::run(&mut terminal, app).await;
    tui::restore()?;
    result
}
