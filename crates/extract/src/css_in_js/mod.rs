//! CSS-in-JS analytics front-ends (CSS program Phases 3b / 3c / 3d).
//!
//! Three front-ends feed the shared styling-analytics pipeline, converging on the
//! same virtual-stylesheet output that `compute_css_analytics` consumes:
//! - `template`: the Phase 3b lexical lifter for `` styled.div`...` `` tagged
//!   templates.
//! - `object`: the Phase 3c AST object-to-CSS serializer (`style({...})`,
//!   `stylex.create({...})`, ...).
//! - `tokens`: the Phase 3d design-token definition extraction + cross-module
//!   consumer scan (StyleX `defineVars`, vanilla-extract `createTheme`).
//!
//! `shared` holds the small invariants the front-ends agree on (the synthetic
//! wrapper selector, the newline counter).

mod object;
mod shared;
mod template;
mod tokens;

pub use object::{CssInJsObjectSheets, css_in_js_object_sheets};
pub use template::css_in_js_virtual_stylesheet;
pub use tokens::{
    CssInJsToken, CssInJsTokenDef, CssInJsTokenOrigin, TokenConsumerHit, css_in_js_theme_consumers,
    css_in_js_theme_token_defs, css_in_js_token_consumers, css_in_js_token_defs,
    panda_style_value_consumers, panda_token_call_consumers,
};
