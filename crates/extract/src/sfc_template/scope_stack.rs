//! The template locals of the open scopes of a markup scan.

/// The template locals of the open scopes, outermost first, in one list.
///
/// A scan reads the locals in scope at each tag and at each expression. A copy
/// of all locals for each read costs locals times elements. The stack keeps
/// one list and the start of each scope in it, so a read is a slice and a
/// scope exit is a truncate.
///
/// The stack always holds the root scope. `K` names the kind of a scope, for
/// a scan that closes a scope by its kind.
#[derive(Debug)]
pub(super) struct ScopeStack<K> {
    locals: Vec<String>,
    /// The kind of each open scope and the index of its first local.
    frames: Vec<(K, usize)>,
}

impl<K: Copy + PartialEq> ScopeStack<K> {
    /// A stack that holds only the root scope, with no locals.
    pub(super) fn new(root: K) -> Self {
        Self {
            locals: Vec::new(),
            frames: vec![(root, 0)],
        }
    }

    /// The locals of all open scopes, outermost first.
    pub(super) fn locals(&self) -> &[String] {
        &self.locals
    }

    /// The number of open scopes, the root scope included.
    pub(super) fn depth(&self) -> usize {
        self.frames.len()
    }

    /// The kind of the innermost scope.
    pub(super) fn innermost_kind(&self) -> Option<K> {
        self.frames.last().map(|&(kind, _)| kind)
    }

    /// The index of the innermost scope of `kind`.
    pub(super) fn innermost_index_of(&self, kind: K) -> Option<usize> {
        self.frames.iter().rposition(|&(frame, _)| frame == kind)
    }

    /// Open a scope of `kind` that declares `locals`.
    pub(super) fn push(&mut self, kind: K, locals: impl IntoIterator<Item = String>) {
        self.frames.push((kind, self.locals.len()));
        self.locals.extend(locals);
    }

    /// Close every scope from index `depth` on. The root scope stays open.
    pub(super) fn truncate(&mut self, depth: usize) {
        let depth = depth.max(1);
        if let Some(&(_, start)) = self.frames.get(depth) {
            self.locals.truncate(start);
            self.frames.truncate(depth);
        }
    }

    /// Close the innermost scope, unless it is the root scope.
    pub(super) fn pop(&mut self) {
        self.truncate(self.frames.len() - 1);
    }

    /// Add `locals` to the innermost scope.
    pub(super) fn extend_innermost(&mut self, locals: impl IntoIterator<Item = String>) {
        self.locals.extend(locals);
    }

    /// Replace the locals of the scope at `index` with `locals`.
    pub(super) fn replace_locals(&mut self, index: usize, locals: Vec<String>) {
        let Some(&(_, start)) = self.frames.get(index) else {
            return;
        };
        let end = self
            .frames
            .get(index + 1)
            .map_or(self.locals.len(), |&(_, next)| next);
        let added = locals.len();
        self.locals.splice(start..end, locals);
        for frame in &mut self.frames[index + 1..] {
            frame.1 = frame.1 - (end - start) + added;
        }
    }

    /// Run `read` with the locals of all open scopes followed by `extra`, as
    /// if a scope declared `extra`. The stack does not change.
    pub(super) fn read_with<R>(
        &mut self,
        extra: impl IntoIterator<Item = String>,
        read: impl FnOnce(&[String]) -> R,
    ) -> R {
        let len = self.locals.len();
        self.locals.extend(extra);
        let result = read(&self.locals);
        self.locals.truncate(len);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::ScopeStack;

    fn names(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn scopes_open_and_close_in_order() {
        let mut stack = ScopeStack::new(0);
        stack.push(1, names(&["a", "b"]));
        stack.push(2, names(&["c"]));
        assert_eq!(stack.locals(), names(&["a", "b", "c"]));
        assert_eq!(stack.innermost_kind(), Some(2));
        stack.pop();
        assert_eq!(stack.locals(), names(&["a", "b"]));
        stack.pop();
        stack.pop();
        assert_eq!(stack.depth(), 1);
        assert!(stack.locals().is_empty());
    }

    #[test]
    fn truncate_keeps_the_root_scope() {
        let mut stack = ScopeStack::new(0);
        stack.extend_innermost(names(&["root"]));
        stack.push(1, names(&["a"]));
        stack.push(2, names(&["b"]));
        stack.truncate(stack.innermost_index_of(1).unwrap_or(0));
        assert_eq!(stack.locals(), names(&["root"]));
        stack.truncate(0);
        assert_eq!(stack.locals(), names(&["root"]));
        assert_eq!(stack.depth(), 1);
    }

    #[test]
    fn replace_locals_moves_the_inner_scopes() {
        let mut stack = ScopeStack::new(0);
        stack.push(1, names(&["a", "b"]));
        stack.push(2, names(&["c"]));
        stack.replace_locals(1, names(&["x", "y", "z"]));
        assert_eq!(stack.locals(), names(&["x", "y", "z", "c"]));
        stack.pop();
        assert_eq!(stack.locals(), names(&["x", "y", "z"]));
        stack.replace_locals(1, Vec::new());
        assert!(stack.locals().is_empty());
    }

    #[test]
    fn read_with_adds_locals_for_the_read_only() {
        let mut stack = ScopeStack::new(0);
        stack.push(1, names(&["a"]));
        let seen = stack.read_with(names(&["b"]), <[String]>::to_vec);
        assert_eq!(seen, names(&["a", "b"]));
        assert_eq!(stack.locals(), names(&["a"]));
    }
}
