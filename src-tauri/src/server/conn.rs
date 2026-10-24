//! Accepting connections: HTTP/1.1 only (what phone browsers use without
//! TLS), with a connection limit and timeouts, so a client that stalls --
//! deliberately (slowloris) or because the phone was locked -- never ties up
//! the server, its file handles or its blocking threads.
//!
//! `axum::serve` offers none of these knobs, hence this small accept loop on
//! hyper directly (the same building blocks axum uses internally).

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::Router;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::Request;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, watch, OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, Sleep};
use tower_service::Service;

/// Connections served at the same time. A phone opens about six; more only
/// cost file handles. Further clients wait in the OS backlog.
const MAX_CONNECTIONS: usize = 64;

/// Time a client gets to send the request headers.
const HEADER_TIMEOUT: Duration = Duration::from_secs(15);

/// A connection on which nothing moves in either direction for this long is
/// closed: stalled downloads and uploads, forgotten keep-alive connections.
/// Interrupted transfers can be resumed (Range requests, upload keys).
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Running transfers get this long to finish when the server shuts down.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Pause after a failed `accept` (e.g. out of file handles), so the loop
/// does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// Serves `router` until `shutdown` fires (or its sender is dropped).
pub(super) async fn serve(listener: TcpListener, router: Router, shutdown: oneshot::Receiver<()>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let (closing_tx, closing_rx) = watch::channel(false);
    let mut shutdown = pin!(shutdown);
    loop {
        let permit = tokio::select! {
            _ = &mut shutdown => break,
            permit = slots.clone().acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let accepted = tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((stream, addr)) => {
                tokio::spawn(serve_connection(
                    stream,
                    addr,
                    router.clone(),
                    closing_rx.clone(),
                    permit,
                ));
            }
            Err(e) => {
                log::debug!("Verbindung nicht angenommen: {}", e.kind());
                tokio::time::sleep(ACCEPT_BACKOFF).await;
            }
        }
    }
    let _ = closing_tx.send(true);
    log::info!("HTTP-Server beendet");
}

async fn serve_connection(
    stream: TcpStream,
    addr: SocketAddr,
    router: Router,
    mut closing: watch::Receiver<bool>,
    _slot: OwnedSemaphorePermit,
) {
    let io = TokioIo::new(IdleTimeout::new(stream, IDLE_TIMEOUT));
    let service = hyper::service::service_fn(move |mut req: Request<Incoming>| {
        // What `into_make_service_with_connect_info` would provide.
        req.extensions_mut().insert(ConnectInfo(addr));
        let mut router = router.clone();
        async move {
            std::future::poll_fn(|cx| Service::<Request<Incoming>>::poll_ready(&mut router, cx))
                .await?;
            router.call(req).await
        }
    });
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT);
    let mut connection = pin!(builder.serve_connection(io, service));
    tokio::select! {
        result = connection.as_mut() => {
            if let Err(e) = result {
                log::debug!("Verbindung beendet: {e}");
            }
            return;
        }
        // Shutdown requested (or the server task is gone).
        _ = closing.wait_for(|closing| *closing) => {}
    }
    connection.as_mut().graceful_shutdown();
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, connection).await;
}

/// Fails reads and writes with `TimedOut` once neither direction made
/// progress for `timeout`. The clock only counts while an operation is
/// actually waiting, so slow server-side work never trips it.
struct IdleTimeout<T> {
    inner: T,
    timeout: Duration,
    deadline: Pin<Box<Sleep>>,
}

impl<T> IdleTimeout<T> {
    fn new(inner: T, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            deadline: Box::pin(tokio::time::sleep(timeout)),
        }
    }

    fn progressed(&mut self) {
        let next = Instant::now() + self.timeout;
        self.deadline.as_mut().reset(next);
    }

    /// Maps an inner result: progress restarts the clock, waiting checks it.
    fn track<R>(&mut self, cx: &mut Context<'_>, poll: Poll<io::Result<R>>) -> Poll<io::Result<R>> {
        match poll {
            Poll::Ready(result) => {
                self.progressed();
                Poll::Ready(result)
            }
            Poll::Pending => match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "connection idle",
                ))),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for IdleTimeout<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_read(cx, buf);
        this.track(cx, poll)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for IdleTimeout<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_write(cx, buf);
        this.track(cx, poll)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_write_vectored(cx, bufs);
        this.track(cx, poll)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_flush(cx);
        this.track(cx, poll)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let poll = Pin::new(&mut this.inner).poll_shutdown(cx);
        this.track(cx, poll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn idle_connections_time_out_but_active_ones_do_not() {
        let (mut client, server) = tokio::io::duplex(64);
        let mut server = IdleTimeout::new(server, Duration::from_millis(100));

        // Data trickling in faster than the timeout keeps it alive.
        let writer = tokio::spawn(async move {
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_millis(40)).await;
                client.write_all(b"x").await.unwrap();
            }
            client
        });
        let mut buf = [0u8; 1];
        for _ in 0..5 {
            server.read_exact(&mut buf).await.unwrap();
        }
        // Keep the client open: silence, not a closed pipe, must end it.
        let _client = writer.await.unwrap();
        let err = server.read_exact(&mut buf).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }
}
