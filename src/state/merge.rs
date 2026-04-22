use crate::pi::ScannedSession;
use crate::state::{compare_sessions, Session};

pub fn merge_scanned_sessions(current: &mut Vec<Session>, scanned: Vec<ScannedSession>) {
    let mut old = std::mem::take(current);
    let mut next: Vec<Session> = Vec::with_capacity(scanned.len() + old.len());

    for scanned_session in scanned {
        if let Some(index) = next
            .iter()
            .position(|session| session.matches_scan(&scanned_session))
        {
            next[index].apply_scan(scanned_session);
            continue;
        }

        if let Some(index) = old
            .iter()
            .position(|session| session.matches_scan(&scanned_session))
        {
            let mut existing = old.remove(index);
            existing.apply_scan(scanned_session);
            next.push(existing);
        } else {
            next.push(Session::from_scan(scanned_session));
        }
    }

    next.extend(old.into_iter().filter(|session| {
        session.draft
            || session.runtime.running
            || session.runtime.queued
            || session.runtime.last_sidecar_ts_ms > 0
    }));
    next.sort_by(compare_sessions);
    *current = next;
}
