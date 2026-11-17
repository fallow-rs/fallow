//! A Cloudflare Worker reads a Durable Object through a binding of the
//! global `Env`. These tests cover the facts that let the analysis follow
//! the binding to the Durable Object class (issue #4548).

use fallow_types::extract::{SemanticFact, TypeMemberTypeEntry};

use crate::tests::parse_at_path;

const GENERATED_ENV: &str = r#"
interface __BaseEnv_Env {
  ROOM: DurableObjectNamespace<import("./src/room").Room>;
  NAME: string;
}
declare namespace Cloudflare {
  interface Env extends __BaseEnv_Env {}
}
interface Env extends __BaseEnv_Env {}
"#;

fn entry(type_name: &str, property: &str, property_type: &str) -> TypeMemberTypeEntry {
    TypeMemberTypeEntry {
        type_name: type_name.to_string(),
        property: property.to_string(),
        property_type: property_type.to_string(),
    }
}

/// The `(type, path, member)` of each typed property access fact.
fn typed_property_accesses(path: &str, source: &str) -> Vec<(String, String, String)> {
    let mut accesses: Vec<_> = parse_at_path(path, source)
        .semantic_facts
        .iter()
        .filter_map(|fact| match fact {
            SemanticFact::TypedPropertyMemberAccess(access) => Some((
                access.type_name.clone(),
                access.property_path.clone(),
                access.member.clone(),
            )),
            _ => None,
        })
        .collect();
    accesses.sort();
    accesses.dedup();
    accesses
}

fn access(type_name: &str, path: &str, member: &str) -> (String, String, String) {
    (type_name.to_string(), path.to_string(), member.to_string())
}

#[test]
fn generated_env_inherits_the_namespace_binding_of_its_base() {
    let info = parse_at_path("/project/worker-configuration.d.ts", GENERATED_ENV);
    let room = r#"import("./src/room").Room"#;
    for expected in [
        entry("Env", "ROOM", room),
        entry("__BaseEnv_Env", "ROOM", room),
    ] {
        assert!(
            info.type_member_types.contains(&expected),
            "missing {expected:?}, found: {:?}",
            info.type_member_types
        );
    }
}

#[test]
fn stub_of_a_typed_env_parameter_reads_the_binding() {
    let accesses = typed_property_accesses(
        "/project/src/index.ts",
        r"
            export async function run(env: Env): Promise<void> {
                const stub = env.ROOM.getByName('a');
                await stub.viaLocal();
            }
        ",
    );
    assert!(
        accesses.contains(&access("Env", "ROOM", "viaLocal")),
        "found: {accesses:?}"
    );
}

#[test]
fn env_import_of_cloudflare_workers_reads_the_global_env() {
    let accesses = typed_property_accesses(
        "/project/src/index.ts",
        r"
            import { env } from 'cloudflare:workers';
            export default {
                async fetch(): Promise<Response> {
                    const stub = env.ROOM.get(env.ROOM.idFromName('a'));
                    await stub.viaLocal();
                    await env.ROOM.getByName('b').viaInline();
                    return new Response();
                },
            };
        ",
    );
    for expected in [
        access("Env", "ROOM", "viaLocal"),
        access("Env", "ROOM", "viaInline"),
    ] {
        assert!(
            accesses.contains(&expected),
            "missing {expected:?}, found: {accesses:?}"
        );
    }
}

#[test]
fn env_of_another_module_or_a_parameter_is_not_the_global_env() {
    for source in [
        r"
            import { env } from './config';
            export const run = () => env.ROOM.getByName('a').notEnv();
        ",
        r"
            import { env } from 'cloudflare:workers';
            export const run = (env: unknown) => env.ROOM.getByName('a').notEnv();
        ",
        r"
            import { env } from 'cloudflare:workers';
            export default { fetch(request, env) { return env.ROOM.getByName('a').notEnv(); } };
        ",
    ] {
        let accesses = typed_property_accesses("/project/src/index.ts", source);
        assert!(
            accesses.iter().all(|(_, _, member)| member != "notEnv"),
            "found: {accesses:?} for {source}"
        );
    }
}

#[test]
fn this_env_of_a_worker_class_reads_its_env_type() {
    let accesses = typed_property_accesses(
        "/project/src/index.ts",
        r"
            import { WorkerEntrypoint as Base } from 'cloudflare:workers';
            export class Api extends Base<Env> {
                async inline() {
                    return this.env.ROOM.getByName('a').viaInline();
                }
                async local() {
                    const id = this.env.ROOM.idFromName('a');
                    const stub = this.env.ROOM.get(id);
                    return stub.viaLocal();
                }
                field = () => this.env.ROOM.getByName('a').viaField();
            }
        ",
    );
    for expected in [
        access("Env", "ROOM", "viaInline"),
        access("Env", "ROOM", "viaLocal"),
        access("Env", "ROOM", "viaField"),
    ] {
        assert!(
            accesses.contains(&expected),
            "missing {expected:?}, found: {accesses:?}"
        );
    }
}

#[test]
fn this_env_outside_a_worker_class_reads_nothing() {
    for source in [
        r"
            export class Plain {
                env: any;
                run() { return this.env.ROOM.getByName('a').notEnv(); }
            }
        ",
        r"
            import { DurableObject } from './local';
            export class Fake extends DurableObject<Env> {
                run() { return this.env.ROOM.getByName('a').notEnv(); }
            }
        ",
        r"
            import { DurableObject } from 'cloudflare:workers';
            export class Room extends DurableObject<Env> {
                run() {
                    return function (this: Other) {
                        return this.env.ROOM.getByName('a').notEnv();
                    };
                }
            }
        ",
    ] {
        let accesses = typed_property_accesses("/project/src/index.ts", source);
        assert!(
            accesses.iter().all(|(_, _, member)| member != "notEnv"),
            "found: {accesses:?} for {source}"
        );
    }
}

#[test]
fn current_wrangler_env_merges_both_declarations() {
    let info = parse_at_path(
        "/project/worker-configuration.d.ts",
        r#"
            declare namespace Cloudflare {
                interface Env {
                    ROOM: DurableObjectNamespace<import("./src/room").Room>;
                }
            }
            interface Env extends Cloudflare.Env {
                LOBBY: DurableObjectNamespace<import("./src/room").Lobby>;
            }
        "#,
    );
    for expected in [
        entry("Env", "ROOM", r#"import("./src/room").Room"#),
        entry("Env", "LOBBY", r#"import("./src/room").Lobby"#),
    ] {
        assert!(
            info.type_member_types.contains(&expected),
            "missing {expected:?}, found: {:?}",
            info.type_member_types
        );
    }
}

#[test]
fn a_module_without_the_workers_import_reads_no_env() {
    let accesses = typed_property_accesses(
        "/project/src/index.ts",
        r"
            export const run = () => env.ROOM.getByName('a').notEnv();
        ",
    );
    assert!(accesses.is_empty(), "found: {accesses:?}");
}

/// The `Env` type argument of `ExportedHandler` types the untyped `env`
/// parameter of each Worker handler in the object.
#[test]
fn untyped_env_parameter_of_an_exported_handler_reads_the_binding() {
    for (source, member) in [
        (
            r"
                export default {
                    async fetch(request, env, ctx) {
                        const stub = env.ROOM.get(env.ROOM.idFromName('a'));
                        return new Response(await stub.viaSatisfies());
                    },
                } satisfies ExportedHandler<Env>;
            ",
            "viaSatisfies",
        ),
        (
            r"
                export default {
                    scheduled: async (controller, env) => {
                        await env.ROOM.getByName('a').viaAs();
                    },
                } as ExportedHandler<Env>;
            ",
            "viaAs",
        ),
        (
            r"
                const handler: ExportedHandler<Env> = {
                    queue: async function (batch, env) {
                        await env.ROOM.getByName('a').viaAnnotation();
                    },
                };
                export default handler;
            ",
            "viaAnnotation",
        ),
    ] {
        let accesses = typed_property_accesses("/project/src/index.ts", source);
        assert!(
            accesses.contains(&access("Env", "ROOM", member)),
            "{member} found: {accesses:?}"
        );
    }
}

/// Only a handler of an `ExportedHandler<E>` object gets the type. A plain
/// object, a type without the `Env` argument, a method that is not a
/// handler, and a typed parameter keep their own types.
#[test]
fn env_parameter_outside_an_exported_handler_reads_no_env() {
    for source in [
        r"
            export default {
                async fetch(request, env) {
                    await env.ROOM.getByName('a').plainObject();
                },
            };
        ",
        r"
            export default {
                async fetch(request, env) {
                    await env.ROOM.getByName('a').noTypeArgument();
                },
            } satisfies ExportedHandler;
        ",
        r"
            export default {
                async helper(request, env) {
                    await env.ROOM.getByName('a').notAHandler();
                },
            } satisfies ExportedHandler<Env>;
        ",
    ] {
        let accesses = typed_property_accesses("/project/src/index.ts", source);
        assert!(accesses.is_empty(), "found: {accesses:?}");
    }
}
