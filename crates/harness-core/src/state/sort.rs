use crate::state::Session;

pub fn compare_sessions(a: &Session, b: &Session) -> std::cmp::Ordering {
    let a_top = a.promoted_at_ms > 0;
    let b_top = b.promoted_at_ms > 0;
    let a_recency = if a_top {
        a.promoted_at_ms
    } else {
        a.updated_at_ms
    };
    let b_recency = if b_top {
        b.promoted_at_ms
    } else {
        b.updated_at_ms
    };

    b_top
        .cmp(&a_top)
        .then(b_recency.cmp(&a_recency))
        .then(a.name.cmp(&b.name))
}
