//! Path-based Next.js App Router route-tree primitive.
//!
//! Shared by the `route-collision` and `dynamic-segment-name-conflict`
//! detectors. The App Router URL a file resolves to is a pure function of its
//! path, so this module needs NO AST, no imports, and no type information: it
//! classifies a discovered file as an App Router convention file, anchors it to
//! its app-root, and decomposes the directory segments between the app-root and
//! the file so each detector can compute its own bucket key.
//!
//! # App-root anchoring (the load-bearing false-positive gate)
//!
//! The app-root is anchored on a DISCOVERED workspace package root, NEVER on a
//! bare directory named `app`. A monorepo `libs/feature-shell/src/app/` library
//! folder must not become a phantom app-root, and two independent Next apps
//! (`apps/web/app/about/page.tsx` and `apps/admin/app/about/page.tsx`) must land
//! in DIFFERENT app-roots so their shared `/about` URL is not a false collision.
//! [`classify_route_file`] only recognizes `<pkgRoot>/app/...` and
//! `<pkgRoot>/src/app/...`, where `pkgRoot` is one of the package roots the
//! caller passes in (project root + every discovered workspace package).
//!
//! # pageExtensions
//!
//! Route files are recognized by a basename STEM (`page`, `route`, ...) plus a
//! route-capable extension ([`ROUTE_EXTENSIONS`]). Matching the stem rather than
//! a hardcoded `page.tsx` literal handles `.mdx` content routes and most custom
//! `pageExtensions` while rejecting colocated `page.module.css` / `page.test.tsx`
//! files (their stem is `page.module` / `page.test`, not `page`). Exotic custom
//! `pageExtensions` outside [`ROUTE_EXTENSIONS`] are out of scope and fail toward
//! false-negative.

use std::path::Path;

/// File extensions Next.js can treat as a route file. Covers the default
/// `pageExtensions` (`js`/`jsx`/`ts`/`tsx`), the module variants, and the
/// content extensions `mdx`/`md`.
const ROUTE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "mdx", "md",
];

/// App Router convention basenames that OWN a URL (the route-collision leaves:
/// a URL can have at most one owner, whether a Page or a Route Handler).
const URL_LEAF_STEMS: &[&str] = &["page", "route"];

/// App Router convention basenames that DECORATE a segment without owning a URL.
/// They never collide, but they still prove a directory segment exists, so they
/// participate in dynamic-segment-name-conflict detection.
const DECORATOR_STEMS: &[&str] = &[
    "layout",
    "template",
    "loading",
    "error",
    "not-found",
    "default",
    "global-error",
    "global-not-found",
    "forbidden",
    "unauthorized",
    // Metadata convention files: not URL owners, but evidence of the directory.
    "icon",
    "apple-icon",
    "opengraph-image",
    "twitter-image",
    "manifest",
    "sitemap",
    "robots",
];

/// The role of a recognized App Router convention file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteRole {
    /// A `page` or `route` file: owns the URL its segments resolve to.
    UrlLeaf,
    /// A `layout` / `loading` / metadata / other convention file: decorates the
    /// segment but does not own a URL.
    Decorator,
}

/// The flavor of a dynamic route segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynKind {
    /// `[param]`.
    Required,
    /// `[...param]`.
    CatchAll,
    /// `[[...param]]`.
    OptionalCatchAll,
}

/// One directory segment between the app-root and the route file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteSegment<'a> {
    /// `(group)`: transparent to the URL.
    Group(&'a str),
    /// `@slot`: a parallel-route slot; does not contribute to the URL but does
    /// fork the render target so two leaves in different slots never collide.
    Slot(&'a str),
    /// `[param]` / `[...param]` / `[[...param]]`. `raw` is the segment as
    /// written (including brackets) so distinct param names stay distinct.
    Dynamic { raw: &'a str, kind: DynKind },
    /// An ordinary literal path segment.
    Literal(&'a str),
}

/// A classified App Router route file: its app-root, role, and the directory
/// segments between the app-root and the file.
#[derive(Debug, Clone)]
pub struct ClassifiedRoute<'a> {
    /// Absolute app-root key (`<pkgRoot>/app` or `<pkgRoot>/src/app`), used only
    /// as a bucket prefix so two app-roots never share a bucket. Never
    /// serialized.
    pub app_root: String,
    /// Whether this file owns a URL (`page`/`route`) or merely decorates a
    /// segment.
    pub role: RouteRole,
    /// Directory segments between the app-root and the file, in order.
    pub segments: Vec<RouteSegment<'a>>,
}

/// A single dynamic segment occurrence, used by dynamic-segment-name-conflict:
/// the slot it lives in, the parent URL position before it, and its spelling.
#[derive(Debug, Clone)]
pub struct DynamicOccurrence {
    /// Parallel-slot path the segment lives under (`""` for the implicit
    /// children slot). Two dynamic segments in different slots are different
    /// positions and never conflict.
    pub slot_key: String,
    /// The parent URL position the dynamic segment is a direct child of, with
    /// route groups stripped (e.g. `/shop` for `/shop/[id]`; `/` at the root).
    pub position: String,
    /// The dynamic segment as written (`[id]`, `[...slug]`, `[[...slug]]`).
    pub spelling: String,
}

impl ClassifiedRoute<'_> {
    /// `true` when this file owns a URL (a route-collision participant).
    #[must_use]
    pub const fn is_url_leaf(&self) -> bool {
        matches!(self.role, RouteRole::UrlLeaf)
    }

    /// The route-collision bucket key: `(app_root, slot_key, url)`. Two URL
    /// leaves sharing this triple collide. Route groups are stripped; slots
    /// fork into `slot_key`; dynamic segments keep their written name so
    /// `[id]` and `[slug]` do not collide here (that is the
    /// dynamic-segment-name-conflict detector's job).
    #[must_use]
    pub fn collision_bucket(&self) -> (String, String, String) {
        let mut slots: Vec<&str> = Vec::new();
        let mut url_parts: Vec<&str> = Vec::new();
        for seg in &self.segments {
            match seg {
                RouteSegment::Group(_) => {}
                RouteSegment::Slot(name) => slots.push(name),
                RouteSegment::Dynamic { raw, .. } => url_parts.push(raw),
                RouteSegment::Literal(name) => url_parts.push(name),
            }
        }
        (
            self.app_root.clone(),
            join_slot_key(&slots),
            join_url(&url_parts),
        )
    }

    /// Every dynamic segment along this file's path, paired with the slot it
    /// lives under and the parent URL position it is a direct child of. Used by
    /// the dynamic-segment-name-conflict detector to group sibling dynamic
    /// segments by position.
    #[must_use]
    pub fn dynamic_occurrences(&self) -> Vec<DynamicOccurrence> {
        let mut slots: Vec<&str> = Vec::new();
        let mut url_parts: Vec<&str> = Vec::new();
        let mut out = Vec::new();
        for seg in &self.segments {
            match seg {
                RouteSegment::Group(_) => {}
                RouteSegment::Slot(name) => slots.push(name),
                RouteSegment::Dynamic { raw, .. } => {
                    out.push(DynamicOccurrence {
                        slot_key: join_slot_key(&slots),
                        position: join_url(&url_parts),
                        spelling: (*raw).to_string(),
                    });
                    url_parts.push(raw);
                }
                RouteSegment::Literal(name) => url_parts.push(name),
            }
        }
        out
    }
}

/// Join slot names into a stable key (`""` when there are no slots).
fn join_slot_key(slots: &[&str]) -> String {
    slots.join("/")
}

/// Join URL parts into a leading-slash path (`/` when empty).
fn join_url(parts: &[&str]) -> String {
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// Classify a discovered file as an App Router route file, anchored to one of
/// `pkg_roots`.
///
/// Returns `None` when the file is not an App Router convention file, is not
/// under any `<pkgRoot>/app` or `<pkgRoot>/src/app`, lives under a private
/// `_folder` segment (opted out of routing), or lives under an intercepting
/// marker segment (`(.)` / `(..)` / `(...)`, which intentionally shadows another
/// URL and so must never be reported as a collision).
#[must_use]
pub fn classify_route_file<'a>(path: &'a Path, pkg_roots: &[&Path]) -> Option<ClassifiedRoute<'a>> {
    // Anchor on the LONGEST matching package root so a nested package wins over
    // an ancestor (and the project root).
    let pkg_root = pkg_roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())?;

    let rel = path.strip_prefix(pkg_root).ok()?;
    let comps: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect();

    // The App Router dir is `<pkgRoot>/app` or `<pkgRoot>/src/app`. Determine the
    // index of the first directory segment AFTER the app-root.
    let (app_root_suffix, first_seg_idx) = match comps.split_first() {
        Some((&"app", _)) => ("app", 1),
        Some((&"src", rest)) if rest.first() == Some(&"app") => ("src/app", 2),
        _ => return None,
    };

    // Need at least the filename after the app-root.
    if comps.len() <= first_seg_idx {
        return None;
    }
    let filename = *comps.last()?;
    let role = classify_filename(filename)?;

    // Directory segments strictly between the app-root and the filename.
    let dir_segments = &comps[first_seg_idx..comps.len() - 1];

    let mut segments = Vec::with_capacity(dir_segments.len());
    for &seg in dir_segments {
        // Private folder or intercepting marker => the file is not a routable
        // collision/conflict participant.
        if is_private(seg) || is_intercepting(seg) {
            return None;
        }
        segments.push(classify_segment(seg));
    }

    let app_root = format!("{}/{app_root_suffix}", pkg_root.display());
    Some(ClassifiedRoute {
        app_root,
        role,
        segments,
    })
}

/// Classify a route file's basename into its role, or `None` if it is not a
/// recognized App Router convention file with a route-capable extension.
fn classify_filename(filename: &str) -> Option<RouteRole> {
    let (stem, ext) = filename.rsplit_once('.')?;
    if !ROUTE_EXTENSIONS.contains(&ext) {
        return None;
    }
    if URL_LEAF_STEMS.contains(&stem) {
        Some(RouteRole::UrlLeaf)
    } else if DECORATOR_STEMS.contains(&stem) {
        Some(RouteRole::Decorator)
    } else {
        None
    }
}

/// A private folder segment (`_components`): opts the subtree out of routing.
fn is_private(seg: &str) -> bool {
    seg.starts_with('_')
}

/// An intercepting-route marker segment (`(.)photo`, `(..)photo`, `(...)photo`,
/// `(..)(..)photo`): intentionally shadows another URL during soft navigation.
fn is_intercepting(seg: &str) -> bool {
    seg.starts_with("(.)") || seg.starts_with("(..)") || seg.starts_with("(...)")
}

/// Classify a single directory segment.
fn classify_segment(seg: &str) -> RouteSegment<'_> {
    if let Some(name) = seg.strip_prefix('@') {
        return RouteSegment::Slot(name);
    }
    if seg.starts_with('[') && seg.ends_with(']') {
        let kind = if seg.starts_with("[[...") {
            DynKind::OptionalCatchAll
        } else if seg.starts_with("[...") {
            DynKind::CatchAll
        } else {
            DynKind::Required
        };
        return RouteSegment::Dynamic { raw: seg, kind };
    }
    // A route group is `(name)` and is NOT an intercepting marker (those are
    // filtered out earlier in `classify_route_file`).
    if seg.starts_with('(') && seg.ends_with(')') {
        return RouteSegment::Group(&seg[1..seg.len() - 1]);
    }
    RouteSegment::Literal(seg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn pkg(root: &str) -> PathBuf {
        PathBuf::from(root)
    }

    fn classify<'a>(path: &'a Path, roots: &[&Path]) -> Option<ClassifiedRoute<'a>> {
        classify_route_file(path, roots)
    }

    #[test]
    fn route_group_pages_share_url_within_one_app_root() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let a = PathBuf::from("/repo/app/(marketing)/about/page.tsx");
        let b = PathBuf::from("/repo/app/(shop)/about/page.tsx");
        let ca = classify(&a, &roots).unwrap();
        let cb = classify(&b, &roots).unwrap();
        assert!(ca.is_url_leaf() && cb.is_url_leaf());
        assert_eq!(ca.collision_bucket(), cb.collision_bucket());
        assert_eq!(ca.collision_bucket().2, "/about");
    }

    #[test]
    fn src_app_prefix_is_recognized() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let p = PathBuf::from("/repo/src/app/blog/page.tsx");
        let c = classify(&p, &roots).unwrap();
        assert_eq!(c.app_root, "/repo/src/app");
        assert_eq!(c.collision_bucket().2, "/blog");
    }

    #[test]
    fn parallel_slots_do_not_share_collision_bucket() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let a = PathBuf::from("/repo/app/@team/members/page.tsx");
        let b = PathBuf::from("/repo/app/@analytics/members/page.tsx");
        let ca = classify(&a, &roots).unwrap();
        let cb = classify(&b, &roots).unwrap();
        // Same URL, different slot => different bucket => no collision.
        assert_eq!(ca.collision_bucket().2, "/members");
        assert_eq!(cb.collision_bucket().2, "/members");
        assert_ne!(ca.collision_bucket().1, cb.collision_bucket().1);
    }

    #[test]
    fn monorepo_two_apps_have_distinct_app_roots() {
        let web = pkg("/repo/apps/web");
        let admin = pkg("/repo/apps/admin");
        let roots: Vec<&Path> = vec![web.as_path(), admin.as_path()];
        let a = PathBuf::from("/repo/apps/web/app/about/page.tsx");
        let b = PathBuf::from("/repo/apps/admin/app/about/page.tsx");
        let ca = classify(&a, &roots).unwrap();
        let cb = classify(&b, &roots).unwrap();
        assert_ne!(ca.collision_bucket().0, cb.collision_bucket().0);
    }

    #[test]
    fn library_app_folder_is_not_an_app_root() {
        // `libs/feature-shell` is a package; its `src/app/` IS its app-root, but
        // a stray `app/` that is not anchored on a package root is not. Here we
        // assert a file under a non-package `app/` (no matching pkg root) is
        // None.
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        // `/repo/libs/feature-shell/app/widget.ts` -> rel starts with `libs`,
        // not `app`/`src/app`, so it is not a route file.
        let p = PathBuf::from("/repo/libs/feature-shell/app/widget.ts");
        assert!(classify(&p, &roots).is_none());
    }

    #[test]
    fn private_folder_excluded() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let p = PathBuf::from("/repo/app/_components/page.tsx");
        assert!(classify(&p, &roots).is_none());
    }

    #[test]
    fn intercepting_marker_excluded() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        for seg in ["(.)photo", "(..)photo", "(...)photo"] {
            let p = PathBuf::from(format!("/repo/app/feed/{seg}/[id]/page.tsx"));
            assert!(classify(&p, &roots).is_none(), "should exclude {seg}");
        }
    }

    #[test]
    fn colocated_non_route_files_rejected() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        for name in ["page.test.tsx", "page.module.css", "helpers.ts", "page.css"] {
            let p = PathBuf::from(format!("/repo/app/about/{name}"));
            assert!(classify(&p, &roots).is_none(), "should reject {name}");
        }
    }

    #[test]
    fn mdx_route_recognized() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let p = PathBuf::from("/repo/app/docs/page.mdx");
        assert!(classify(&p, &roots).unwrap().is_url_leaf());
    }

    #[test]
    fn page_and_route_share_url_owner_namespace() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let page = PathBuf::from("/repo/app/(a)/about/page.tsx");
        let route = PathBuf::from("/repo/app/(b)/about/route.ts");
        let cp = classify(&page, &roots).unwrap();
        let cr = classify(&route, &roots).unwrap();
        assert!(cp.is_url_leaf() && cr.is_url_leaf());
        assert_eq!(cp.collision_bucket(), cr.collision_bucket());
    }

    #[test]
    fn dynamic_names_kept_distinct_in_collision_bucket() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let id = PathBuf::from("/repo/app/(a)/[id]/page.tsx");
        let slug = PathBuf::from("/repo/app/(b)/[slug]/page.tsx");
        let cid = classify(&id, &roots).unwrap();
        let cslug = classify(&slug, &roots).unwrap();
        // Different dynamic names => different collision buckets (the conflict is
        // the sibling detector's job, not a route-collision).
        assert_ne!(cid.collision_bucket(), cslug.collision_bucket());
    }

    #[test]
    fn dynamic_occurrence_position_and_spelling() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let p = PathBuf::from("/repo/app/shop/[id]/edit/page.tsx");
        let c = classify(&p, &roots).unwrap();
        let occ = c.dynamic_occurrences();
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].position, "/shop");
        assert_eq!(occ[0].spelling, "[id]");
        assert_eq!(occ[0].slot_key, "");
    }

    #[test]
    fn decorator_files_classified_but_not_leaves() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let p = PathBuf::from("/repo/app/shop/[id]/layout.tsx");
        let c = classify(&p, &roots).unwrap();
        assert!(!c.is_url_leaf());
        // A layout still proves the [id] dynamic dir exists.
        assert_eq!(c.dynamic_occurrences()[0].spelling, "[id]");
    }

    #[test]
    fn catch_all_kinds_parsed() {
        let root = pkg("/repo");
        let roots: Vec<&Path> = vec![root.as_path()];
        let catch = PathBuf::from("/repo/app/docs/[...slug]/page.tsx");
        let opt = PathBuf::from("/repo/app/docs/[[...slug]]/page.tsx");
        assert_eq!(
            classify(&catch, &roots).unwrap().dynamic_occurrences()[0].spelling,
            "[...slug]"
        );
        assert_eq!(
            classify(&opt, &roots).unwrap().dynamic_occurrences()[0].spelling,
            "[[...slug]]"
        );
    }
}
