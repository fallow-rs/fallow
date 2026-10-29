use std::ops::Range;

pub(super) struct Block {
    pub start: usize,
    pub attrs: Range<usize>,
    pub body: Range<usize>,
    pub end: usize,
}

/// Resolve quoted opening tags backwards so failed candidates never rescan a suffix.
fn openings(source: &str, prefix: &regex::Regex) -> Vec<Block> {
    let mut tags: Vec<_> = prefix
        .find_iter(source)
        .filter(|m| m.end() < source.len())
        .map(|m| Block {
            start: m.start(),
            attrs: m.end()..m.end(),
            body: m.end()..m.end(),
            end: 0,
        })
        .collect();
    if tags.is_empty() {
        return tags;
    }
    let mut pending = tags.len();
    let mut end = None;
    let mut after_single = None;
    let mut after_double = None;
    for (pos, byte) in source.bytes().enumerate().rev() {
        let after = end;
        end = match byte {
            b'>' => Some(pos + 1),
            b'\'' => {
                let paired = after_single;
                after_single = after;
                paired
            }
            b'"' => {
                let paired = after_double;
                after_double = after;
                paired
            }
            _ => end,
        };
        if pending > 0 && tags[pending - 1].attrs.start == pos {
            pending -= 1;
            if let Some(end) = end {
                tags[pending].attrs.end = end - 1;
                tags[pending].body = end..end;
                tags[pending].end = end;
            }
        }
    }
    tags.retain(|tag| tag.end != 0);
    tags
}

pub(super) fn scan(
    source: &str,
    prefix: &regex::Regex,
    closing: Option<&regex::Regex>,
) -> Vec<Block> {
    let mut tags = openings(source, prefix);
    let closes: Vec<_> = closing
        .into_iter()
        .flat_map(|regex| regex.find_iter(source))
        .collect();
    let mut cursor = 0;
    tags.retain_mut(|tag| {
        if tag.start < cursor {
            return false;
        }
        if closing.is_some() {
            let index = closes.partition_point(|m| m.start() < tag.end);
            let Some(close) = closes.get(index) else {
                return false;
            };
            tag.body.end = close.start();
            tag.end = close.end();
        }
        cursor = tag.end;
        true
    });
    tags
}
