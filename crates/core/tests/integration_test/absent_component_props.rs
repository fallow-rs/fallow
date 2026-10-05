#![expect(
    clippy::literal_string_with_formatting_args,
    reason = "JavaScript fixture braces are source syntax"
)]
use super::common::fixture_path;
use fallow_config::{FallowConfig, OutputFormat};

#[test]
fn angular_non_hyphenated_element_selector_supplies_optional_input() {
    let deps = serde_json::json!({"@angular/core":"*"});
    let card = "import {Component,Input} from '@angular/core';@Component({selector:'app-card,card',template:'{{flag}}'})export class Card{@Input()flag?:boolean;}";
    let main = "import {Component} from '@angular/core';import {Card} from './Card';@Component({selector:'app-root',imports:[Card],template:'<app-card/>'})export class App{}";
    let absent = analyze_project(
        deps.clone(),
        "src/main.ts",
        &[("src/main.ts", main), ("src/Card.ts", card)],
    );
    assert_eq!(absent.absent_component_props.len(), 1);
    assert_eq!(absent.absent_component_props[0].prop.prop_name, "flag");
    let supplied = main.replace("<app-card/>", "<app-card/><card [flag]=\"false\"/>");
    let results = analyze_project(
        deps,
        "src/main.ts",
        &[("src/main.ts", &supplied), ("src/Card.ts", card)],
    );
    assert!(
        results.absent_component_props.is_empty(),
        "all declared Angular element selectors must retain supplied inputs"
    );
}

#[test]
fn reports_used_optional_prop_missing_from_known_callers() {
    let root = fixture_path("absent-component-prop");
    let config: FallowConfig = serde_json::from_value(serde_json::json!({
        "rules": { "absent-component-props": "warn" }
    }))
    .expect("valid rule configuration");
    let config = config.resolve(root, OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis succeeds");
    let output = serde_json::to_value(results).expect("results serialize");
    let finding = output
        .get("absent_component_props")
        .and_then(|items| items.as_array())
        .and_then(|items| items.iter().find(|item| item["prop_name"] == "highlight"));
    assert!(
        finding.is_some(),
        "used optional highlight needs a caller-review finding: {output}"
    );
    let finding = finding.unwrap();
    assert_eq!(finding["component_name"], "Card");
    assert_eq!(finding["has_default"], true);
    assert_eq!(finding["line"], 3);
    assert_eq!(finding["inspected_call_sites"][0]["line"], 2);
}

fn analyze_project(
    deps: serde_json::Value,
    entry: &str,
    files: &[(&str, &str)],
) -> fallow_core::results::AnalysisResults {
    analyze_visible_project(deps, entry, files, true)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "fixture helpers own dependency JSON"
)]
fn analyze_visible_project(
    deps: serde_json::Value,
    entry: &str,
    files: &[(&str, &str)],
    private: bool,
) -> fallow_core::results::AnalysisResults {
    let directory = tempfile::tempdir().expect("temporary application");
    let root = directory.path().to_path_buf();
    std::fs::write(root.join("package.json"), serde_json::json!({"name":"caller-fixture","private":private,"main":entry,"dependencies":deps}).to_string()).unwrap();
    for (path, source) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let config: FallowConfig = serde_json::from_value(
        serde_json::json!({"entry":[entry],"rules":{"absent-component-props":"warn"}}),
    )
    .unwrap();
    let mut config = config.resolve(root, OutputFormat::Human, 4, true, true, None);
    config.auto_imports = deps.get("nuxt").is_some();
    fallow_core::analyze(&config).expect("private application analysis")
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one table of cross-framework behavior fixtures"
)]
fn supports_framework_contracts_with_supplied_value_controls() {
    let cases = [
        (
            "react",
            "src/main.tsx",
            "src/Card.tsx",
            "import {Card} from './Card'; export const app=<Card title='x' />;",
            "export function Card({flag=false,title}:{flag?:boolean;title:string}){return <p>{flag ? title : ''}</p>}",
            serde_json::json!({"react":"*"}),
        ),
        (
            "preact",
            "src/main.tsx",
            "src/Card.tsx",
            "import {Card} from './Card'; export const app=<Card title='x' />;",
            "import {h} from 'preact'; export function Card({flag=false,title}:{flag?:boolean;title:string}){return <p>{flag ? title : ''}</p>}",
            serde_json::json!({"preact":"*"}),
        ),
        (
            "solid",
            "src/main.tsx",
            "src/Card.tsx",
            "import {Card} from './Card'; export const app=<Card title='x' />;",
            "export function Card({flag=false,title}:{flag?:boolean;title:string}){return <p>{flag ? title : ''}</p>}",
            serde_json::json!({"solid-js":"*"}),
        ),
        (
            "qwik",
            "src/main.tsx",
            "src/Card.tsx",
            "import {Card} from './Card'; export const app=<Card title='x' />;",
            "import {component$} from '@builder.io/qwik'; export const Card=component$<{flag?:boolean;title:string}>(({flag=false,title})=><p>{flag ? title : ''}</p>);",
            serde_json::json!({"@builder.io/qwik":"*"}),
        ),
        (
            "vue",
            "src/main.vue",
            "src/Card.vue",
            "<script setup lang='ts'>import Card from './Card.vue';</script><template><Card title='x' /></template>",
            "<script setup lang='ts'>const {flag=false,title}=defineProps<{flag?:boolean;title:string}>();</script><template><p>{{flag ? title : ''}}</p></template>",
            serde_json::json!({"vue":"*"}),
        ),
        (
            "svelte",
            "src/main.svelte",
            "src/Card.svelte",
            "<script lang='ts'>import Card from './Card.svelte';</script><Card title='x' />",
            "<script lang='ts'>let {flag=false,title}:{flag?:boolean;title:string}=$props();</script><p>{flag ? title : ''}</p>",
            serde_json::json!({"svelte":"*"}),
        ),
        (
            "astro",
            "src/main.astro",
            "src/Card.astro",
            "---\nimport Card from './Card.astro';\n---\n<Card title='x' />",
            "---\ninterface Props {flag?:boolean;title:string}\nconst {flag=false,title}=Astro.props;\n---\n<p>{flag ? title : ''}</p>",
            serde_json::json!({"astro":"*"}),
        ),
        (
            "angular",
            "src/main.ts",
            "src/Card.ts",
            "import {Component} from '@angular/core'; import {Card} from './Card'; @Component({selector:'app-root',imports:[Card],template:`<app-card title='x' />`}) export class App {}",
            "import {Component,Input} from '@angular/core'; @Component({selector:'app-card',template:`{{flag}}`}) export class Card { @Input() flag?:boolean; @Input({required:true}) title!:string; }",
            serde_json::json!({"@angular/core":"*"}),
        ),
        (
            "lit",
            "src/main.ts",
            "src/Card.ts",
            "import {html} from 'lit'; import './Card'; export const view=html`<x-card title='x'></x-card>`;",
            "import {LitElement,html} from 'lit'; import {customElement,property} from 'lit/decorators.js'; @customElement('x-card') export class Card extends LitElement { @property({type:Boolean}) flag?:boolean; render(){return html`<p>${this.flag}</p>`;} }",
            serde_json::json!({"lit":"*"}),
        ),
        (
            "ember",
            "src/main.gts",
            "src/Card.gts",
            "import Card from './Card'; <template><Card @title='x' /></template>",
            "import Component from '@glimmer/component'; interface Signature {Args:{flag?:boolean;title:string}} export default class Card extends Component<Signature> { <template><p>{{@flag}}</p></template> }",
            serde_json::json!({"@glimmer/component":"*"}),
        ),
    ];
    for (framework, entry, component, caller, source, deps) in cases {
        let result = analyze_project(deps.clone(), entry, &[(entry, caller), (component, source)]);
        let flagged: Vec<_> = result
            .absent_component_props
            .iter()
            .map(|finding| {
                (
                    finding.prop.framework.as_str(),
                    finding.prop.prop_name.as_str(),
                )
            })
            .collect();
        assert!(
            flagged.contains(&(framework, "flag")),
            "{framework} used optional input must be inspected: {flagged:?}"
        );
        let supplied = if framework == "ember" {
            caller.replace("@title='x'", "@title='x' @flag={{false}}")
        } else {
            caller.replace("title='x'", "title='x' flag")
        };
        let result = analyze_project(
            deps.clone(),
            entry,
            &[(entry, &supplied), (component, source)],
        );
        assert!(
            !result
                .absent_component_props
                .iter()
                .any(|finding| finding.prop.prop_name == "flag"),
            "{framework} explicit input supplies flag"
        );
        let uncertain = match framework {
            "react" | "preact" | "solid" | "qwik" => {
                caller.replace("title='x'", "title='x' {...opaque}")
            }
            "vue" => caller.replace("title='x'", "title='x' v-bind='opaque'"),
            "svelte" | "astro" => caller.replace("title='x'", "title='x' {...opaque}"),
            "angular" => caller.replace(
                "<app-card title='x' />",
                "<app-card title='x' /><ng-container [ngComponentOutlet]='Card'/>",
            ),
            "lit" => format!(
                "import {{unsafeHTML}} from 'lit/directives/unsafe-html.js';{}",
                caller.replace("</x-card>", "</x-card>${unsafeHTML('<x-card flag/>')}")
            ),
            "ember" => caller.replace("</template>", "{{component Card flag=true}}</template>"),
            _ => unreachable!(),
        };
        let result = analyze_project(deps, entry, &[(entry, &uncertain), (component, source)]);
        assert!(
            result.absent_component_props.is_empty(),
            "{framework} incomplete caller abstains: {:?}",
            result.absent_component_props
        );
    }
}

const OPTIONAL_CARD: &str =
    "export function Card({flag=false}:{flag?:boolean}) {return <p>{flag ? 'yes' : 'no'}</p>}";

fn assert_flagged(results: &fallow_core::results::AnalysisResults) {
    assert!(
        results.absent_component_props.iter().any(
            |finding| finding.prop.component_name == "Card" && finding.prop.prop_name == "flag"
        ),
        "Card.flag positive control: {:?}",
        results.absent_component_props
    );
}

#[test]
fn canonical_alias_default_namespace_and_shadowed_bindings() {
    for caller in [
        "import {Public as Alias} from './barrel'; export const app=<Alias/>;",
        "import Alias from './default'; export const app=<Alias/>;",
        "import * as Parts from './barrel'; export const app=<Parts.Public/>;",
        "import {Card} from './Card'; const Alias=Card; export const app=<Alias/>;",
        "import {Card} from './Card'; export const app=<Card/>; export function Shadow(Card:()=>unknown){return <Card flag/>}",
    ] {
        let result = analyze_project(
            serde_json::json!({"react":"*"}),
            "src/main.tsx",
            &[
                ("src/main.tsx", caller),
                ("src/Card.tsx", OPTIONAL_CARD),
                ("src/barrel.ts", "export {Card as Public} from './Card';"),
                (
                    "src/default.ts",
                    "import {Card} from './Card'; export default Card;",
                ),
            ],
        );
        assert_flagged(&result);
        assert!(
            result
                .absent_component_props
                .iter()
                .flat_map(|finding| &finding.actions)
                .all(|action| !action.is_auto_fixable()),
            "candidate actions remain manual"
        );
    }
}

#[test]
fn supplied_names_and_opaque_consumers_fail_closed() {
    let deps = serde_json::json!({"react":"*"});
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.tsx",
        &[
            (
                "src/main.tsx",
                "import {Card} from './Card';export const app=<Card/>;",
            ),
            ("src/Card.tsx", OPTIONAL_CARD),
        ],
    ));
    for caller in [
        "import {Card} from './Card';export const app=<><Card/><Card flag={false}/></>;",
        "import {Card} from './Card';export const app=<><Card/><Card flag={undefined}/></>;",
        "import {Card} from './Card';const supplied={...{flag:false}};export const app=<Card {...supplied}/>;",
        "import {Card} from './Card';const supplied={};Object.assign(supplied,{flag:true});export const app=<Card {...supplied}/>;",
        "import {Card} from './Card';export const app=<Card/>;export const registry={Card};",
        "import {Card} from './Card';export const app=<Card/>;export const lazy=()=>import('./Card');",
        "import * as Parts from './Card';export const app=<Parts.Card/>;export const registry=Parts;",
    ] {
        let result = analyze_project(
            deps.clone(),
            "src/main.tsx",
            &[("src/main.tsx", caller), ("src/Card.tsx", OPTIONAL_CARD)],
        );
        assert!(
            result.absent_component_props.is_empty(),
            "supply or uncertain consumer blocks absence: {caller} -> {:?}",
            result.absent_component_props
        );
    }
}

#[test]
fn ambiguous_barrel_consumer_does_not_disappear_beside_direct_caller() {
    let deps = serde_json::json!({"react":"*"});
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.tsx",
        &[
            (
                "src/main.tsx",
                "import {Card} from './Card';export const app=<Card/>;",
            ),
            ("src/Card.tsx", OPTIONAL_CARD),
        ],
    ));
    let result = analyze_project(
        deps,
        "src/main.tsx",
        &[
            (
                "src/main.tsx",
                "import {Card} from './Card';import {Card as Ambiguous} from './barrel';export const app=<><Card/><Ambiguous flag/></>;",
            ),
            ("src/Card.tsx", OPTIONAL_CARD),
            ("src/Other.tsx", OPTIONAL_CARD),
            (
                "src/barrel.ts",
                "export * from './Card';export * from './Other';",
            ),
        ],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "ambiguous supplied caller leaves consumer set open: {:?}",
        result.absent_component_props
    );
}

#[test]
fn mixed_jsx_runtimes_require_declaration_framework_provenance() {
    let caller = "import {Card} from './Card';export const app=<Card/>;";
    assert_flagged(&analyze_project(
        serde_json::json!({"react":"*"}),
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", OPTIONAL_CARD)],
    ));
    let result = analyze_project(
        serde_json::json!({"react":"*","solid-js":"*"}),
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", OPTIONAL_CARD)],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "bare JSX in mixed runtimes must abstain: {:?}",
        result.absent_component_props
    );
    let proven = format!("import type {{ReactNode}} from 'react';{OPTIONAL_CARD}");
    assert_flagged(&analyze_project(
        serde_json::json!({"react":"*","solid-js":"*"}),
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", &proven)],
    ));
}

#[test]
fn angular_external_template_read_belongs_to_its_component() {
    let deps = serde_json::json!({"@angular/core":"*"});
    let app = "import {Component} from '@angular/core';import {Card} from './Card';@Component({selector:'app-root',imports:[Card],template:`<app-card/>`})export class App{}";
    let card = "import {Component,Input} from '@angular/core';@Component({selector:'app-card',templateUrl:'./card.html'})export class Card{@Input()flag?:boolean;}";
    let result = analyze_project(
        deps.clone(),
        "src/main.ts",
        &[
            ("src/main.ts", app),
            ("src/Card.ts", card),
            ("src/card.html", "<p>{{flag}}</p>"),
        ],
    );
    assert_flagged(&result);
    let wrong_owner = "import {Component,Input} from '@angular/core';@Component({selector:'app-card',template:`<p>unused</p>`})export class Card{@Input()flag?:boolean;}@Component({selector:'app-other',templateUrl:'./other.html'})export class Other{}";
    let result = analyze_project(
        deps,
        "src/main.ts",
        &[
            ("src/main.ts", app),
            ("src/Card.ts", wrong_owner),
            ("src/other.html", "<p>{{flag}}</p>"),
        ],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "another class template cannot establish Card.flag read: {:?}",
        result.absent_component_props
    );
}

#[test]
fn nuxt_synthetic_auto_imports_preserve_identity_and_collision_uncertainty() {
    let deps = serde_json::json!({"nuxt":"*","vue":"*"});
    let component = "<script setup lang='ts'>const {flag=false}=defineProps<{flag?:boolean}>();</script><template><p>{{flag}}</p></template>";
    let app = "<template><Card/></template>";
    let result = analyze_project(
        deps.clone(),
        "app.vue",
        &[("app.vue", app), ("components/Card.vue", component)],
    );
    assert_flagged(&result);
    let app = "<script setup lang='ts'>import Card from './components/foo/Card.vue';</script><template><Card/><FooCard flag/></template>";
    let result = analyze_project(
        deps,
        "app.vue",
        &[
            ("app.vue", app),
            ("components/foo/Card.vue", component),
            ("components/FooCard.vue", component),
        ],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "ambiguous synthetic consumer cannot disappear beside a direct known caller: {:?}",
        result.absent_component_props
    );
}

#[test]
fn duplicate_selector_without_contract_leaves_consumer_identity_open() {
    let deps = serde_json::json!({"@angular/core":"*"});
    let app = "import {Component} from '@angular/core';import {Card,Other} from './Card';@Component({selector:'app-root',imports:[Card,Other],template:`<app-card/>`})export class App{}";
    let card = "import {Component,Input} from '@angular/core';@Component({selector:'app-card',template:`{{flag}}`})export class Card{@Input()flag?:boolean;}";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.ts",
        &[("src/main.ts", app), ("src/Card.ts", card)],
    ));
    let duplicate =
        format!("{card}@Component({{selector:'app-card',template:''}})export class Other{{}}");
    let result = analyze_project(
        deps,
        "src/main.ts",
        &[("src/main.ts", app), ("src/Card.ts", &duplicate)],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "selector ambiguity includes components without optional inputs"
    );
}

#[test]
fn glimmer_dynamic_component_caller_abstains_beside_static_caller() {
    let deps = serde_json::json!({"@glimmer/component":"*"});
    let card = "import Component from '@glimmer/component';interface Signature{Args:{flag?:boolean}}export default class Card extends Component<Signature>{<template>{{@flag}}</template>}";
    let positive = "import Card from './Card';<template><Card/></template>";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.gts",
        &[("src/main.gts", positive), ("src/Card.gts", card)],
    ));
    let dynamic =
        "import Card from './Card';<template><Card/>{{component Card flag=true}}</template>";
    let result = analyze_project(
        deps,
        "src/main.gts",
        &[("src/main.gts", dynamic), ("src/Card.gts", card)],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "unmodeled component helper keeps Glimmer consumers open"
    );
}

#[test]
fn suppression_default_off_and_legacy_rule_remain_distinct() {
    let root = fixture_path("absent-component-prop");
    let default = FallowConfig::default().resolve(root, OutputFormat::Human, 4, true, true, None);
    assert!(
        fallow_core::analyze(&default)
            .unwrap()
            .absent_component_props
            .is_empty(),
        "advisory category defaults off"
    );
    let deps = serde_json::json!({"react":"*"});
    let caller = "import {Card} from './Card';export const app=<Card/>;";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", OPTIONAL_CARD)],
    ));
    let suppressed = "export function Card({flag=false}: {\n// fallow-ignore-next-line absent-component-prop\nflag?:boolean\n}){return <p>{flag ? 'yes' : 'no'}</p>}";
    let result = analyze_project(
        deps.clone(),
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", suppressed)],
    );
    assert!(result.absent_component_props.is_empty());
    assert!(
        result.stale_suppressions.is_empty(),
        "active declaration suppression is consumed"
    );
    let unused = "export function Card({flag=false}:{flag?:boolean}){return <p/>}";
    let local_unused = format!(
        "{};export const app=<Card/>;",
        unused.replace("export function", "function")
    );
    let result = analyze_project(deps, "src/main.tsx", &[("src/main.tsx", &local_unused)]);
    assert!(
        result.absent_component_props.is_empty(),
        "unread input belongs to old rule"
    );
    assert!(
        result
            .unused_component_props
            .iter()
            .any(|finding| finding.prop.prop_name == "flag"),
        "old unused-input finding is preserved"
    );
}

#[test]
fn lit_attributes_follow_html_case_and_property_binding_semantics() {
    let deps = serde_json::json!({"lit":"*"});
    let source = "import {LitElement,html} from 'lit';import {customElement,property} from 'lit/decorators.js';@customElement('x-card')export class Card extends LitElement{@property({type:Boolean})flag?:boolean;render(){return html`<p>${this.flag}</p>`;}}";
    let caller =
        "import {html} from 'lit';import './Card';export const view=html`<x-card></x-card>`;";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.ts",
        &[("src/main.ts", caller), ("src/Card.ts", source)],
    ));
    for attribute in ["FLAG", ".flag=${false}"] {
        let supplied = caller.replace("<x-card>", &format!("<x-card {attribute}>"));
        let result = analyze_project(
            deps.clone(),
            "src/main.ts",
            &[("src/main.ts", &supplied), ("src/Card.ts", source)],
        );
        assert!(
            result.absent_component_props.is_empty(),
            "HTML attribute/property supplies flag: {attribute}"
        );
    }
    let uppercase = caller.replace("</x-card>", "</x-card><X-CARD FLAG></X-CARD>");
    assert!(
        analyze_project(
            deps.clone(),
            "src/main.ts",
            &[("src/main.ts", &uppercase), ("src/Card.ts", source)]
        )
        .absent_component_props
        .is_empty(),
        "uppercase HTML tag supply remains visible"
    );
    for options in ["attribute:'data-flag'", "attribute:false"] {
        let source = source.replace("type:Boolean", options);
        let ordinary = caller.replace("<x-card>", "<x-card flag>");
        assert_flagged(&analyze_project(
            deps.clone(),
            "src/main.ts",
            &[("src/main.ts", &ordinary), ("src/Card.ts", &source)],
        ));
        let property = caller.replace("<x-card>", "<x-card .flag=${false}>");
        let result = analyze_project(
            deps.clone(),
            "src/main.ts",
            &[("src/main.ts", &property), ("src/Card.ts", &source)],
        );
        assert!(
            result.absent_component_props.is_empty(),
            "direct property binding supplies flag with {options}"
        );
    }
}

#[test]
fn public_package_namespace_and_framework_default_entry_are_excluded() {
    let deps = serde_json::json!({"react":"*"});
    let main = "import {Card} from './Card';export const app=<Card/>;export * as Components from './Card';";
    let private = analyze_visible_project(
        deps.clone(),
        "src/main.tsx",
        &[("src/main.tsx", main), ("src/Card.tsx", OPTIONAL_CARD)],
        true,
    );
    assert_flagged(&private);
    let public = analyze_visible_project(
        deps,
        "src/main.tsx",
        &[("src/main.tsx", main), ("src/Card.tsx", OPTIONAL_CARD)],
        false,
    );
    assert!(
        public.absent_component_props.is_empty(),
        "public namespace component API remains open"
    );
    let entry = "<script setup lang='ts'>import Card from './Card.vue';const {flag=false}=defineProps<{flag?:boolean}>();</script><template><p>{{flag}}</p><Card/></template>";
    let card = "<script setup lang='ts'>const {flag=false}=defineProps<{flag?:boolean}>();</script><template><p>{{flag}}</p></template>";
    let result = analyze_project(
        serde_json::json!({"vue":"*"}),
        "src/main.vue",
        &[("src/main.vue", entry), ("src/Card.vue", card)],
    );
    assert_flagged(&result);
    assert!(
        !result
            .absent_component_props
            .iter()
            .any(|finding| finding.prop.component_name == "main"),
        "framework entry receives injected inputs"
    );
}

#[test]
fn required_inputs_and_children_keep_distinct_semantics() {
    let deps = serde_json::json!({"react":"*"});
    let card = "export function Card({flag,title,children}:{flag?:boolean;title:string;children?:unknown}) {return <p>{flag ? title : children}</p>}";
    let caller = "import {Card} from './Card';export const app=<Card>text</Card>;";
    let result = analyze_project(
        deps,
        "src/main.tsx",
        &[("src/main.tsx", caller), ("src/Card.tsx", card)],
    );
    assert_flagged(&result);
    assert!(
        !result
            .absent_component_props
            .iter()
            .any(|finding| matches!(finding.prop.prop_name.as_str(), "children" | "title")),
        "children supplied and required title excluded"
    );
}

#[test]
fn mixed_markup_custom_element_consumers_are_retained_or_uncertain() {
    let card = "import {LitElement,html} from 'lit';import {customElement,property} from 'lit/decorators.js';@customElement('x-card')export class Card extends LitElement{@property({type:Boolean})flag?:boolean;render(){return html`<p>${this.flag}</p>`;}}";
    let caller =
        "import {html} from 'lit';import './Card';export const view=html`<x-card></x-card>`;";
    assert_flagged(&analyze_project(
        serde_json::json!({"lit":"*"}),
        "src/main.ts",
        &[("src/main.ts", caller), ("src/Card.ts", card)],
    ));
    for (file, source, deps) in [
        (
            "src/view.astro",
            "<x-card flag/>",
            serde_json::json!({"lit":"*","astro":"*"}),
        ),
        (
            "src/view.vue",
            "<template><x-card flag/></template>",
            serde_json::json!({"lit":"*","vue":"*"}),
        ),
        (
            "src/view.svelte",
            "<x-card flag/>",
            serde_json::json!({"lit":"*","svelte":"*"}),
        ),
        (
            "src/view.tsx",
            "export const view=<x-card flag/>;",
            serde_json::json!({"lit":"*","react":"*"}),
        ),
    ] {
        let main = format!("import './{}';{caller}", file.strip_prefix("src/").unwrap());
        let result = analyze_project(
            deps,
            "src/main.ts",
            &[
                ("src/main.ts", &main),
                ("src/Card.ts", card),
                (file, source),
            ],
        );
        assert!(
            result.absent_component_props.is_empty(),
            "mixed {file} custom-element caller must not disappear: {:?}",
            result.absent_component_props
        );
    }
}

#[test]
fn cached_optional_input_evidence_matches_fresh_analysis() {
    let root = fixture_path("absent-component-prop");
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().to_path_buf();
    std::fs::create_dir_all(target.join("src")).unwrap();
    for path in ["package.json", "src/main.tsx", "src/Card.tsx"] {
        std::fs::copy(root.join(path), target.join(path)).unwrap();
    }
    let config: FallowConfig =
        serde_json::from_value(serde_json::json!({"rules":{"absent-component-props":"warn"}}))
            .unwrap();
    let config = config.resolve(target, OutputFormat::Human, 4, false, true, None);
    let cold = fallow_core::analyze(&config).unwrap();
    let warm = fallow_core::analyze(&config).unwrap();
    assert!(
        cold.absent_component_props
            .iter()
            .any(|finding| finding.prop.prop_name == "highlight"),
        "cold run is a substantive positive"
    );
    assert_eq!(
        serde_json::to_value(cold.absent_component_props).unwrap(),
        serde_json::to_value(warm.absent_component_props).unwrap(),
        "cached facts preserve caller/default/declaration evidence"
    );
}

#[test]
fn component_values_forwarded_through_template_arguments_are_opaque_consumers() {
    let cases = [
        (
            "src/main.vue",
            "src/Card.vue",
            "src/Child.vue",
            "<script setup>import Card from './Card.vue';import Child from './Child.vue';</script><template><Card/><Child :renderer='Card'/></template>",
            "<script setup lang='ts'>const {flag=false}=defineProps<{flag?:boolean}>();</script><template>{{flag}}</template>",
            "<template/>",
            serde_json::json!({"vue":"*"}),
        ),
        (
            "src/main.svelte",
            "src/Card.svelte",
            "src/Child.svelte",
            "<script>import Card from './Card.svelte';import Child from './Child.svelte';</script><Card/><Child renderer={Card}/>",
            "<script lang='ts'>let {flag=false}:{flag?:boolean}=$props();</script>{flag}",
            "<p/>",
            serde_json::json!({"svelte":"*"}),
        ),
        (
            "src/main.gts",
            "src/Card.gts",
            "src/Child.gts",
            "import Card from './Card';import Child from './Child';<template><Card/><Child @renderer={{Card}}/></template>",
            "import Component from '@glimmer/component';interface Signature{Args:{flag?:boolean}}export default class Card extends Component<Signature>{<template>{{@flag}}</template>}",
            "import Component from '@glimmer/component';export default class Child extends Component{}",
            serde_json::json!({"@glimmer/component":"*"}),
        ),
    ];
    for (entry, card, child, caller, source, child_source, deps) in cases {
        let positive = caller
            .replace(":renderer='Card'", "title='text'")
            .replace("renderer={Card}", "title='text'")
            .replace("@renderer={{Card}}", "@title='text'");
        assert_flagged(&analyze_project(
            deps.clone(),
            entry,
            &[(entry, &positive), (card, source), (child, child_source)],
        ));
        let result = analyze_project(
            deps,
            entry,
            &[(entry, caller), (card, source), (child, child_source)],
        );
        assert!(
            result.absent_component_props.is_empty(),
            "template forwarding cannot establish closed consumers: {entry}"
        );
    }
}

#[test]
fn unknown_lit_attribute_contract_abstains_with_known_caller_control() {
    let deps = serde_json::json!({"lit":"*"});
    let source = "import {LitElement,html} from 'lit';import {customElement,property} from 'lit/decorators.js';@customElement('x-card')export class Card extends LitElement{@property({attribute:'show'})flag?:boolean;render(){return html`<p>${this.flag}</p>`;}}";
    let caller =
        "import {html} from 'lit';import './Card';export const view=html`<x-card></x-card>`;";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.ts",
        &[("src/main.ts", caller), ("src/Card.ts", source)],
    ));
    let unknown = source.replace("attribute:'show'", "attribute:computedAlias");
    let result = analyze_project(
        deps,
        "src/main.ts",
        &[("src/main.ts", caller), ("src/Card.ts", &unknown)],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "unknown runtime attribute alias cannot imply absence"
    );
}

#[test]
fn dynamic_template_alias_preserves_cross_framework_import_uncertainty() {
    let deps = serde_json::json!({"react":"*","vue":"*"});
    let main = "import {Card} from './Card';import './view.vue';export const app=<Card/>;";
    let view = "<script setup>import {Card} from './Card';const chosen=Card;</script><template><component :is='chosen' flag/></template>";
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.tsx",
        &[
            ("src/main.tsx", main),
            ("src/Card.tsx", OPTIONAL_CARD),
            ("src/view.vue", "<template/>"),
        ],
    ));
    let result = analyze_project(
        deps,
        "src/main.tsx",
        &[
            ("src/main.tsx", main),
            ("src/Card.tsx", OPTIONAL_CARD),
            ("src/view.vue", view),
        ],
    );
    assert!(
        result.absent_component_props.is_empty(),
        "dynamic alias must not hide a possible cross-framework caller"
    );
}

#[test]
fn angular_bound_component_values_are_opaque_consumers() {
    let deps = serde_json::json!({"@angular/core":"*"});
    let card = "import {Component,Input} from '@angular/core';@Component({selector:'x-card',template:'{{flag}}'})export class Card{@Input()flag?:boolean;}";
    let main = "import {Component} from '@angular/core';import {Card} from './Card';@Component({selector:'x-app',imports:[Card],template:'<x-card/><child [renderer]=\"Card\"/>'})export class App{}";
    let positive = main.replace("[renderer]=\"Card\"", "title=\"text\"");
    assert_flagged(&analyze_project(
        deps.clone(),
        "src/main.ts",
        &[("src/main.ts", &positive), ("src/Card.ts", card)],
    ));
    assert!(
        analyze_project(
            deps,
            "src/main.ts",
            &[("src/main.ts", main), ("src/Card.ts", card)]
        )
        .absent_component_props
        .is_empty(),
        "a bound component value can receive props outside static callers"
    );
}

#[test]
fn immutable_template_aliases_forwarded_to_external_consumers_are_opaque() {
    let cases = [
        (
            "src/main.vue",
            "src/Card.vue",
            "<script setup>import Card from './Card.vue';import Child from 'external-renderer';const chosen=Card;</script><template><Card/><Child :renderer='chosen'/></template>",
            "<script setup lang='ts'>const {flag=false}=defineProps<{flag?:boolean}>();</script><template>{{flag}}</template>",
            serde_json::json!({"vue":"*"}),
        ),
        (
            "src/main.svelte",
            "src/Card.svelte",
            "<script>import Card from './Card.svelte';import Child from 'external-renderer';const chosen=Card;</script><Card/><Child renderer={chosen}/>",
            "<script lang='ts'>let {flag=false}:{flag?:boolean}=$props();</script>{flag}",
            serde_json::json!({"svelte":"*"}),
        ),
        (
            "src/main.gts",
            "src/Card.gts",
            "import Card from './Card';import Child from 'external-renderer';const chosen=Card;<template><Card/><Child @renderer={{chosen}}/></template>",
            "import Component from '@glimmer/component';interface Signature{Args:{flag?:boolean}}export default class Card extends Component<Signature>{<template>{{@flag}}</template>}",
            serde_json::json!({"@glimmer/component":"*"}),
        ),
    ];
    for (entry, path, source, card, deps) in cases {
        let positive = source
            .replace(":renderer='chosen'", "title='text'")
            .replace("renderer={chosen}", "title='text'")
            .replace("@renderer={{chosen}}", "@title='text'");
        assert_flagged(&analyze_project(
            deps.clone(),
            entry,
            &[(entry, &positive), (path, card)],
        ));
        assert!(
            analyze_project(deps, entry, &[(entry, source), (path, card)])
                .absent_component_props
                .is_empty(),
            "opaque template alias consumer: {entry}"
        );
    }
}
