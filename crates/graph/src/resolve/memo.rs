//! One resolution for each `(specifier, from_style)` pair of an importing file.
//!
//! The extractor records one import entry for each binding, so
//! `import { a, b, c } from './x'` asks three times for `./x` from the same
//! file. The result depends only on the importing file, the specifier and the
//! style flag, so the first result serves the other bindings.
//!
//! The memo lives in a thread-local scope that covers one module. Resolution
//! runs one module per rayon task, so the scope closes with the module and a
//! result never crosses to another importing file. A lookup for a different
//! importing file inside the scope does not use the memo.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::ResolveResult;

struct FileMemo {
    from_file: PathBuf,
    /// Results for script context (`[0]`) and style context (`[1]`).
    results: [FxHashMap<Box<str>, ResolveResult>; 2],
}

impl FileMemo {
    fn serves(&self, from_file: &Path) -> bool {
        self.from_file.as_os_str() == from_file.as_os_str()
    }
}

thread_local! {
    static MEMO: RefCell<Option<FileMemo>> = const { RefCell::new(None) };
}

/// Restores the previous scope when the file scope ends, also on unwind.
struct ScopeGuard {
    previous: Option<FileMemo>,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        MEMO.with(|memo| memo.replace(previous));
    }
}

/// Run `resolve` with an empty memo for the importing file `from_file`.
pub(super) fn in_file_scope<T>(from_file: &Path, resolve: impl FnOnce() -> T) -> T {
    let fresh = FileMemo {
        from_file: from_file.to_path_buf(),
        results: [FxHashMap::default(), FxHashMap::default()],
    };
    let _guard = ScopeGuard {
        previous: MEMO.with(|memo| memo.replace(Some(fresh))),
    };
    resolve()
}

/// Return the memoized result for this pair, or run `resolve` and keep its
/// result for the next binding of the same specifier.
pub(super) fn resolve_once(
    from_file: &Path,
    specifier: &str,
    from_style: bool,
    resolve: impl FnOnce() -> ResolveResult,
) -> ResolveResult {
    let slot = usize::from(from_style);
    let cached = MEMO.with(|memo| {
        let memo = memo.borrow();
        let memo = memo.as_ref().filter(|memo| memo.serves(from_file))?;
        memo.results[slot].get(specifier).cloned()
    });
    if let Some(result) = cached {
        return result;
    }
    // The borrow is released here, so a nested resolution cannot conflict.
    let result = resolve();
    MEMO.with(|memo| {
        if let Some(memo) = memo
            .borrow_mut()
            .as_mut()
            .filter(|memo| memo.serves(from_file))
        {
            memo.results[slot].insert(specifier.into(), result.clone());
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn counted(calls: &Cell<u32>, value: &str) -> ResolveResult {
        calls.set(calls.get() + 1);
        ResolveResult::NpmPackage(value.to_string())
    }

    fn package(result: &ResolveResult) -> &str {
        match result {
            ResolveResult::NpmPackage(name) => name,
            other => panic!("unexpected result {other:?}"),
        }
    }

    #[test]
    fn one_file_resolves_each_pair_once() {
        let calls = Cell::new(0);
        let file = Path::new("/p/src/a.ts");
        in_file_scope(file, || {
            let first = resolve_once(file, "./x", false, || counted(&calls, "x"));
            let second = resolve_once(file, "./x", false, || counted(&calls, "other"));
            assert_eq!(package(&first), package(&second));
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn the_style_flag_is_part_of_the_key() {
        let calls = Cell::new(0);
        let file = Path::new("/p/src/a.vue");
        in_file_scope(file, || {
            resolve_once(file, "./x", false, || counted(&calls, "script"));
            let style = resolve_once(file, "./x", true, || counted(&calls, "style"));
            assert_eq!(package(&style), "style");
        });
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn another_importing_file_does_not_use_the_memo() {
        let calls = Cell::new(0);
        let file = Path::new("/p/src/a.ts");
        let other = Path::new("/p/lib/b.ts");
        in_file_scope(file, || {
            resolve_once(file, "./x", false, || counted(&calls, "a"));
            let from_other = resolve_once(other, "./x", false, || counted(&calls, "b"));
            assert_eq!(package(&from_other), "b");
            resolve_once(other, "./x", false, || counted(&calls, "b"));
        });
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn nothing_is_kept_outside_a_scope_or_between_scopes() {
        let calls = Cell::new(0);
        let file = Path::new("/p/src/a.ts");
        resolve_once(file, "./x", false, || counted(&calls, "x"));
        resolve_once(file, "./x", false, || counted(&calls, "x"));
        in_file_scope(file, || {
            resolve_once(file, "./x", false, || counted(&calls, "x"))
        });
        in_file_scope(file, || {
            resolve_once(file, "./x", false, || counted(&calls, "x"))
        });
        assert_eq!(calls.get(), 4);
    }
}
