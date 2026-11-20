use super::*;

#[test]
fn used_slots_follow_the_greedy_match() {
    let used = |current, saved| match_severity_slots(current, saved).used;
    assert_eq!(used([0, 1, 0], [1, 1, 0]), [0, 1, 0]);
    assert_eq!(used([1, 0, 0], [0, 0, 1]), [0, 0, 1]);
    assert_eq!(used([0, 0, 1], [1, 0, 0]), [0, 0, 0]);
    assert_eq!(used([2, 1, 0], [1, 1, 2]), [1, 1, 1]);
    assert_eq!(used([5, 5, 5], [1, 1, 1]), [1, 1, 1]);
}

#[test]
fn a_severity_overflows_when_no_compatible_slot_is_free() {
    let overflow = |current, saved| match_severity_slots(current, saved).overflow;
    assert_eq!(overflow([1, 0, 0], [0, 0, 1]), [false, false, false]);
    assert_eq!(overflow([0, 0, 1], [1, 1, 0]), [false, false, true]);
    assert_eq!(overflow([1, 1, 0], [0, 1, 0]), [false, true, false]);
    assert_eq!(overflow([2, 0, 0], [1, 0, 0]), [true, false, false]);
}
