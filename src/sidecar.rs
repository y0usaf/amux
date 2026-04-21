#[path = "sidecar/stream.rs"]
mod stream;

use std::fs;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use winit::event_loop::EventLoopProxy;

use crate::pi::PiSidecarSnapshot;
use stream::read_sidecar_stream;

pub struct SidecarListener {
    socket_path: PathBuf,
    rx: Receiver<PiSidecarSnapshot>,
}

impl SidecarListener {
    pub fn start(proxy: EventLoopProxy<()>, socket_path: PathBuf) -> anyhow::Result<Self> {
        let Some(parent) = socket_path.parent() else {
            anyhow::bail!("sidecar socket has no parent: {}", socket_path.display());
        };
        fs::create_dir_all(parent)?;
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;
        let (tx, rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("pi-harness-sidecar-listener".into())
            .spawn(move || {
                for incoming in listener.incoming() {
                    match incoming {
                        Ok(stream) => {
                            let tx = tx.clone();
                            let proxy = proxy.clone();
                            let _ = std::thread::Builder::new()
                                .name("pi-harness-sidecar-stream".into())
                                .spawn(move || read_sidecar_stream(stream, tx, proxy));
                        }
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self { socket_path, rx })
    }

    pub fn try_recv(&self) -> Option<PiSidecarSnapshot> {
        self.rx.try_recv().ok()
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
