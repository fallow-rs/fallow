/// JSX prop names that the extractor follows through a spread of a local
/// object literal (`const props = { dangerouslySetInnerHTML: x }; <div {...props} />`).
///
/// The security catalogue owns the `jsx-attr` sink rows. A catalogue test
/// asserts that every `jsx-attr` row pattern is in this list, so a new JSX sink
/// row cannot silently lose spread coverage.
pub const JSX_SPREAD_SINK_PROPS: &[&str] = &[
    "dangerouslySetInnerHTML",
    "srcDoc",
    "srcdoc",
    "injectedJavaScript",
];
