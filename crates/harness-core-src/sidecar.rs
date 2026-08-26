#[path = "sidecar/stream.rs"]
mod stream;

use std::fs;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use crate::notify::Notify;
use crate::pi::PiSidecarSnapshot;
use stream::read_sidecar_stream;

#[derive(Debug)]
pub enum SidecarMessage {
    Snapshot(PiSidecarSnapshot),
    Theme([crate::render::Color; 15]),
}

/// Write half of every connected sidechannel extension plus the sticky
/// hello line replayed to each new connection.
#[derive(Default)]
struct Downstream {
    hello: Option<String>,
    writers: Vec<UnixStream>,
}

impl Downstream {
    fn send_line(writer: &mut UnixStream, line: &str) -> bool {
        writer
            .write_all(line.as_bytes())
            .and_then(|()| writer.write_all(b"\n"))
            .is_ok()
    }

    fn broadcast(&mut self, line: &str) {
        self.writers
            .retain_mut(|writer| Self::send_line(writer, line));
    }

    fn attach(&mut self, mut writer: UnixStream) {
        if let Some(hello) = self.hello.clone() {
            if !Self::send_line(&mut writer, &hello) {
                return;
            }
        }
        self.writers.push(writer);
    }

    fn set_hello(&mut self, line: String) {
        if self.hello.as_deref() == Some(line.as_str()) {
            return;
        }
        self.broadcast(&line);
        self.hello = Some(line);
    }
}

pub struct SidecarListener {
    socket_path: PathBuf,
    rx: Receiver<SidecarMessage>,
    downstream: Arc<Mutex<Downstream>>,
}

impl SidecarListener {
    pub fn start(notify: Notify, socket_path: PathBuf) -> anyhow::Result<Self> {
        let Some(parent) = socket_path.parent() else {
            anyhow::bail!("sidecar socket has no parent: {}", socket_path.display());
        };
        fs::create_dir_all(parent)?;
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;
        let (tx, rx) = mpsc::channel();
        let downstream = Arc::new(Mutex::new(Downstream::default()));
        let accept_downstream = Arc::clone(&downstream);

        std::thread::Builder::new()
            .name("pi-harness-sidecar-listener".into())
            .spawn(move || {
                for incoming in listener.incoming() {
                    match incoming {
                        Ok(stream) => {
                            if let Ok(writer) = stream.try_clone() {
                                let mut downstream = accept_downstream
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                downstream.attach(writer);
                            }
                            let tx = tx.clone();
                            let notify = notify.clone();
                            let _ = std::thread::Builder::new()
                                .name("pi-harness-sidecar-stream".into())
                                .spawn(move || read_sidecar_stream(stream, tx, notify));
                        }
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self {
            socket_path,
            rx,
            downstream,
        })
    }

    pub fn try_recv(&self) -> Option<SidecarMessage> {
        self.rx.try_recv().ok()
    }

    /// Sticky line replayed to every future connection and broadcast to the
    /// current ones. Used for the rail hello (width).
    pub fn set_hello(&self, line: String) {
        self.downstream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_hello(line);
    }

    /// One-shot line written to all currently connected extensions.
    pub fn broadcast(&self, line: &str) {
        self.downstream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .broadcast(line);
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}

impl Drop for SidecarListener {
    fn drop(&mut self) {
        if self.socket_path.exists() {
            let _ = fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(test)]
mod downstream_tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    fn pair() -> (UnixStream, UnixStream) {
        UnixStream::pair().expect("socketpair")
    }

    #[test]
    fn attach_replays_hello_and_broadcast_reaches_writer() {
        let (writer, reader) = pair();
        let mut downstream = Downstream::default();
        downstream.set_hello("{\"type\":\"hello\"}".to_string());
        downstream.attach(writer);
        downstream.broadcast("{\"type\":\"digest\"}");
        drop(downstream);

        let reader = BufReader::new(reader);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        assert_eq!(
            lines,
            vec![
                "{\"type\":\"hello\"}".to_string(),
                "{\"type\":\"digest\"}".to_string()
            ]
        );
    }

    #[test]
    fn broadcast_drops_closed_writers() {
        let (writer, reader) = pair();
        drop(reader);
        let mut downstream = Downstream::default();
        downstream.attach(writer);
        // First write may be buffered; second write observes the closed peer.
        downstream.broadcast("a");
        downstream.broadcast("b");
        downstream.broadcast("c");
        assert!(downstream.writers.is_empty());
    }
}
