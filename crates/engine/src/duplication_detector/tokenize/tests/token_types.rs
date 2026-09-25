use super::*;

#[test]
fn point_span_near_u32_max() {
    let span = point_span(u32::MAX - 1);
    assert_eq!(span.start, u32::MAX - 1);
    assert_eq!(span.end, u32::MAX);
}
