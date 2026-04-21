use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use winit::event_loop::EventLoopProxy;

use crate::pi::PiSidecarSnapshot;

pub(super) fn read_sidecar_stream(
    stream: UnixStream,
    tx: mpsc::Sender<PiSidecarSnapshot>,
    proxy: EventLoopProxy<()>,
) {
    let reader = BufReader::new(stream);
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(snapshot) = serde_json::from_str::<PiSidecarSnapshot>(trimmed) else {
            continue;
        };
        if snapshot.is_valid() {
            let _ = tx.send(snapshot);
            let _ = proxy.send_event(());
        }
    }
}
