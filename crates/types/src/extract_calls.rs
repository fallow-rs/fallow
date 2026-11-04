//! Persisted call-site facts: callee paths, imported calls, import binding
//! references and process commands.

/// A literal command that a process-spawning call runs.
///
/// The extractor records the call only when the callee resolves to a known
/// process API (`node:child_process`, `execa`, `zx`, `Bun`) and the command is
/// a string literal or the static head of a template.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct CommandInvocation {
    /// The command text. For a program call this is the program name. For a
    /// shell call this is the static shell text up to the first expression.
    pub command: String,
    /// True when a shell parses `command` (`exec`, `execSync`, `$` templates).
    pub shell: bool,
    /// Start byte offset of the command literal.
    pub span_start: u32,
}

/// A flattenable callee path of a call or `new` site; one per `(callee_path, is_new)` per module.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct CalleeUse {
    /// The dotted or bare callee path as written at the site (e.g. `cp.exec`).
    pub callee_path: String,
    /// Start byte offset of the first site with this path and form.
    pub span_start: u32,
    /// True for a `new` expression. `banned-effect` and the trace read only false entries.
    pub is_new: bool,
}

/// A direct call whose root identifier resolves to a runtime ESM import.
///
/// Every occurrence is retained. Computed or optional callees and arbitrary
/// value aliases are excluded; origin resolution through re-exports is deferred
/// to analysis.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ImportedCallSite {
    /// The local root import binding used by the callee.
    pub local_name: String,
    /// Dot-separated static member suffix, empty for a bare imported call.
    pub member_path: String,
    /// Static first string argument, or `None` for any other argument shape.
    pub first_argument: Option<String>,
    /// Start byte offset of this call site.
    pub span_start: u32,
}

/// A value reference to a runtime ESM import binding that is not an admitted
/// call site, or that initializes a top-level declarator.
///
/// The admission rules are the same as for [`ImportedCallSite`]: the binding
/// has one import declaration, it is not type-only, and the reference reads a
/// value. `fallow trace --dependency` reads these facts to count each use of
/// an imported name that it cannot resolve to a call, and to find project
/// wrappers such as `export const useX = useSelector.withTypes()`.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct ImportBindingReference {
    /// Index into `ModuleInfo.imports` of the root binding.
    pub import_index: u32,
    /// Static member suffix after the root, empty for a bare reference.
    pub member_path: Box<str>,
    /// How the code uses the reference.
    pub kind: ImportBindingReferenceKind,
    /// Start byte of the root identifier, or of the call for
    /// [`ImportBindingReferenceKind::InitializerCall`].
    pub span_start: u32,
    /// Name of the top-level declarator that this reference initializes.
    pub declared_name: Option<Box<str>>,
    /// The exported top-level declarator that the reference reads, when the
    /// reference names that declarator and not the import binding. The
    /// declarator is initialized by a reference to the import binding at
    /// `import_index`, so the reference is a use of the binding through a
    /// project wrapper in its own module. `None` for a direct reference.
    pub through: Option<Box<str>>,
}

/// How the code uses an [`ImportBindingReference`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ImportBindingReferenceKind {
    /// An admitted call that is the whole initializer of a top-level
    /// declarator: `const x = useSelector.withTypes<T>()`.
    InitializerCall,
    /// The reference is the whole initializer of a declarator:
    /// `const x = useDispatch`.
    ValueAlias,
    /// The reference names a JSX opening element: `<Provider>`.
    JsxElement,
    /// Any other value use that is not an admitted call, for example an
    /// argument, an array element or an optional call.
    Other,
    /// A direct call of a wrapper declarator in its own module. Only a
    /// reference with `through` set has this kind.
    Call,
}
