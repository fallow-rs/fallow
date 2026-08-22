use fallow_config::Severity;

use crate::common::{create_config, create_config_with_rules, fixture_path};

/// Run the issue #2355 fixture (Astro plugin enabled so `src/pages/**` is an
/// entry surface) and return every reported unused export as a
/// `(file name, export name)` pair. File names are unique across the fixture,
/// so no path separators take part in the comparison.
fn reported_unused_exports() -> Vec<(String, String)> {
    let root = fixture_path("issue-2355-astro-mdx-member-tags");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_exports
        .iter()
        .map(|e| {
            let file = e
                .export
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            (file, e.export.export_name.clone())
        })
        .collect()
}

fn is_reported(reported: &[(String, String)], file: &str, name: &str) -> bool {
    reported
        .iter()
        .any(|(reported_file, reported_name)| reported_file == file && reported_name == name)
}

/// Issue #2355: a namespace import rendered only through a member-expression
/// tag in Astro markup (`<SC.UsedStyle />`) or an MDX body (`<MD.UsedBlock />`)
/// must credit the referenced export, while genuinely unused siblings keep
/// reporting, matching the `.tsx` semantics from #2348.
///
/// Precision retained (every mention of the binding is a dotted tag or a
/// parsed expression, so the binding narrows and its unused sibling reports):
/// - entry `index.astro` (`SC`), non-entry `Card.astro` (`S`),
/// - entry `guide.mdx` (`MD`), non-entry `notes.mdx` (`Doc`),
/// - non-entry `Mixed.astro` / `mixed.mdx` mixing a dotted tag with an
///   attribute expression, a call, or a multi-line block (`Moon` credited,
///   the sibling reports), while a namespace passed whole (`all={NS}`) keeps
///   every export credited.
#[test]
fn astro_and_mdx_member_tags_credit_namespace_exports() {
    let reported = reported_unused_exports();

    for (file, name) in [
        ("style.ts", "ActuallyUnusedStyle"),
        ("card-style.ts", "UnusedSibling"),
        ("md-style.ts", "UnusedMdBlock"),
        ("doc-style.ts", "UnusedDocSibling"),
        ("attr-icons.ts", "AttrUnused"),
        ("call-icons.ts", "CallUnused"),
        ("md-attr-icons.ts", "MdAttrUnused"),
        ("md-call-icons.ts", "MdCallUnused"),
        ("md-multi-icons.ts", "MdMultiUnused"),
    ] {
        assert!(
            is_reported(&reported, file, name),
            "{file}:{name}: a binding used only through dotted tags and parsed expressions must narrow so its unused sibling reports: {reported:?}"
        );
    }

    for (file, name) in [
        ("style.ts", "UsedStyle"),
        ("style.ts", "Layout"),
        ("card-style.ts", "Wrapper"),
        ("md-style.ts", "UsedBlock"),
        ("doc-style.ts", "Note"),
        ("attr-icons.ts", "Star"),
        ("attr-icons.ts", "Moon"),
        ("call-icons.ts", "Star"),
        ("call-icons.ts", "Moon"),
        ("md-attr-icons.ts", "Star"),
        ("md-attr-icons.ts", "Moon"),
        ("md-call-icons.ts", "Star"),
        ("md-call-icons.ts", "Moon"),
        ("md-multi-icons.ts", "Star"),
        ("md-multi-icons.ts", "Moon"),
        ("whole-icons.ts", "WholeShielded"),
        ("md-whole-icons.ts", "MdWholeShielded"),
    ] {
        assert!(
            !is_reported(&reported, file, name),
            "{file}:{name}: a member used through a dotted tag, a parsed expression, or a whole-object pass must stay credited: {reported:?}"
        );
    }
}

/// Issue #2355 completeness guard: a mention of an import binding that the
/// template passes could not classify keeps the binding on the mark-all path,
/// so no used member is ever reported, for an entry page and a non-entry
/// consumer of each kind:
/// - Astro (`shapes.astro` in pages, `Shapes.astro` in components):
///   `<style define:vars={{ accent: NS.accent }}>`, `<script define:vars={{
///   moon: NS.Moon }}>`, `<script is:inline set:html={NS.code}>`, `<script
///   define:vars={{ NS }}>`, and a CSS module class read through
///   `define:vars`,
/// - MDX (`shapes.mdx` in pages and docs): a template literal attribute
///   `title={`Moon: ${NS.Moon}`}`, a whole pass inside a literal
///   `{`${JSON.stringify(NS)}`}`, a CSS module class read through
///   `className={`${styles.spare} x`}`, a fenced code block mentioning
///   `<NS.Foo />`, and an inline code span mentioning `<NS.CodeOnly />`.
#[test]
fn astro_and_mdx_unexplained_mentions_keep_mark_all() {
    let reported = reported_unused_exports();

    for prefix in ["ea", "na"] {
        for (file, name) in [
            ("style-vars.ts", "Star"),
            ("style-vars.ts", "accent"),
            ("script-vars.ts", "Star"),
            ("script-vars.ts", "Moon"),
            ("set-html.ts", "Star"),
            ("set-html.ts", "code"),
            ("whole-vars.ts", "Star"),
            ("whole-vars.ts", "Moon"),
            ("whole-vars.ts", "Extra"),
            ("card.module.css", "root"),
            ("card.module.css", "accent"),
        ] {
            let file = format!("{prefix}-{file}");
            assert!(
                !is_reported(&reported, &file, name),
                "{file}:{name}: a mention on a masked <style> / <script> opening tag must keep the binding on mark-all: {reported:?}"
            );
        }
    }

    for prefix in ["em", "nm"] {
        for (file, name) in [
            ("literal.ts", "Star"),
            ("literal.ts", "Moon"),
            ("literal-whole.ts", "Star"),
            ("literal-whole.ts", "Moon"),
            ("literal-whole.ts", "Extra"),
            ("doc.module.css", "root"),
            ("doc.module.css", "spare"),
            ("fenced.ts", "Star"),
            ("fenced.ts", "Foo"),
            ("inline.ts", "Star"),
            ("inline.ts", "CodeOnly"),
        ] {
            let file = format!("{prefix}-{file}");
            assert!(
                !is_reported(&reported, &file, name),
                "{file}:{name}: a mention inside a template literal, fenced code, or inline code must keep the binding on mark-all: {reported:?}"
            );
        }
    }
}

/// Issue #2355 script-side guard: a bare mention of a namespace in the Astro
/// frontmatter or on an MDX statement line that the visitor does not record
/// as a whole-object use keeps the binding on the mark-all path, so the
/// members reached through the copy (and every sibling) stay credited, for an
/// entry page and a non-entry consumer of each kind:
/// - Astro (`script-shapes.astro` in pages, `ScriptShapes.astro` in
///   components): an alias (`const N = NS`), a cast (`NS as Record<...>`), a
///   call argument (`pick(NS)`), `Object.assign({}, NS)`, an array literal
///   (`[NS]`), and a props object (`{ all: NS }` spread into a component),
/// - MDX (`script-shapes.mdx` in pages and docs): `export const all = NS`, a
///   JSX attribute on an exported component (`<Callout all={NS} />`), and a
///   default-export layout passing the namespace whole.
///
/// Precision retained: a namespace used in the frontmatter or on a statement
/// line only through a dotted access (`const moon = NS.Moon`) still narrows,
/// so its unused sibling reports.
#[test]
fn astro_and_mdx_script_mentions_keep_mark_all() {
    let reported = reported_unused_exports();

    for (prefixes, shapes, dotted) in [
        (
            ["ea", "na"],
            &[
                "fm-alias",
                "fm-as-cast",
                "fm-call-arg",
                "fm-object-assign",
                "fm-array-literal",
                "fm-props-pass",
            ][..],
            "fm-dotted",
        ),
        (
            ["em", "nm"],
            &["stmt-whole", "stmt-attr", "stmt-default"][..],
            "stmt-dotted",
        ),
    ] {
        for prefix in prefixes {
            for shape in shapes {
                let file = format!("{prefix}-{shape}.ts");
                for name in ["Star", "Moon", "Shielded"] {
                    assert!(
                        !is_reported(&reported, &file, name),
                        "{file}:{name}: a bare script mention must keep the binding on mark-all: {reported:?}"
                    );
                }
            }
            let file = format!("{prefix}-{dotted}.ts");
            for name in ["Star", "Moon"] {
                assert!(
                    !is_reported(&reported, &file, name),
                    "{file}:{name}: a dotted script access must stay credited: {reported:?}"
                );
            }
            assert!(
                is_reported(&reported, &file, "DottedUnused"),
                "{file}:DottedUnused: a binding used only through dotted accesses must still narrow: {reported:?}"
            );
        }
    }
}

/// Issue #2355: a documentation sentence in MDX prose naming
/// `process.env.API_KEY` is not an environment read. The prose scan records
/// dotted chains only for import-local roots, so the MDX module never becomes
/// a secret source and a `"use client"` page importing it reports no
/// `client-server-leak`, while a sibling client importing a module that really
/// reads the variable still does.
#[test]
fn mdx_prose_env_mention_is_not_a_secret_source() {
    let root = fixture_path("issue-2355-mdx-prose-env-mention");
    let config = create_config_with_rules(root, |rules| {
        rules.security_client_server_leak = Severity::Warn;
    });
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let anchors: Vec<String> = results
        .security_findings
        .iter()
        .map(|finding| finding.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        anchors.iter().any(|path| path.ends_with("app/control.tsx")),
        "the client importing a real process.env read must report: {anchors:?}"
    );
    assert!(
        !anchors.iter().any(|path| path.ends_with("app/page.tsx")),
        "an unfenced env mention in MDX prose must not make the module a secret source: {anchors:?}"
    );
}
