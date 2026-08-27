use clap::Parser;
use persistence::Engine;
use server::resp_server;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use web::Metrics;

#[derive(Parser, Debug)]
#[command(
    name = "kvdb-server",
    version,
    about = "High-performance in-memory KV store"
)]
struct Args {
    /// Directory holding appendonly.aof and dump.rdb
    #[arg(long, default_value = "./data")]
    data_dir: String,

    /// Address for the RESP (mio) protocol server
    #[arg(long, default_value = "127.0.0.1:6380")]
    resp_addr: SocketAddr,

    /// Address for the HTTP dashboard / metrics / websocket server
    #[arg(long, default_value = "127.0.0.1:8080")]
    http_addr: SocketAddr,

    /// Seconds between automatic snapshots
    #[arg(long, default_value_t = 60)]
    snapshot_interval_secs: u64,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    let engine = Engine::open(&args.data_dir)?;
    let metrics = Arc::new(Metrics::new());

    engine.spawn_snapshot_loop(Duration::from_secs(args.snapshot_interval_secs));

    {
        let engine = engine.clone();
        let metrics = metrics.clone();
        let resp_addr = args.resp_addr;
        std::thread::Builder::new()
            .name("resp-event-loop".into())
            .spawn(move || {
                if let Err(e) = resp_server::run(resp_addr, engine, metrics) {
                    tracing::error!(error = %e, "RESP server exited with error");
                }
            })?;
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move { web::http::run(args.http_addr, engine, metrics).await })?;

    Ok(())
}
