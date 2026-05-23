use std::path::Path;

use fallow_types::discover::FileId;

use crate::parse::parse_source_to_module;

#[test]
fn extracts_mdx_imports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("post.mdx"),
        r"import { Chart } from './Chart'
import Button from './Button'

# My Post

Some markdown content here.

<Chart data={[1, 2, 3]} />
<Button>Click me</Button>
",
        0,
        false,
    );
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports.iter().any(|i| i.source == "./Chart"));
    assert!(info.imports.iter().any(|i| i.source == "./Button"));
}

#[test]
fn extracts_mdx_exports() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("post.mdx"),
        r"export const meta = { title: 'Hello' }

# My Post

Content here.
",
        0,
        false,
    );
    assert!(!info.exports.is_empty());
}

#[test]
fn mdx_no_imports_returns_empty() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("simple.mdx"),
        "# Just Markdown\n\nNo imports here.\n",
        0,
        false,
    );
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
}

#[test]
fn mdx_multiline_import() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("multi.mdx"),
        r"import {
  Chart,
  Table,
  Graph
} from './components'

# Dashboard

<Chart />
",
        0,
        false,
    );
    assert_eq!(info.imports.len(), 3);
    assert!(info.imports.iter().all(|i| i.source == "./components"));
}

#[test]
fn mdx_imports_between_content() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("mixed.mdx"),
        r"import { Header } from './Header'

# Section 1

Some content.

import { Footer } from './Footer'

## Section 2

More content.
",
        0,
        false,
    );
    assert_eq!(info.imports.len(), 2);
    assert!(info.imports.iter().any(|i| i.source == "./Header"));
    assert!(info.imports.iter().any(|i| i.source == "./Footer"));
}

#[test]
fn mdx_ignores_imports_inside_code_fences() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("docs.mdx"),
        r"import { Live } from './Live'

# Example

```ts
// file: exampleSlice.ts
import exampleSliceReducer from './exampleSlice'
import { store } from './store'
```
",
        0,
        false,
    );
    assert_eq!(info.imports.len(), 1);
    assert!(info.imports.iter().any(|i| i.source == "./Live"));
    assert!(!info.imports.iter().any(|i| i.source == "./exampleSlice"));
    assert!(!info.imports.iter().any(|i| i.source == "./store"));
}

#[test]
fn mdx_ignores_exports_inside_code_fences() {
    let info = parse_source_to_module(
        FileId(0),
        Path::new("docs.mdx"),
        r"# Example

```tsx
export const Example = () => null
```
",
        0,
        false,
    );
    assert!(info.imports.is_empty());
    assert!(info.exports.is_empty());
}
