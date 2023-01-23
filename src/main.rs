use anyhow::Context;
use clap::Parser;
use std::{fs::File, path::PathBuf, process::ExitCode};
use tcp_serial_proxy::{
    proxy_server::ProxyServer,
    server_config::{DirtyServerConfig, ServerConfig},
};
use tokio::{runtime::Builder, signal, sync::watch};

/// A proxy server between TCP and serial ports.
/// To stop the server, press ctrl-c.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the config file in JSON format.
    ///
    /// Config is a JSON object containing mappings from TCP ports to serial port configs
    /// and optional default settings under the key "default_settings".
    /// Values from the default settings will be used for all ports where corresponding values are missing.
    /// Multiple TCP ports can be proxied to one serial port, but serial ports are set to be exlusive
    /// (two TCP connections cannot use the same serial port).
    #[arg(short, long)]
    config: PathBuf,
}

fn run(args: Args) -> anyhow::Result<()> {
    let (stop_tx, stop_rx) = watch::channel(());
    let server = {
        let config_file = File::open(&args.config)
            .with_context(|| format!("failed to open config file: {}", args.config.display()))?;
        let dirty_config: DirtyServerConfig =
            serde_json::from_reader(config_file).with_context(|| {
                format!(
                    "failed to deserialize config file: {}",
                    args.config.display()
                )
            })?;
        let clean_config = ServerConfig::try_from(dirty_config)
            .with_context(|| format!("invalid config: {}", args.config.display()))?;
        ProxyServer::new(clean_config, stop_rx)
    };

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build async runtime")?;

    runtime.block_on(async move {
        tokio::spawn(async move {
            match signal::ctrl_c().await {
                Ok(()) => log::info!("ctrl-c received"),
                Err(e) => log::error!("failed to wait for ctrl-c: {}", e),
            }

            log::info!("stopping the proxy server");

            if stop_tx.send(()).is_err() {
                log::warn!("proxy server already stopped");
            }
        });

        server.run().await;
    });

    Ok(())
}

fn main() -> ExitCode {
    env_logger::init();

    let args = Args::parse();
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{:?}", e);
            ExitCode::FAILURE
        }
    }
}
