use super::*;

fn parse(source: &str) -> ElysiaFile {
    elysia_file(Path::new("app.ts"), source)
}

fn routes(source: &str) -> Vec<(Option<&'static str>, String)> {
    parse(source)
        .routes
        .into_iter()
        .map(|route| (route.method, route.path))
        .collect()
}

#[test]
fn file_route_id_reads_the_first_literal_argument() {
    let path = Path::new("src/routes/repo.$repo.tsx");
    let source = "import { createFileRoute } from '@tanstack/react-router';\nexport const Route = createFileRoute('/repo/$repo')({ component: () => null });\n";
    assert_eq!(file_route_id(path, source).as_deref(), Some("/repo/$repo"));
    let lazy = "export const Route = createLazyFileRoute(`/about`)({});\n";
    assert_eq!(file_route_id(path, lazy).as_deref(), Some("/about"));
}

#[test]
fn file_route_id_skips_a_computed_or_missing_argument() {
    let path = Path::new("src/routes/a.tsx");
    assert_eq!(
        file_route_id(path, "export const Route = createFileRoute(id)({});\n"),
        None
    );
    assert_eq!(
        file_route_id(path, "export const Route = createFileRoute()({});\n"),
        None
    );
    assert_eq!(file_route_id(path, "export const x = 1;\n"), None);
}

#[test]
fn elysia_chain_with_prefix_group_and_route_call() {
    let source = "import { Elysia } from 'elysia';\nnew Elysia({ prefix: '/api' })\n  .get('/', () => 1)\n  .group('v1', (app) => app.post('/repos/:repo', () => 2).all('/any', () => 3))\n  .route('patch', '/items', () => 4)\n  .listen(3000);\n";
    assert_eq!(
        routes(source),
        vec![
            (Some("GET"), "/api/".to_owned()),
            (Some("POST"), "/api/v1/repos/:repo".to_owned()),
            (None, "/api/v1/any".to_owned()),
            (Some("PATCH"), "/api/items".to_owned()),
        ]
    );
}

#[test]
fn elysia_skips_dynamic_paths_and_prefixes() {
    let source = "import { Elysia } from 'elysia';\nconst p = '/x';\nnew Elysia({ prefix: p }).get('/a', () => 1);\nnew Elysia()\n  .get(`/v/${p}`, () => 1)\n  .group(p, (app) => app.get('/b', () => 2))\n  .get('/kept', () => 3);\n";
    assert_eq!(routes(source), vec![(Some("GET"), "/kept".to_owned())]);
}

#[test]
fn elysia_requires_the_import_of_the_class() {
    let source = "class Elysia { get() { return this; } }\nnew Elysia().get('/a', () => 1);\n";
    assert!(parse(source).is_empty());
}

#[test]
fn elysia_records_handlers_mounts_and_exports() {
    let source = "import { Elysia as App } from 'elysia';\nimport { getRepo } from './handlers';\nimport * as h from './more';\nimport { users } from './users';\nconst repos = new App({ prefix: '/repos' }).get('/:repo', getRepo).delete('/:repo', h.remove);\nexport const app = new App().use(repos).use(users).group('/v1', (v) => v.use(users));\nexport default repos;\n";
    let file = parse(source);
    assert_eq!(file.instance_count, 2);
    assert_eq!(
        file.exports,
        vec![("app".to_owned(), 1), ("default".to_owned(), 0)]
    );
    let handlers: Vec<&Reference> = file.routes.iter().map(|route| &route.handler).collect();
    assert_eq!(
        handlers,
        vec![
            &Reference::Name {
                local: "getRepo".to_owned(),
                member: None
            },
            &Reference::Name {
                local: "h".to_owned(),
                member: Some("remove".to_owned())
            },
        ]
    );
    let users = Child::Binding(Reference::Name {
        local: "users".to_owned(),
        member: None,
    });
    assert_eq!(
        file.mounts,
        vec![
            Mount {
                parent: 1,
                base: Some(String::new()),
                child: Child::Local(0)
            },
            Mount {
                parent: 1,
                base: Some(String::new()),
                child: users.clone()
            },
            Mount {
                parent: 1,
                base: Some("/v1".to_owned()),
                child: users
            },
        ]
    );
}

#[test]
fn finish_pattern_normalizes_slashes() {
    assert_eq!(finish_pattern(""), "/");
    assert_eq!(finish_pattern("/api/"), "/api");
    assert_eq!(finish_pattern("/api/v1/repos/:repo"), "/api/v1/repos/:repo");
    assert_eq!(finish_pattern("x"), "/x");
}

#[test]
fn mount_prefixes_follow_every_mount_and_stop_on_a_cycle() {
    let a = (FileId(0), 0);
    let b = (FileId(1), 0);
    let c = (FileId(2), 0);
    let mut parents: FxHashMap<InstanceKey, Vec<(InstanceKey, Option<String>)>> =
        FxHashMap::default();
    parents.insert(b, vec![(a, Some("/api".to_owned())), (a, None)]);
    parents.insert(
        c,
        vec![(b, Some("/v1".to_owned())), (a, Some("/v2".to_owned()))],
    );
    assert_eq!(prefixes_of(a, &parents), vec![String::new()]);
    assert_eq!(
        prefixes_of(c, &parents),
        vec!["/api/v1".to_owned(), "/v2".to_owned()]
    );
    let mut cycle: FxHashMap<InstanceKey, Vec<(InstanceKey, Option<String>)>> =
        FxHashMap::default();
    cycle.insert(a, vec![(b, Some("/a".to_owned()))]);
    cycle.insert(b, vec![(a, Some("/b".to_owned()))]);
    assert!(prefixes_of(a, &cycle).is_empty());
}

type Parents = FxHashMap<InstanceKey, Vec<(InstanceKey, Option<String>)>>;

fn prefixes_of(key: InstanceKey, parents: &Parents) -> Vec<String> {
    mount_prefixes(
        key,
        parents,
        &mut FxHashMap::default(),
        &mut FxHashSet::default(),
    )
}

/// 16 layers of 8 instances. Each instance mounts under every instance of the
/// layer above, so the number of mount paths grows as 8^16.
fn layered_mounts() -> (Parents, InstanceKey) {
    const LAYERS: usize = 16;
    const WIDTH: usize = 8;
    let key = |layer: usize, slot: usize| (FileId(0), layer * WIDTH + slot);
    let mut parents = Parents::default();
    for layer in 1..LAYERS {
        for slot in 0..WIDTH {
            let edges = (0..WIDTH)
                .map(|up| (key(layer - 1, up), Some(format!("/l{layer}s{up}"))))
                .collect();
            parents.insert(key(layer, slot), edges);
        }
    }
    (parents, key(LAYERS - 1, 0))
}

#[test]
fn wide_and_deep_mounts_stay_bounded() {
    let (parents, leaf) = layered_mounts();
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(prefixes_of(leaf, &parents));
    });
    let prefixes = receive
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("mount prefixes of a wide and deep mount graph finish quickly");
    assert_eq!(prefixes.len(), MAX_PREFIXES_PER_INSTANCE);
}

#[test]
fn a_repeated_mount_edge_gives_one_prefix() {
    let root = (FileId(0), 0);
    let child = (FileId(0), 1);
    let mut parents = Parents::default();
    parents.insert(
        child,
        vec![
            (root, Some("/v1".to_owned())),
            (root, Some("/v1".to_owned())),
        ],
    );
    assert_eq!(prefixes_of(child, &parents), vec!["/v1".to_owned()]);
}
