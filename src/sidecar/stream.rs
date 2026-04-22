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
    read_sidecar_stream_from_reader(reader, tx, || {
        let _ = proxy.send_event(());
    });
}

fn read_sidecar_stream_from_reader<R, F>(
    reader: BufReader<R>,
    tx: mpsc::Sender<PiSidecarSnapshot>,
    mut notify: F,
) where
    R: std::io::Read,
    F: FnMut(),
{
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
            notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::read_sidecar_stream_from_reader;
    use crate::pi::PiSessionStage;
    use std::io::{BufReader, Cursor};
    use std::sync::mpsc;

    #[test]
    fn sidecar_stream_skips_blank_and_invalid_lines_and_emits_valid_snapshots() {
        let input = b"\nnot-json\n{\"type\":\"other\",\"sessionId\":\"s1\",\"stage\":\"idle\"}\n{\"type\":\"snapshot\",\"sessionId\":\"s2\",\"stage\":\"thinking\"}\n";
        let (tx, rx) = mpsc::channel();
        let mut notifications = 0;

        read_sidecar_stream_from_reader(BufReader::new(Cursor::new(&input[..])), tx, || {
            notifications += 1;
        });

        let snapshots: Vec<_> = rx.try_iter().collect();
        assert_eq!(notifications, 1);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].session_id, "s2");
        assert_eq!(snapshots[0].stage, PiSessionStage::Thinking);
    }
}
