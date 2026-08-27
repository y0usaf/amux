use std::collections::HashMap;
use std::path::PathBuf;

use crate::state::{compare_sessions, ScannedSession, Session};

pub fn merge_scanned_sessions(current: &mut Vec<Session>, scanned: Vec<ScannedSession>) {
    let mut old: Vec<Option<Session>> = std::mem::take(current).into_iter().map(Some).collect();
    let mut old_by_pi_id = HashMap::new();
    let mut old_by_session_file = HashMap::<PathBuf, usize>::new();
    for (index, session) in old.iter().enumerate() {
        let Some(session) = session.as_ref() else {
            continue;
        };
        if let Some(pi_session_id) = session.pi_session_id.as_ref() {
            old_by_pi_id.insert(pi_session_id.clone(), index);
        }
        if let Some(session_file) = session.session_file.as_ref() {
            old_by_session_file.insert(session_file.clone(), index);
        }
    }

    let mut next: Vec<Session> = Vec::with_capacity(scanned.len() + old.len());
    let mut next_by_pi_id = HashMap::new();
    let mut next_by_session_file = HashMap::<PathBuf, usize>::new();

    for scanned_session in scanned {
        if let Some(index) = match_index(&next_by_pi_id, &next_by_session_file, &scanned_session) {
            next[index].apply_scan(scanned_session);
            update_indices(
                &next[index],
                index,
                &mut next_by_pi_id,
                &mut next_by_session_file,
            );
            continue;
        }

        let session = if let Some(index) =
            match_index(&old_by_pi_id, &old_by_session_file, &scanned_session)
        {
            let mut session = old
                .get_mut(index)
                .and_then(Option::take)
                .unwrap_or_else(|| Session::from_scan(scanned_session.clone()));
            session.apply_scan(scanned_session);
            session
        } else {
            Session::from_scan(scanned_session)
        };

        let index = next.len();
        update_indices(
            &session,
            index,
            &mut next_by_pi_id,
            &mut next_by_session_file,
        );
        next.push(session);
    }

    next.extend(old.into_iter().flatten().filter(|session| {
        session.draft
            || session.runtime.running
            || session.runtime.queued
            || session.runtime.interrupted
            || session.runtime.last_sidecar_ts_ms > 0
    }));
    next.sort_by(compare_sessions);
    *current = next;
}

fn match_index(
    by_pi_id: &HashMap<String, usize>,
    by_session_file: &HashMap<PathBuf, usize>,
    scanned_session: &ScannedSession,
) -> Option<usize> {
    by_pi_id
        .get(scanned_session.session_id.as_str())
        .copied()
        .or_else(|| by_session_file.get(&scanned_session.session_file).copied())
}

fn update_indices(
    session: &Session,
    index: usize,
    by_pi_id: &mut HashMap<String, usize>,
    by_session_file: &mut HashMap<PathBuf, usize>,
) {
    if let Some(pi_session_id) = session.pi_session_id.as_ref() {
        by_pi_id.insert(pi_session_id.clone(), index);
    }
    if let Some(session_file) = session.session_file.as_ref() {
        by_session_file.insert(session_file.clone(), index);
    }
}
