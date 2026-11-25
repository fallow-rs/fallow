//! Feature flag uses, flag site facts, flag patterns, and the flag key
//! registry.

/// The kind of feature flag pattern detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum FlagUseKind {
    /// `process.env.FEATURE_X` pattern.
    EnvVar,
    /// SDK function call like `useFlag('name')`.
    SdkCall,
    /// Config object access like `config.features.x`.
    ConfigObject,
}

/// A feature flag use site.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct FlagUse {
    /// Flag identifier.
    pub flag_name: String,
    /// Detection kind.
    pub kind: FlagUseKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded block.
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded block.
    pub guard_span_end: Option<u32>,
    /// SDK/provider name.
    pub sdk_name: Option<String>,
    /// Facts about the site, for the retirement report and the confidence
    /// mapping.
    pub facts: FlagSiteFacts,
}

const _: () = assert!(std::mem::size_of::<FlagUse>() <= 96);

/// Facts about a flag site that the flag retirement report and the
/// confidence mapping read.
///
/// The branch facts describe the `if`, ternary or JSX `&&` that the site
/// guards. A site without a guard has no branch facts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FlagSiteFacts(u8);

impl FlagSiteFacts {
    const IDENTICAL_BRANCHES: u8 = 1;
    const EMPTY_BRANCH: u8 = 1 << 1;
    const DEFINITION: u8 = 1 << 2;
    const UNCONFIRMED_SDK: u8 = 1 << 3;

    /// Both branches of the guard are the same code, ignoring whitespace
    /// and comments.
    #[must_use]
    pub const fn identical_branches(self) -> bool {
        self.0 & Self::IDENTICAL_BRANCHES != 0
    }

    /// No branch of the guard holds code, so the flag does nothing. An
    /// empty branch is `{}`, `;`, `null`, `undefined`, `void 0`, `<></>`, or
    /// `false` next to JSX. A missing `else` is an empty branch. Code in one
    /// branch, for the on case or for the off case, clears this fact.
    #[must_use]
    pub const fn empty_branch(self) -> bool {
        self.0 & Self::EMPTY_BRANCH != 0
    }

    /// The site defines the flag, as in `export const x = flag({ key })`,
    /// and does not read it.
    #[must_use]
    pub const fn definition(self) -> bool {
        self.0 & Self::DEFINITION != 0
    }

    /// The site calls a generic SDK name, such as `isEnabled` or
    /// `getValue`, and its file imports no flag SDK or flag module. Other
    /// libraries use the same names, so the site is less certain.
    #[must_use]
    pub const fn unconfirmed_sdk(self) -> bool {
        self.0 & Self::UNCONFIRMED_SDK != 0
    }

    /// These facts with `unconfirmed_sdk` set to `value`.
    #[must_use]
    pub const fn with_unconfirmed_sdk(self, value: bool) -> Self {
        Self::set(self, Self::UNCONFIRMED_SDK, value)
    }

    /// These facts with `definition` set to `value`.
    #[must_use]
    pub const fn with_definition(self, value: bool) -> Self {
        Self::set(self, Self::DEFINITION, value)
    }

    /// These facts with `identical_branches` set to `value`.
    #[must_use]
    pub const fn with_identical_branches(self, value: bool) -> Self {
        Self::set(self, Self::IDENTICAL_BRANCHES, value)
    }

    /// These facts with `empty_branch` set to `value`.
    #[must_use]
    pub const fn with_empty_branch(self, value: bool) -> Self {
        Self::set(self, Self::EMPTY_BRANCH, value)
    }

    const fn set(self, bit: u8, value: bool) -> Self {
        if value {
            Self(self.0 | bit)
        } else {
            Self(self.0 & !bit)
        }
    }
}

/// User flag patterns from the `flags` config section that detection
/// applies during the parse. The default holds the built-in patterns only.
///
/// The parse cache keys on these patterns, so every parse that writes the
/// cache must use the patterns of the resolved config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlagPatterns {
    /// Extra SDK calls: function name, zero-based name argument, provider label.
    pub sdk_patterns: Vec<(String, usize, String)>,
    /// Extra environment variable prefixes.
    pub env_prefixes: Vec<String>,
    /// Whether an access on a config object with a flag-like name is a flag.
    pub config_object_heuristics: bool,
}

impl FlagPatterns {
    /// Whether no user pattern is present.
    #[must_use]
    pub fn is_builtin_only(&self) -> bool {
        self.sdk_patterns.is_empty()
            && self.env_prefixes.is_empty()
            && !self.config_object_heuristics
    }
}

/// A flag-key registry that a module exports: a module-level `as const`
/// object or a TypeScript enum whose members hold string flag keys.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FlagKeyRegistry {
    /// Name the module exports the registry under.
    pub export_name: String,
    /// Member name and flag key of each string member, in declaration order.
    pub members: Vec<(String, String)>,
}

/// A flag read whose key is a member of an imported registry, as in
/// `useFlag(FLAGS.X)` where `FLAGS` is imported.
///
/// Extraction sees one file only, so project analysis resolves the key
/// through the import of `registry`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct FlagRegistryRead {
    /// Local name of the imported registry binding.
    pub registry: String,
    /// Registry member that holds the flag key.
    pub member: String,
    /// The read site. `flag_name` stays empty until analysis resolves the key.
    pub flag_use: FlagUse,
}

/// A module-level `const` with a flag-style name and a literal value, such
/// as `const FEATURE_NEW_UI = true`, that a guard in the same module tests.
///
/// The flag retirement report reads these. They are not in the per-site
/// flag findings.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FlagConstant {
    /// Binding name.
    pub name: String,
    /// The literal value as source code: `true`, `0` or `'on'`.
    pub value: String,
    /// 1-based line of the binding.
    pub line: u32,
    /// 0-based byte column of the binding.
    pub col: u32,
    /// Guard tests that read the binding, in source order.
    pub reads: Vec<FlagConstantRead>,
}

/// A guard test that reads a [`FlagConstant`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FlagConstantRead {
    /// 1-based line.
    pub line: u32,
    /// 0-based byte column.
    pub col: u32,
    /// Facts about the guard.
    pub facts: FlagSiteFacts,
}

/// A flag definition bound to a `const`, as in
/// `export const showBanner = flag({ key: 'show-banner' })`.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FlagDefinition {
    /// The binding that holds the definition.
    pub binding: String,
    /// 1-based line of the definition call, as on its [`FlagUse`].
    pub line: u32,
    /// 0-based byte column of the definition call, as on its [`FlagUse`].
    pub col: u32,
}

/// Registry facts, and other flag facts outside the per-site findings, that
/// a module gives to feature flag analysis.
#[derive(Debug, Clone, Default, bitcode::Encode, bitcode::Decode)]
pub struct FlagRegistryFacts {
    /// Registries this module exports.
    pub registries: Vec<FlagKeyRegistry>,
    /// Flag reads that name a member of an imported registry.
    pub reads: Vec<FlagRegistryRead>,
    /// Literal `const` flags that a guard in the module tests.
    pub constants: Vec<FlagConstant>,
    /// Flag definitions bound to a `const`.
    pub definitions: Vec<FlagDefinition>,
}

impl FlagRegistryFacts {
    /// Whether the module contributes no fact.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.registries.is_empty()
            && self.reads.is_empty()
            && self.constants.is_empty()
            && self.definitions.is_empty()
    }
}
