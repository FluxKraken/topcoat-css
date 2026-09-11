//! Component-local CSS modules, compiled into one ordinary Topcoat asset.
//!
//! Add `topcoat-css-build` as a build dependency and call
//! `topcoat_css_build::BuildConfig::new().render()?` from `build.rs`.
//! Write `let style = css! { .card { display: grid; } };` inside components,
//! then use `style.card` in a view. Link [`stylesheet!`] once in your layout.
//!
//! See the repository README and the runnable `examples/topcoat-app` package.

pub use topcoat_css_macro::{css, stylesheet};
