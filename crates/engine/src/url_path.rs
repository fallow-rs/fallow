//! URL path decoding for runtime source locations.

/// A URL path with its `%XX` escapes decoded. `None` when
/// a decoded segment is not UTF-8 or holds a path separator, so an escape
/// cannot name a file outside its segment.
pub fn decode_path(path: &str) -> Option<String> {
    let segments = path
        .split('/')
        .map(|segment| {
            let decoded = percent_encoding::percent_decode_str(segment)
                .decode_utf8()
                .ok()?;
            (!decoded.contains(['/', '\\'])).then(|| decoded.into_owned())
        })
        .collect::<Option<Vec<String>>>()?;
    Some(segments.join("/"))
}
