//! Original-source component contract regressions across framework producers.
use crate::tests::parse_at_path;
use fallow_types::extract::{ComponentFramework, ComponentReference};

#[test]
fn component_contract_glimmer_whole_args_forwarding_abstains() {
    for (body, incomplete) in [
        ("return this.args.flag;", false),
        ("consume(this.args);return this.args.flag;", true),
    ] {
        let source = format!(
            "import Component from '@glimmer/component'; interface Signature {{Args:{{flag?:boolean}}}}; export default class Card extends Component<Signature>{{get flag(){{{body}}}}}"
        );
        let info = parse_at_path("Card.gts", &source);
        let prop = info
            .component_contracts
            .as_deref()
            .and_then(|facts| facts.declarations.iter().find(|prop| prop.name == "flag"))
            .expect("declared prop");
        assert!(prop.is_used);
        assert_eq!(prop.incomplete, incomplete);
    }
}

#[test]
fn component_contract_vue_and_glimmer_write_only_is_not_consumed() {
    for (path, source) in [
        (
            "Card.vue",
            "<script>export default {props: {flag: {default: false}},methods:{change(){this.flag=false;}}};</script><template><p/></template>",
        ),
        (
            "Card.gts",
            "import Component from '@glimmer/component'; interface Signature {Args:{flag?:boolean}}; export default class Card extends Component<Signature>{change(){this.args.flag=false;}}",
        ),
    ] {
        let info = parse_at_path(path, source);
        let prop = info
            .component_contracts
            .as_deref()
            .and_then(|facts| facts.declarations.iter().find(|prop| prop.name == "flag"))
            .expect("declared prop");
        assert!(!prop.is_used, "write-only prop at {path}");
    }
}

#[test]
fn component_contract_destructured_framework_macros() {
    for (path, source, framework) in [
        (
            "Card.svelte",
            "<script lang=\"ts\">let {flag=false}: {flag?:boolean}=$props();</script>{flag}",
            ComponentFramework::Svelte,
        ),
        (
            "Card.vue",
            "<script setup lang=\"ts\">const {flag}=defineProps<{flag?:boolean}>();</script><template>{{flag}}</template>",
            ComponentFramework::Vue,
        ),
    ] {
        let info = parse_at_path(path, source);
        let prop = info
            .component_contracts
            .as_deref()
            .and_then(|facts| facts.declarations.iter().find(|prop| prop.name == "flag"))
            .expect("destructured macro prop");
        assert!(prop.optional && prop.is_used && !prop.incomplete);
        assert_eq!(prop.framework, framework);
        assert_eq!(&source[prop.span_start as usize..][..4], "flag");
    }
}

#[test]
fn component_contract_lit_injected_markup_is_incomplete() {
    for source in [
        "import {html} from 'lit'; import {unsafeHTML as inject} from 'lit/directives/unsafe-html.js'; const view=html`<app-card/>${inject('<app-card flag/>')}`;",
        "import {html} from 'lit'; import {unsafeSVG} from 'lit/directives/unsafe-svg.js'; const view=html`<app-card/>${unsafeSVG(markup)}`;",
        "import {html, unsafeStatic} from 'lit/static-html.js'; const tag=unsafeStatic('app-card'); const view=html`<${tag} flag/>`;",
        "import * as lit from 'lit'; import * as unsafe from 'lit/directives/unsafe-html.js'; const view=lit.html`<app-card/>${unsafe.unsafeHTML(markup)}`;",
    ] {
        let info = parse_at_path("view.ts", source);
        let facts = info.component_contracts.as_deref().expect("contracts");
        assert!(
            facts
                .incomplete_frameworks
                .contains(&ComponentFramework::Lit)
        );
    }
    let info = parse_at_path(
        "view.ts",
        "import {html} from 'lit'; const view=html`<app-card .title=${title}/>`;",
    );
    let facts = info.component_contracts.as_deref().expect("contracts");
    assert!(
        !facts
            .incomplete_frameworks
            .contains(&ComponentFramework::Lit)
    );
    assert!(
        facts
            .invocations
            .iter()
            .any(|caller| caller.supplied_properties == ["title"])
    );
}

#[test]
fn component_contract_svelte_legacy_defaults_and_caller() {
    let source = r"<script>import Card from './Card.svelte'; export let condensed = false;</script><Card flag={undefined}/>{condensed}";
    let info = parse_at_path("View.svelte", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let prop = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("legacy prop declaration");
    assert_eq!(
        (prop.optional, prop.has_default, prop.is_used),
        (true, true, true)
    );
    assert_eq!(prop.framework, ComponentFramework::Svelte);
    assert_eq!(&source[prop.span_start as usize..][..9], "condensed");
    assert_eq!(facts.invocations[0].supplied, ["flag"]);
    assert!(
        matches!(&facts.invocations[0].target, ComponentReference::Import {local, ..} if local == "Card")
    );
}

#[test]
fn component_contract_vue_named_type_runtime_and_bindings() {
    let source = r#"<script setup lang="ts">import Card from './Card.vue'; interface Props { condensed?: boolean; title: string }; const props = defineProps<Props>();</script><template><Card :show-extra="false"/><p>{{ props.condensed }}</p></template>"#;
    let info = parse_at_path("View.vue", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let prop = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("typed Vue declaration");
    assert_eq!(
        (prop.optional, prop.has_default, prop.is_used),
        (true, false, true)
    );
    assert_eq!(facts.invocations[0].supplied, ["showExtra"]);
    assert_eq!(&source[prop.span_start as usize..][..9], "condensed");
}

#[test]
fn component_contract_astro_original_source_props() {
    let source = "---\nimport Card from './Card.astro';\ninterface Props { condensed?: boolean; title: string }\nconst { condensed = false, title } = Astro.props;\n---\n<Card title={title}/><p>{condensed}</p>";
    let info = parse_at_path("View.astro", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let prop = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("Astro optional declaration");
    assert_eq!(
        (prop.optional, prop.has_default, prop.is_used),
        (true, true, true)
    );
    assert_eq!(prop.framework, ComponentFramework::Astro);
    assert_eq!(facts.invocations[0].supplied, ["title"]);
    assert_eq!(&source[prop.span_start as usize..][..9], "condensed");
}

#[test]
fn component_contract_angular_alias_required_and_inline_caller() {
    let source = r#"import { Component, Input, input } from '@angular/core';
        @Component({selector: 'app-card', template: '<p>{{ condensed() }}</p>'})
        export class Card { @Input({alias: 'heading', required: true}) title = ''; condensed = input(false); }
        @Component({selector:'app-view', imports:[Card], template:'<app-card heading="example"/>'})
        export class View {}"#;
    let info = parse_at_path("card.ts", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let optional = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("Angular input");
    let required = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "heading")
        .expect("Angular aliased required input");
    assert_eq!(
        (optional.optional, optional.has_default, optional.is_used),
        (true, true, true)
    );
    assert!(!required.optional);
    assert_eq!(optional.component, "Card");
    assert!(facts.invocations.iter().any(|caller| caller.target
        == ComponentReference::Selector("app-card".to_string())
        && caller.supplied == ["heading"]));
}

#[test]
fn component_contract_lit_property_and_tagged_template() {
    let source = r"import { LitElement, html } from 'lit'; import { property, customElement } from 'lit/decorators.js';
        @customElement('app-card') export class Card extends LitElement { @property({attribute:'show-extra'}) condensed = false; render() { return html`<p>${this.condensed}</p>`; } }
        const view = html`<app-card .title=${undefined}></app-card>`;";
    let info = parse_at_path("card.ts", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let prop = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("Lit reactive property");
    assert_eq!(
        (prop.optional, prop.has_default, prop.is_used),
        (true, true, true)
    );
    assert_eq!(prop.framework, ComponentFramework::Lit);
    assert!(facts.invocations.iter().any(|caller| caller.target
        == ComponentReference::Selector("app-card".to_string())
        && caller.supplied_properties == ["title"]));
}

#[test]
fn component_contract_glimmer_typed_args_and_template() {
    let source = r#"import Component from '@glimmer/component'; import Card from './card';
        interface Signature { Args: { condensed?: boolean; title: string } }
        export default class View extends Component<Signature> { get expanded() { return !this.args.condensed; } <template><Card @title="example"/>{{this.args.condensed}}</template> }"#;
    let info = parse_at_path("view.gts", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let prop = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("Glimmer optional argument");
    assert_eq!((prop.optional, prop.is_used), (true, true));
    assert_eq!(prop.framework, ComponentFramework::Ember);
    assert!(
        facts
            .invocations
            .iter()
            .any(|caller| caller.supplied == ["title"])
    );
}

#[test]
fn component_contract_vue_options_runtime_default() {
    let source = r"<script>export default { props: { condensed: {type: Boolean, default: false}, title: {type: String, required: true} }, computed: { expanded() { return !this.condensed; } } };</script><template><p>{{ condensed }}</p></template>";
    let info = parse_at_path("Card.vue", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    let optional = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "condensed")
        .expect("runtime Vue prop");
    let required = facts
        .declarations
        .iter()
        .find(|prop| prop.name == "title")
        .expect("required Vue prop");
    assert_eq!(
        (optional.optional, optional.has_default, optional.is_used),
        (true, true, true)
    );
    assert!(!required.optional);
}

#[test]
fn component_contract_qwik_generic_wrapper_and_other_jsx_frameworks() {
    for (import, framework) in [
        (
            "import {component$} from '@builder.io/qwik';",
            ComponentFramework::Qwik,
        ),
        ("import {h} from 'preact';", ComponentFramework::Preact),
        (
            "import {createSignal} from 'solid-js';",
            ComponentFramework::Solid,
        ),
    ] {
        let declaration = if framework == ComponentFramework::Qwik {
            "const Card = component$<{condensed?: boolean}>((props) => <div>{props.condensed}</div>);"
        } else {
            "const Card = (props: {condensed?: boolean}) => <div>{props.condensed}</div>;"
        };
        let info = parse_at_path(
            "Card.tsx",
            &format!("{import} {declaration} const view = <Card/>;"),
        );
        let facts = info.component_contracts.as_deref().expect("contracts");
        let prop = facts
            .declarations
            .iter()
            .find(|prop| prop.name == "condensed")
            .expect("JSX optional prop");
        assert_eq!(
            (prop.optional, prop.is_used, prop.framework),
            (true, true, framework)
        );
        assert_eq!(facts.invocations[0].supplied, Vec::<String>::new());
    }
}

#[test]
fn component_contract_template_literal_spreads_and_opaque_controls() {
    for (path, source) in [
        (
            "View.vue",
            "<script setup>import Card from './Card.vue';</script><template><Card v-bind=\"{ enabled: undefined, ...{ flag: false } }\"/><Card v-bind=\"opaque\"/></template>",
        ),
        (
            "View.svelte",
            "<script>import Card from './Card.svelte';</script><Card {...{enabled: undefined, ...{ flag: false }}}/><Card {...opaque}/>",
        ),
    ] {
        let info = parse_at_path(path, source);
        let facts = info.component_contracts.as_deref().expect("contracts");
        assert_eq!(facts.invocations[0].supplied, ["enabled", "flag"]);
        assert!(!facts.invocations[0].unknown_props);
        assert!(facts.invocations[1].unknown_props);
    }
}

#[test]
fn component_contract_template_const_spread_and_escape() {
    let source = "<script setup>import Card from './Card.vue'; const attrs = {flag: false}; const escaped = {other:undefined}; send(escaped);</script><template><Card v-bind=\"attrs\"/><Card v-bind=\"escaped\"/></template>";
    let info = parse_at_path("View.vue", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    assert_eq!(facts.invocations[0].supplied, ["flag"]);
    assert!(!facts.invocations[0].unknown_props);
    assert!(facts.invocations[1].unknown_props);
}

#[test]
fn component_contract_required_vue_defaults_and_template_mutation() {
    let source = "<script setup lang=\"ts\">import Card from './Card.vue'; const props = withDefaults(defineProps<{required:string; optional?:string}>(), {required:'x',optional:'y'}); const attrs={};</script><template><p>{{props.required}}{{props.optional}}</p><button @click=\"attrs.highlight=true\"/><Card v-bind=\"attrs\"/></template>";
    let info = parse_at_path("View.vue", source);
    let facts = info.component_contracts.as_deref().expect("contracts");
    assert_eq!(
        facts
            .declarations
            .iter()
            .find(|prop| prop.name == "required")
            .map(|prop| (prop.optional, prop.has_default)),
        Some((false, true))
    );
    assert_eq!(
        facts
            .declarations
            .iter()
            .find(|prop| prop.name == "optional")
            .map(|prop| (prop.optional, prop.has_default)),
        Some((true, true))
    );
    assert!(facts.invocations[0].unknown_props);
}

#[test]
fn component_contract_svelte_children_dynamic_and_dom_uncertainty() {
    let info = parse_at_path(
        "View.svelte",
        "<script>import Card from './Card.svelte';</script><Card><span>content</span></Card><svelte:component this={Card}/>",
    );
    let facts = info.component_contracts.as_deref().expect("contracts");
    assert_eq!(facts.invocations[0].supplied, ["children"]);
    assert!(
        facts
            .incomplete_frameworks
            .contains(&ComponentFramework::Svelte)
    );
    let info = parse_at_path(
        "view.ts",
        "import { html } from 'lit'; const view=html`<app-card/>`; document.querySelector('app-card').flag=true;",
    );
    let facts = info.component_contracts.as_deref().expect("contracts");
    assert!(
        facts
            .invocations
            .iter()
            .any(|caller| caller.target == ComponentReference::Selector("app-card".into()))
    );
    assert!(
        facts
            .escapes
            .contains(&ComponentReference::Selector("app-card".into()))
    );
}

#[test]
fn template_alias_target_is_retained() {
    let source = "import Card from './Card';import Child from 'external-renderer';const chosen=Card;<template><Card/><Child @renderer={{chosen}}/></template>";
    let info = parse_at_path("main.gts", source);
    let facts = info.component_contracts.as_deref().unwrap();
    assert!(
        facts
            .escapes
            .iter()
            .any(|target| matches!(target,ComponentReference::Import{local,..} if local=="Card")),
        "{facts:?}"
    );
}
