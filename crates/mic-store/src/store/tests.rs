use std::num::NonZeroU32;

use mic_message::{Message, MessageBody, MessageId, PersonId, SessionId};

use super::cut_page;

/// `u` = UserInput，`.` = 其它；按 id 升序，返回倒序的末尾 `cap + 1` 条。
fn newest_first(shape: &str, cap: u32) -> Vec<Message> {
    let mut v: Vec<Message> = shape
        .chars()
        .enumerate()
        .map(|(i, c)| Message {
            id: MessageId(i as i64 + 1),
            session_id: SessionId(1),
            body: if c == 'u' {
                MessageBody::UserInput {
                    person: PersonId(1),
                    parts: Vec::new(),
                }
            } else {
                MessageBody::HarnessNote {
                    text: String::new(),
                }
            },
            created_at: 0,
            delivered_at: None,
        })
        .collect();
    v.reverse();
    v.truncate(cap as usize + 1);
    v
}

fn page(shape: &str, rounds: u32, cap: u32) -> (Vec<i64>, bool) {
    let p = cut_page(
        newest_first(shape, cap),
        NonZeroU32::new(rounds).unwrap(),
        NonZeroU32::new(cap).unwrap(),
    );
    (p.messages.iter().map(|m| m.id.0).collect(), p.has_earlier)
}

#[test]
fn starts_at_round_head() {
    assert_eq!(page("u..u.u..", 2, 10), (vec![4, 5, 6, 7, 8], true));
}

#[test]
fn cap_cuts_inside_round() {
    assert_eq!(page("u.......", 2, 3), (vec![6, 7, 8], true));
}

#[test]
fn whole_session_when_short() {
    assert_eq!(page(".u.u.", 5, 10), (vec![1, 2, 3, 4, 5], false));
    assert_eq!(page("u..", 1, 3), (vec![1, 2, 3], false));
    assert_eq!(page("..u", 2, 3), (vec![1, 2, 3], false));
}

#[test]
fn earliest_round_exact() {
    assert_eq!(page("u.u.", 2, 10), (vec![1, 2, 3, 4], false));
}

#[test]
fn empty() {
    assert_eq!(page("", 2, 3), (vec![], false));
}
