//! # topcoat-css
//!
//! Write CSS beside a Topcoat component, refer to classes through checked Rust
//! fields, and load one stylesheet from your layout.
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat::{Result, view::{View, component, view}};
//! use topcoat_css::css;
//!
//! #[component]
//! async fn user_card() -> Result<impl View> {
//!     let style = css! {
//!         .card {
//!             display: grid;
//!             grid-template-columns: auto 1fr;
//!             gap: 1rem;
//!         }
//!
//!         .title { font-weight: bold; }
//!         .card:hover .title { color: #285d40; }
//!     };
//!
//!     Ok(view! {
//!         <article class=(style.card)>
//!             <h2 class=(style.title)>"User"</h2>
//!         </article>
//!     })
//! }
//! ```
//!
//! `style.card` is a `&'static str` such as `card_tc123…`.
//! A typo like `style.cadr` is a Rust compiler error. Each invocation gets its own
//! names, including when two components use identical CSS. No per-component CSS
//! registration, CSS injection, or component-level `<link>` is needed.
//!
//! ## Add to an application
//!
//! These examples are parts of a Cargo application with its own `build.rs`.
//! They are compiled together by `tests/doc_examples.rs`; standalone rustdoc
//! execution is disabled because it cannot provide that build-script context.
//!
//! ```toml
//! [dependencies]
//! topcoat = { version = "0.7", default-features = false, features = ["asset", "view", "router", "serve", "discover"] }
//! topcoat-css = "=0.1.2"
//! tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
//!
//! [build-dependencies]
//! topcoat-css-build = "=0.1.2"
//! ```
//!
//! Keep `topcoat-css` and `topcoat-css-build` on matching versions. The example
//! setup explicitly enables Topcoat's `asset`, `view`, `router`, `serve`, and
//! `discover` features; these are also enabled by Topcoat's default features.
//! `discover` is needed for `.discover()` below. Applications that register routes
//! explicitly can omit it. Using `css!` alone does not require a Topcoat dependency.
//!
//! To use a local checkout, replace the two CSS dependencies with paths:
//! `topcoat-css = { path = "../topcoat_css_package" }` and
//! `topcoat-css-build = { path = "../topcoat_css_package/build" }`.
//!
//! Create or extend `build.rs`:
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! fn main() {
//!     topcoat_css_build::BuildConfig::new().render().unwrap();
//! }
//! ```
//!
//! Link the generated asset **once in your root layout**:
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat::{
//!     Result,
//!     router::{Slot, layout},
//!     view::{View, view},
//! };
//!
//! #[layout("/")]
//! async fn root_layout(slot: Slot<'_>) -> Result<impl View> {
//!     Ok(view! {
//!         <!DOCTYPE html>
//!         <html>
//!             <head>
//!                 <link rel="stylesheet" href=(topcoat_css::stylesheet!())>
//!             </head>
//!             <body>(slot)</body>
//!         </html>
//!     })
//! }
//! ```
//!
//! Register the asset bundle on your router, **including when using `topcoat dev`**:
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat::{
//!     asset::{AssetBundle, RouterBuilderAssetExt},
//!     router::{Router, RouterBuilderDiscoverExt},
//! };
//!
//! #[tokio::main]
//! async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//!     let router = Router::builder()
//!         .discover()
//!         .assets(AssetBundle::load()?)
//!         .build();
//!     topcoat::start(router).await?;
//!     Ok(())
//! }
//! ```
//!
//! Keep any existing route registrations and application context in that builder
//! chain. `RouterBuilderAssetExt` brings `.assets(...)` into scope; the call
//! registers the asset configuration and serves the bundled files. Omitting it
//! causes a `no asset config registered in this router context` panic when the
//! stylesheet link renders.
//!
//! Run `topcoat dev` to build the application and bundle its assets before
//! starting it. For a manual build, build and run `topcoat asset bundle` for the
//! same package and profile before launching the application. Both workflows still
//! require the `.assets(AssetBundle::load()?)` call above.
//!
//! `stylesheet!()` returns a const-compatible `topcoat::asset::Asset`.
//! The handle must be used in a live code path so Topcoat can discover its
//! declaration. _See [Topcoat's asset documentation](https://docs.rs/topcoat/0.7.0/topcoat/asset/index.html)._
//!
//! The CSS file is generated in Cargo's `OUT_DIR`. Topcoat copies it into its
//! asset bundle and gives it a content-hashed URL. Build and bundle each deployment
//! together; `OUT_DIR` asset identities differ across checkouts and build profiles.
//!
//! ## CSS syntax and exports
//!
//! The build helper uses [Lightning CSS](https://lightningcss.dev/css-modules.html)
//! to parse and scope actual CSS. Classes, IDs, keyframes, and other supported CSS
//! module identifiers, including grid and container names, are scoped consistently. Custom properties
//! such as `--foreground` remain unchanged.
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat_css::css;
//!
//! let style = css! {
//!     :global(.dark-theme) .card { color: var(--foreground); }
//!     .card { animation: fade-in 200ms ease; }
//!     .user-name { font-weight: 600; }
//!
//!     @media (width < 800px) {
//!         .card { display: block; }
//!     }
//!
//!     @keyframes fade-in {
//!         from { opacity: 0; }
//!         to { opacity: 1; }
//!     }
//! };
//! ```
//!
//! - `.user-name` exports `style.user_name`.
//! - Keywords use raw identifiers: `.type` exports `style.r#type`.
//! - Colliding fields such as `.user-name` and `.user_name` fail the build. Names
//!   that cannot be Rust fields, including `self`, `Self`, `super`, `crate`,
//!   and `_`, are rejected.
//! - `:global(.external)` leaves the class unchanged and does not export a field.
//!   Element selectors and global rules can still affect the whole document;
//!   CSS Modules do not provide a Shadow DOM boundary.
//! - Nested selectors, media/supports rules, and local/global `composes` are
//!   supported. Composed exports contain a space-separated class list, including
//!   transitive compositions.
//! - IDs and animation names also appear in the export object. Use an ID export
//!   with `id=(style.some_id)`, not with `class`.
//! - Without interpolation, the generated value implements `Copy`, `Clone`, and
//!   `Debug`, and can be used in const expressions. There is no automatically added
//!   stylesheet handle. Use `stylesheet!()` for the application stylesheet.
//!
//! Bare CSS must also be accepted by Rust's tokenizer. Use double-quoted CSS
//! strings and `/* CSS comments */` in token bodies. CSS containing single-quoted
//! strings, backslash escapes, or URLs that Rust cannot tokenize belongs in
//! a **Rust string literal**:
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat_css::css;
//!
//! let style = css!(r#"
//!     .card::before { content: 'Hello'; }
//!     .card { background-image: url('https://example.com/photo.png'); }
//! "#);
//! ```
//!
//! The string form has the same scoping and field access. Whitespace-sensitive
//! selectors are preserved in both forms; `.card :hover` and `.card:hover` remain
//! different selectors. Rust `//` comments are not CSS comments.
//!
//! ## Runtime values
//!
//! Use `$(expression)` in token-body declaration values to capture Rust values:
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! use topcoat::{Result, view::{View, component, view}};
//! use topcoat_css::css;
//!
//! #[component]
//! async fn header(bg: &str, fg: &str) -> Result<impl View> {
//!     let colors = css! {
//!         .header {
//!             --header-bg: $(bg);
//!             --header-fg: $(fg);
//!             background: var(--header-bg);
//!             color: var(--header-fg);
//!         }
//!     };
//!
//!     Ok(view! {
//!         <header class=(colors.header) style=(colors.style())>
//!             "Header"
//!         </header>
//!     })
//! }
//! ```
//!
//! The stylesheet stays static: each expression becomes a reference to a generated,
//! module-specific CSS variable. `colors.style()` returns the inline declarations
//! that supply those variables. Apply it to the styled element or a common ancestor
//! of elements using the module. CSS variables inherit into descendants, so separate
//! instances of the same component can use different values with the same classes.
//! Without the style attribute, the generated variables have no supplied values.
//!
//! Expressions must implement `Display`. They are evaluated and formatted once, in
//! source order, when `css!` executes; the module owns the resulting strings and does
//! not retain references to the inputs. Calling `.style()` borrows the stored text
//! without evaluating expressions again. Interpolated modules implement `Clone` and
//! `Debug`, but are neither `Copy` nor const-compatible. Their exported class fields
//! remain `&'static str`. Modules without interpolation also have an empty `.style()`.
//!
//! Interpolation works in custom properties and ordinary declaration values,
//! including values inside CSS functions, nested rules, media rules, and keyframes.
//! It follows CSS `var(...)` semantics: use `$(format!("{width}px"))` or
//! `calc($(width) * 1px)` for a length, rather than `$(width)px`. Runtime identifiers
//! are not automatically scoped; pass an exported name when referencing a local
//! keyframe or other CSS module identifier.
//!
//! Selectors, property names, at-rule conditions, and the contents of CSS strings
//! or `url(...)` cannot be interpolated. For a dynamic URL, interpolate a complete
//! CSS value such as `$(format!("url(\"{url}\")"))`. Interpolation is not performed in
//! the string-literal form of `css!` or inside quoted CSS strings and comments.
//!
//! Values are CSS text, not automatically quoted CSS strings. Supply CSS quotes
//! when a property such as `content` requires them. Runtime token serialization keeps
//! each value inside its generated declaration; malformed tokens, top-level
//! semicolons, and `!important` cause a panic at the `css!` call. Put `!important`
//! after the interpolation in the CSS declaration if needed. Property-specific
//! value validity remains the browser's responsibility. Normal HTML attribute
//! escaping is handled by the view renderer.
//!
//! ## Collection and rebuilds
//!
//! `BuildConfig` recursively scans `src/**/*.rs` by default. Files are combined in
//! sorted path order, with invocations in source order. Overlapping source roots do
//! not duplicate files. Scoped hashes include the package namespace, relative
//! source path, invocation index, and CSS text; they do not include absolute
//! checkout paths or line numbers. Formatting CSS outside interpolation expressions
//! changes its hash.
//! Adding or removing an earlier invocation in the same file also changes the
//! hashes of later invocations. File order determines the cascade for global rules.
//!
//! ```rust,ignore (compiled as a Cargo application by tests/doc_examples.rs)
//! fn main() {
//!     topcoat_css_build::BuildConfig::new()
//!         .source("components") // Additional source directory or individual .rs file.
//!         .macro_name("component_css") // For `use topcoat_css::css as component_css`.
//!         .minify(true) // Compact CSS output without reordering modules.
//!         .render()
//!         .unwrap();
//! }
//! ```
//!
//! Use `.clear_sources().source("...")` to replace the default source root. Renamed
//! Topcoat dependencies work with `topcoat_css::stylesheet!(my_topcoat)`. Renaming
//! the `topcoat-css` dependency itself needs no special configuration.
//! `.macro_name(...)` adds an alias while continuing to collect `css!`.
//! Source roots must exist and remain inside the package directory.
//!
//! For custom build drivers, `.manifest_dir(...)`, `.out_dir(...)`, and
//! `.namespace(...)` override `CARGO_MANIFEST_DIR`, `OUT_DIR`, and the package name.
//! `.compile()` produces files without printing Cargo directives; `.render()` also
//! prints the environment and rebuild directives required by normal Cargo builds.
//! Both return a `BuildOutput` with `stylesheet` and `manifest` paths and a
//! `module_count`. Failures return a `BuildError` with source or filesystem context.
//!
//! `.minify(true)` compacts printed CSS. It does not configure browser targets,
//! add a compatibility pass, or merge and reorder rules across modules.
//!
//! The helper watches source directories, including additions and deletions, and
//! rewrites the combined stylesheet from scratch when needed. Unchanged output
//! files are left untouched. During compilation, procedural macros only read the
//! build manifest; they never write stylesheets or append to global files. Macro
//! expansions track the generated manifest and stylesheet with `include_str!` so
//! Rust observes artifact changes.
//!
//! Collection happens before Rust macro expansion. The scanner reads original
//! source text and records each invocation's location and token contents. `css!`
//! uses stable source-location APIs to select and verify its build record during
//! compilation; it does not reconstruct CSS using `TokenStream::to_string()` or
//! best-effort `Span::source_text()`. Write CSS directly in scanned files; CSS produced by
//! another macro or generated later in the build is unsupported.
//!
//! ### Editor support
//!
//! `css!` and `stylesheet!` have dedicated expansions for rust-analyzer, including
//! Neovim and Zed. The editor derives CSS fields from the current macro input using
//! the same CSS compiler and field-name rules as the build helper. It does not need
//! `TOPCOAT_CSS_MANIFEST`, `TOPCOAT_CSS_STYLESHEET`, or a successful application build.
//! Completions and field checking update on unsaved edits, and recover after empty
//! or invalid CSS is corrected without restarting the language server.
//! Editor hosts that omit source locations use the same input-based fallback, even
//! when they select the ordinary macros. Missing or stale build manifests do not
//! block that fallback.
//!
//! This also works while `topcoat dev` rebuilds the application. Saving incomplete
//! CSS can still produce a real build-script error in Cargo, Clippy, or the dev
//! server; fix the CSS and save again. Editor analysis remains independent of those
//! build results. Keep rust-analyzer's proc-macro support enabled.
//!
//! Editor expansions provide types only. Their placeholder values are guarded so
//! they cannot compile into an application, even if `--cfg rust_analyzer` is passed
//! to rustc. Normal builds still require `BuildConfig::render()`, verify source
//! locations, and use the exact source text to generate scoped CSS. Without source
//! locations, token-body whitespace is approximated for editor field discovery;
//! use string-literal CSS if whitespace changes which names are exported.
//!
//! Editor regression tests require `rust-analyzer` on `PATH`:
//!
//! ```sh
//! cargo test --test cargo_workflow rust_analyzer -- --ignored
//! cargo test --test editor_workflow -- --ignored
//! ```
//!
//! The latter tests saved and unsaved edits without restarting the language server,
//! with both missing and stale build manifests. It covers the dedicated editor macros
//! and the ordinary macros' fallback. Set `RUST_ANALYZER` to select a particular binary.
//!
//! ## Initial limitations
//!
//! - **Collection is lexical.** It includes invocations in inactive `#[cfg]`
//!   branches, unused modules, and test code under scanned roots. Every collected body
//!   must be valid CSS. It does not perform tree shaking or resolve Rust imports.
//!   Other macros named `css!` in scanned files will also be collected.
//! - **One package per bundle.** Dependency crates are not scanned automatically.
//!   Each reusable component crate can build its own stylesheet and expose an asset
//!   for the application to link separately.
//! - **Runtime values use CSS variables.** Interpolation requires applying
//!   `.style()` to the relevant element or ancestor. Selectors and at-rule conditions
//!   remain static. Cross-file `composes` is unsupported.
//! - **Resource paths are explicit.** Relative `url(images/photo.png)` is rejected
//!   because moving CSS into the asset bundle would change its meaning.
//!   Use root-relative, absolute, data, or fragment URLs. This library does not copy,
//!   download, or rewrite resources referenced by CSS.
//! - **No CSS imports or namespaces.** `@import`, `@namespace`, and unrecognized
//!   at-rules are rejected rather than concatenated into a potentially invalid or
//!   incorrectly scoped stylesheet. Link external stylesheets separately.
//! - **Source files must be local to the package.** Add nonstandard directories
//!   explicitly. Symlinks encountered within source directories cause a build error.
//!   Explicit source roots are canonicalized before scanning. The scanner does not
//!   inherit ignore-file rules; point it at source directories, not the
//!   package root or `target/`.
//! - **Formatting support is separate.** This crate does not add `css!` formatting
//!   to `topcoat fmt` or rustfmt.
//!
//! Syntax is checked by Lightning CSS, with errors reported at build time. CSS
//! property names and values that the parser preserves as unknown tokens are not
//! guaranteed to be semantically valid in every browser.

extern crate self as topcoat_css;

#[cfg(not(rust_analyzer))]
pub use topcoat_css_macro::{css, stylesheet};

#[doc(hidden)]
#[path = "value.rs"]
pub mod __private;

// Select these in the analyzed crate, not with cfg! inside a compiled proc macro:
// rust-analyzer loads the same proc-macro library that Cargo builds for rustc.
#[cfg(rust_analyzer)]
pub use topcoat_css_macro::{css_editor as css, stylesheet_editor as stylesheet};
