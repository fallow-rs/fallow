//! The code layout of a file that `fallow mutation` can instrument.
//!
//! A JavaScript or TypeScript file is one program. A Vue or Svelte
//! single-file component and an Astro component hold their code in script
//! blocks. The extract crate finds the blocks, so the plan and inspect use
//! the same blocks as the analysis.

use std::borrow::Cow;

use fallow_extract::sfc::SfcScript;
use fallow_mutation::{
    Instrumented, MutationError, MutationOptions, ScriptBlock, ScriptRole, SourceKind,
};

/// The framework of a component file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Component {
    Vue,
    Svelte,
    Astro,
}

/// How a file holds its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Layout {
    /// The whole file is one program.
    Script(SourceKind),
    /// A Svelte module file (`.svelte.js`, `.svelte.ts`): one program that
    /// can use runes.
    SvelteModule(SourceKind),
    /// The code is in the script blocks of a component file.
    Component(Component),
}

impl Layout {
    /// Selects the layout from the file extension of `path`.
    pub(super) fn from_path(path: &str) -> Option<Self> {
        if let Some(kind) = SourceKind::from_path(path) {
            let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
            let runes =
                stem.ends_with(".svelte") && matches!(kind, SourceKind::Js | SourceKind::Ts);
            return Some(if runes {
                Self::SvelteModule(kind)
            } else {
                Self::Script(kind)
            });
        }
        let component = match path.rsplit_once('.')?.1 {
            "vue" => Component::Vue,
            "svelte" => Component::Svelte,
            "astro" => Component::Astro,
            _ => return None,
        };
        Some(Self::Component(component))
    }

    /// Instruments `source`. `path` is the root-relative path.
    pub(super) fn instrument(
        self,
        source: &str,
        path: &str,
        options: &MutationOptions,
    ) -> Result<Instrumented, MutationError> {
        fallow_extract::parse_bounded(source, || self.instrument_unbounded(source, path, options))
            .unwrap_or_else(|| {
                Err(MutationError::Parse {
                    messages: vec!["the source nests brackets too deep to parse".to_owned()],
                })
            })
    }

    fn instrument_unbounded(
        self,
        source: &str,
        path: &str,
        options: &MutationOptions,
    ) -> Result<Instrumented, MutationError> {
        match self {
            Self::Script(kind) => fallow_mutation::instrument(source, path, kind, options),
            Self::SvelteModule(kind) => {
                let block = ScriptBlock {
                    span: (
                        0,
                        u32::try_from(source.len()).map_err(|_| MutationError::TooLarge)?,
                    ),
                    kind,
                    role: ScriptRole::SvelteModule,
                };
                fallow_mutation::instrument_blocks(source, path, &[block], options)
            }
            Self::Component(component) => fallow_mutation::instrument_blocks(
                source,
                path,
                &script_blocks(component, source),
                options,
            ),
        }
    }

    /// The text and language for a parse of the whole file with file
    /// offsets. For a component file, each byte outside the script blocks
    /// becomes a space, and each line break stays. The blocks then parse as
    /// one program at their own offsets.
    pub(super) fn parse_view(self, source: &str) -> (Cow<'_, str>, SourceKind) {
        let component = match self {
            Self::Script(kind) | Self::SvelteModule(kind) => return (Cow::Borrowed(source), kind),
            Self::Component(component) => component,
        };
        let blocks = script_blocks(component, source);
        let mut masked: Vec<u8> = source
            .bytes()
            .map(|byte| if byte == b'\n' { b'\n' } else { b' ' })
            .collect();
        for block in &blocks {
            let (start, end) = (block.span.0 as usize, block.span.1 as usize);
            masked[start..end].copy_from_slice(&source.as_bytes()[start..end]);
        }
        let typescript = blocks
            .iter()
            .any(|block| matches!(block.kind, SourceKind::Ts | SourceKind::Tsx));
        let jsx = blocks
            .iter()
            .any(|block| matches!(block.kind, SourceKind::Jsx | SourceKind::Tsx));
        let kind = match (typescript, jsx) {
            (true, true) => SourceKind::Tsx,
            (true, false) => SourceKind::Ts,
            (false, true) => SourceKind::Jsx,
            (false, false) => SourceKind::Js,
        };
        // Each block starts and ends on a character boundary, and each
        // replaced byte is ASCII, so the text stays UTF-8.
        let text = String::from_utf8(masked).unwrap_or_default();
        (Cow::Owned(text), kind)
    }

    /// The byte spans of the script blocks, or `None` when the whole file is
    /// one program.
    pub(super) fn script_spans(self, source: &str) -> Option<Vec<(u32, u32)>> {
        match self {
            Self::Script(_) | Self::SvelteModule(_) => None,
            Self::Component(component) => Some(
                script_blocks(component, source)
                    .iter()
                    .map(|block| block.span)
                    .collect(),
            ),
        }
    }
}

/// The inline script blocks of a component file. A block with a `src`
/// attribute has no inline code.
fn script_blocks(component: Component, source: &str) -> Vec<ScriptBlock> {
    match component {
        Component::Vue | Component::Svelte => fallow_extract::sfc::extract_sfc_scripts(source)
            .iter()
            .filter(|script| script.src.is_none())
            .filter_map(|script| {
                let role = match (component, script.is_setup, script.is_context_module) {
                    (Component::Vue, true, _) => ScriptRole::VueSetup,
                    (Component::Vue, false, _) => ScriptRole::VueScript,
                    (_, _, true) => ScriptRole::SvelteModule,
                    (_, _, false) => ScriptRole::SvelteInstance,
                };
                block(script, role)
            })
            .collect(),
        Component::Astro => fallow_extract::astro::extract_astro_frontmatter(source)
            .iter()
            .filter_map(|script| block(script, ScriptRole::AstroFrontmatter))
            .collect(),
    }
}

fn block(script: &SfcScript, role: ScriptRole) -> Option<ScriptBlock> {
    let start = u32::try_from(script.byte_offset).ok()?;
    let end = u32::try_from(script.byte_offset + script.body.len()).ok()?;
    let kind = match (script.is_typescript, script.is_jsx) {
        (true, true) => SourceKind::Tsx,
        (true, false) => SourceKind::Ts,
        (false, true) => SourceKind::Jsx,
        (false, false) => SourceKind::Js,
    };
    Some(ScriptBlock {
        span: (start, end),
        kind,
        role,
    })
}

/// True when `path` is a Vue, Svelte or Astro component file, whose code is
/// in script blocks. A Svelte module file (`.svelte.ts`) is not a component
/// file.
#[must_use]
pub fn is_component_file(path: &str) -> bool {
    matches!(Layout::from_path(path), Some(Layout::Component(_)))
}

/// Instruments a source file of any layout that `fallow mutation` supports:
/// JavaScript, TypeScript, and the script blocks of Vue, Svelte and Astro
/// components. Returns `None` for another file type.
///
/// `path` is the root-relative path. It selects the layout and is part of
/// each mutant id.
#[must_use]
pub fn instrument_source(
    source: &str,
    path: &str,
    options: &MutationOptions,
) -> Option<Result<Instrumented, MutationError>> {
    Layout::from_path(path).map(|layout| layout.instrument(source, path, options))
}
