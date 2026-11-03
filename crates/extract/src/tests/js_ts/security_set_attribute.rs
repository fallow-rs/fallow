use crate::tests::parse_ts as parse_source;

fn sink_paths(source: &str) -> Vec<String> {
    parse_source(source)
        .security_sinks
        .iter()
        .map(|sink| sink.callee_path.clone())
        .collect()
}

#[test]
fn script_set_attribute_src_becomes_script_element_src() {
    let paths = sink_paths(
        "export const load = (url: string) => {\n  const s = document.createElement('script');\n  s.setAttribute('SRC', url);\n};\n",
    );
    assert!(
        paths.iter().any(|p| p == "HTMLScriptElement.src"),
        "expected HTMLScriptElement.src, observed {paths:?}"
    );
}

#[test]
fn image_set_attribute_src_is_not_a_sink() {
    let paths = sink_paths(
        "export const show = (url: string) => {\n  const img = document.createElement('img');\n  img.setAttribute('src', url);\n};\n",
    );
    assert!(
        !paths.iter().any(|p| p.rsplit('.').next() == Some("src")),
        "an image src must not become a sink, observed {paths:?}"
    );
}

#[test]
fn set_attribute_srcdoc_stays_captured() {
    let paths = sink_paths(
        "export const render = (frame: HTMLIFrameElement, html: string) => {\n  frame.setAttribute('srcdoc', html);\n};\n",
    );
    assert!(
        paths.iter().any(|p| p == "frame.srcdoc"),
        "expected frame.srcdoc, observed {paths:?}"
    );
}
