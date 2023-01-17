use crate::server_config::{SerialPortSettings, ServerConfig};
use anyhow::Context;
use backoff::{future, ExponentialBackoffBuilder};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io,
    net::{TcpListener, TcpStream},
    sync::watch::Receiver,
};
use tokio_serial::SerialPortBuilderExt;

pub struct ProxyServer {
    config: ServerConfig,
    stop_watch: Receiver<()>,
}

impl ProxyServer {
    pub fn new(config: ServerConfig, stop_watch: Receiver<()>) -> Self {
        Self { config, stop_watch }
    }

    pub async fn run(self) {
        let tasks = self
            .config
            .into_iter()
            .map(|(tcp_port, serial_port)| {
                let port_path = serial_port.path.clone();
                let worker = ProxyWorker::new(tcp_port, serial_port);
                (
                    tcp_port,
                    port_path,
                    tokio::spawn(worker.run(self.stop_watch.clone())),
                )
            })
            .collect::<Vec<_>>();

        for (tcp_port, port_path, task) in tasks {
            if let Err(e) = task.await {
                log::error!(
                    "background task for TCP port {} and serial port {} stopped unexpectedly: {}",
                    tcp_port,
                    port_path,
                    e
                );
            }
        }
    }
}

struct ProxyWorker {
    tcp_port: SocketAddr,
    serial_port: SerialPortSettings,
    listener: Option<TcpListener>,
}

impl ProxyWorker {
    const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
    const MAX_BACKOFF: Duration = Duration::from_secs(10);

    fn new(tcp_port: SocketAddr, serial_port: SerialPortSettings) -> Self {
        Self {
            tcp_port,
            serial_port,
            listener: None,
        }
    }

    async fn get_listener(&mut self) -> TcpListener {
        if let Some(listener) = self.listener.take() {
            return listener;
        }

        let backoff = ExponentialBackoffBuilder::new()
            .with_initial_interval(Self::INITIAL_BACKOFF)
            .with_max_interval(Self::MAX_BACKOFF)
            .with_max_elapsed_time(None)
            .build();

        let listener = future::retry_notify(
            backoff,
            || async {
                TcpListener::bind(self.tcp_port)
                    .await
                    .map_err(backoff::Error::transient)
            },
            |err, _| {
                log::error!(
                    "failed to start listening on TCP port {}: {}",
                    self.tcp_port,
                    err
                );
            },
        )
        .await
        .expect(
            "there is no limit for max elapsed time, listener bind backoff should never timeout",
        );

        listener
    }

    async fn proxy(&self, mut conn: TcpStream) -> anyhow::Result<()> {
        conn.set_nodelay(true)
            .context("failed to set TCP_NODELAY to true")?;

        let mut port = tokio_serial::new(&self.serial_port.path, self.serial_port.baud_rate)
            .open_native_async()
            .context("failed to open serial port")?;
        port.set_exclusive(true)
            .context("failed to set serial port as exclusive")?;

        io::copy_bidirectional(&mut conn, &mut port)
            .await
            .context("failed to transfer data between ports")?;

        Ok(())
    }

    async fn serve_client(&mut self) -> anyhow::Result<()> {
        let listener = self.get_listener().await;
        let (conn, peer) = listener
            .accept()
            .await
            .context("failed to accept a connection")?;
        self.listener.replace(listener);

        log::info!(
            "accepted a connection from {} on TCP port {}",
            peer,
            self.tcp_port
        );
        self.proxy(conn).await?;

        Ok(())
    }

    async fn run(mut self, mut stop_watch: Receiver<()>) {
        loop {
            tokio::select! {
                biased;
                _ = stop_watch.changed() => break,
                result = self.serve_client() => {
                    let Err(e) = result else { continue };
                    log::error!("proxy connection from TCP port {} to serial port {}: {:?}", self.tcp_port, self.serial_port.path, e);
                },
            }
        }
    }
}
