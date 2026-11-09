//! Task identities: a UUID names the document; `T-` and six base-36
//! characters name the task for people and agents, without coordination.
const ALPHABET: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static FORCED: std::cell::RefCell<std::collections::VecDeque<String>> = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}

/// Makes the next short IDs drawn on this thread these, in order (tests of
/// this and other crates: a draw that collides with a task).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub fn force_short_ids_for_test(ids: &[&str]) {
    FORCED.with(|forced| {
        forced
            .borrow_mut()
            .extend(ids.iter().map(|id| (*id).to_owned()))
    });
}

/// A random short ID. Callers redraw it when it collides with a local task.
pub fn short_id() -> String {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(id) = FORCED.with(|forced| forced.borrow_mut().pop_front()) {
        return id;
    }
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let mut value = u64::from_le_bytes(bytes[..8].try_into().expect("eight bytes"));
    let mut id = String::from("T-");
    for _ in 0..6 {
        id.push(ALPHABET[(value % 36) as usize] as char);
        value /= 36;
    }
    id
}

pub fn is_short_id(value: &str) -> bool {
    value.len() == 8 && value.starts_with("T-") && value[2..].bytes().all(|b| ALPHABET.contains(&b))
}

pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ids_are_well_formed_and_vary() {
        let ids: std::collections::BTreeSet<_> = (0..200).map(|_| short_id()).collect();
        assert!(ids.iter().all(|id| is_short_id(id)));
        assert!(ids.len() > 190);
        assert!(!is_short_id("T-ABCDEF"));
        assert!(!is_short_id("T-abc"));
        assert!(is_uuid(&new_uuid()));
        assert!(!is_uuid("not-a-uuid"));
    }
}
