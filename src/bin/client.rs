use std::{env, net::SocketAddr};
use tokio::{io, net::TcpStream, runtime::Builder};

fn main() {
    let socket = env::args().collect::<Vec<_>>();
    let socket: SocketAddr = socket[1].parse().unwrap();

    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    runtime
        .block_on(async move {
            let conn = TcpStream::connect(socket).await.unwrap();
            conn.set_nodelay(true).unwrap();
            let (mut rx, mut tx) = conn.into_split();
            let mut stdin = io::stdin();
            let mut stdout = io::stdout();

            tokio::try_join!(
                io::copy(&mut rx, &mut stdout),
                io::copy(&mut stdin, &mut tx),
            )
        })
        .unwrap();
}
