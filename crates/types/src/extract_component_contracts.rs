//! Persisted syntactic evidence for conservative component-input analysis.

/// Framework owning a statically extracted component contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ComponentFramework {
    /// React JSX.
    React,
    /// Preact JSX.
    Preact,
    /// Solid JSX.
    Solid,
    /// Qwik JSX.
    Qwik,
    /// Vue and Nuxt single-file components.
    Vue,
    /// Svelte components.
    Svelte,
    /// Astro components.
    Astro,
    /// Angular components and directives.
    Angular,
    /// Lit custom elements.
    Lit,
    /// Ember and Glimmer components.
    Ember,
}

/// Stable component binding identity, independent of transient parser symbols.
#[derive(Debug, Clone, PartialEq, Eq, Hash, bitcode::Encode, bitcode::Decode)]
pub enum ComponentReference {
    /// A same-module declaration, including resolved immutable local aliases.
    Local {
        /// Declaration binding name.
        name: String,
        /// Start byte of the declaration binding.
        span_start: u32,
    },
    /// An import binding. Analysis resolves its effective export via the graph.
    Import {
        /// Local name of the actual import binding, never a shadowing identifier.
        local: String,
        /// Start byte of the import binding declaration.
        span_start: u32,
    },
    /// A direct member of a proven namespace import.
    NamespaceMember {
        /// Local namespace import binding name.
        local: String,
        /// Namespace import binding byte anchor.
        span_start: u32,
        /// Effective value export requested by the member access.
        member: String,
    },
    /// A statically registered Angular selector or custom-element tag.
    Selector(String),
    /// Dynamic or unsupported component identity.
    Unknown,
}

/// A declared public input, kept distinct from legacy unused-prop facts.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentPropDeclaration {
    /// Component declaration name. Empty denotes the sole SFC default export.
    pub component: String,
    /// Stable declaration binding byte anchor.
    pub component_span: u32,
    /// Public input name.
    pub name: String,
    /// Local binding name used by destructured template reads.
    pub local: String,
    /// Additional public attribute names supplying the same input.
    pub aliases: Vec<String>,
    /// Declaration byte anchor in the original source.
    pub span_start: u32,
    /// Whether the declaration explicitly permits omission.
    pub optional: bool,
    /// Whether omission has a syntactically declared default value.
    pub has_default: bool,
    /// Whether the input is consumed within the component.
    pub is_used: bool,
    /// Producer framework.
    pub framework: ComponentFramework,
    /// Unsupported declarations, forwarding, or open consumers prohibit absence claims.
    pub incomplete: bool,
}

/// One inspected render invocation and its supplied input names.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentInvocation {
    /// Binding or selector used by the invocation.
    pub target: ComponentReference,
    /// Original source byte anchor of the opening tag or call.
    pub span_start: u32,
    /// Supplied names, independent of values and including proven spread keys.
    pub supplied: Vec<String>,
    /// Case-sensitive Lit JavaScript property bindings, distinct from HTML attributes.
    pub supplied_properties: Vec<String>,
    /// A spread or dynamic attribute may supply additional names.
    pub unknown_props: bool,
    /// Framework syntax of this caller.
    pub framework: ComponentFramework,
}

/// Persisted syntactic evidence for conservative component-input analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentContractFacts {
    /// Immutable root script aliases resolved to their semantic targets.
    pub aliases: Vec<ComponentAliasBinding>,
    /// Exact Angular templateUrl source owned by each component binding.
    pub external_templates: Vec<ComponentExternalTemplate>,
    /// Original component bodies that own embedded Glimmer templates.
    pub template_owners: Vec<ComponentTemplateOwner>,
    /// Effective local export bindings retained after graph payload release.
    pub exports: Vec<ComponentExportBinding>,
    /// Immutable local object shapes retained for template spread resolution.
    pub spread_bindings: Vec<ComponentSpreadBinding>,
    /// Known declaration contracts.
    pub declarations: Vec<ComponentPropDeclaration>,
    /// Inspected render sites, including module-level invocations.
    pub invocations: Vec<ComponentInvocation>,
    /// Component bindings used opaquely outside inspected renders.
    pub escapes: Vec<ComponentReference>,
    /// Frameworks with a dynamic consumer whose affected target is unknowable.
    pub incomplete_frameworks: Vec<ComponentFramework>,
}

/// An immutable, unescaped local object with statically known own keys.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentSpreadBinding {
    /// Root script binding name, resolved before template scopes are applied.
    pub local: String,
    /// Script binding declaration anchor.
    pub span_start: u32,
    /// Complete own property names.
    pub keys: Vec<String>,
}

/// An export name resolved to its syntactic component or import binding.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentExportBinding {
    /// Public export name, including `default`.
    pub export_name: String,
    /// Original export declaration byte anchor.
    pub export_span: u32,
    /// Proven local binding, canonicalizing immutable aliases.
    pub target: ComponentReference,
}

/// A semantic component declaration body containing an embedded template.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentTemplateOwner {
    /// Owning component binding anchor.
    pub component_span: u32,
    /// Original class body start.
    pub start: u32,
    /// Original class body end.
    pub end: u32,
}

/// A statically declared external template owned by one component.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentExternalTemplate {
    /// Owning component binding anchor.
    pub component_span: u32,
    /// Literal templateUrl source, resolved through the graph import edge.
    pub source: String,
}

/// An immutable root binding that aliases a component or import.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ComponentAliasBinding {
    /// Root script binding spelling used by template expressions.
    pub local: String,
    /// Canonical semantic binding target, including its source anchor.
    pub target: ComponentReference,
}
