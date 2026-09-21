use std::{env, fs};

mod editor;

use proc_macro::{Span, TokenStream};
use quote::quote;
use topcoat_css_core::{
    MANIFEST_ENV, MANIFEST_VERSION, Manifest, Module, STYLESHEET_ENV, fingerprint,
};

/// Return a const-compatible value with a static string field for each
/// local CSS module export. Requires the `topcoat-css-build` build helper.
#[proc_macro]
pub fn css(input: TokenStream) -> TokenStream {
    let span = input
        .clone()
        .into_iter()
        .next()
        .map_or(Span::call_site(), |t| t.span());
    expand_css(input, span).unwrap_or_else(|message| error(span, message))
}

fn error(span: Span, message: String) -> TokenStream {
    syn::Error::new(span.into(), message)
        .to_compile_error()
        .into()
}

fn expand_css(input: TokenStream, span: Span) -> Result<TokenStream, String> {
    if input.is_empty() {
        return Err("css! requires a CSS body or a string literal".into());
    }
    let manifest_path = env::var(MANIFEST_ENV).map_err(|_| {
        "css! requires a build script: add topcoat-css-build to [build-dependencies] and call topcoat_css_build::BuildConfig::new().render().unwrap() in build.rs".to_string()
    })?;
    let manifest: Manifest = serde_json::from_slice(&fs::read(&manifest_path).map_err(|e| {
        format!("cannot read CSS build manifest {manifest_path}: {e}; rerun the build script")
    })?)
    .map_err(|e| format!("invalid CSS build manifest: {e}"))?;
    if manifest.version != MANIFEST_VERSION {
        return Err("incompatible CSS build manifest; use matching topcoat-css and topcoat-css-build versions".into());
    }
    let tokens = fingerprint(input.into());
    let Some(file) = span.local_file() else {
        return editor_expansion(&manifest, &tokens).map(Into::into);
    };
    let file =
        fs::canonicalize(&file).map_err(|e| format!("cannot locate {}: {e}", file.display()))?;
    let module = manifest.modules.iter().find(|module| {
        std::path::Path::new(&module.file) == file
            && module.line == span.line()
            && module.column == span.column()
            && module.fingerprint == tokens
    }).ok_or_else(|| format!(
        "css! at {}:{}:{} was not collected by the build script; add its source directory with BuildConfig::source(...), register a renamed macro with macro_name(...), and ensure the build script reruns when sources change. Macro-generated CSS is not supported",
        span.file(), span.line(), span.column()
    ))?;
    module_expansion(module, false).map(Into::into)
}

// Some proc-macro hosts (including rust-analyzer's legacy protocol) omit file
// paths and line/column information. Token contents can recover the field shape,
// but cannot identify scoped values: identical CSS in two calls has two hashes.
fn editor_expansion(manifest: &Manifest, tokens: &str) -> Result<proc_macro2::TokenStream, String> {
    let mut matches = manifest
        .modules
        .iter()
        .filter(|module| module.fingerprint == tokens);
    let module = matches.next().ok_or(
        "css! editor analysis needs an up-to-date CSS build manifest; save the Rust source and run cargo check",
    )?;
    if matches.any(|other| !module.fields.keys().eq(other.fields.keys())) {
        return Err("css! editor analysis cannot determine the exported fields without source locations: matching token contents have different exports; use string-literal CSS to distinguish whitespace-sensitive bodies".into());
    }
    module_expansion(module, true)
}

fn module_expansion(module: &Module, editor: bool) -> Result<proc_macro2::TokenStream, String> {
    let fields = module
        .fields
        .keys()
        .map(|name| syn::parse_str::<syn::Ident>(name))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("invalid generated CSS field: {e}"))?;
    let values: Vec<_> = module
        .fields
        .values()
        .map(|value| if editor { "" } else { value.as_str() })
        .collect();
    // Keep field completion/type checking available without inventing scoped
    // values. This fallback must never produce a runnable build, even if a
    // compiler (rather than an editor) supplies a span without a local file.
    let guard = editor.then(editor_guard);
    let manifest_dependency = (!editor).then(|| {
        quote! {
            const _: &str = include_str!(env!("TOPCOAT_CSS_MANIFEST"));
        }
    });
    Ok(quote! {{
        #guard
        // Track the artifact read during expansion for Cargo/rustc incremental builds.
        #manifest_dependency
        #[allow(dead_code, non_snake_case)]
        #[derive(Clone, Copy, Debug)]
        struct __TopcoatCssModule {
            #(pub #fields: &'static str,)*
        }
        __TopcoatCssModule { #(#fields: #values,)* }
    }})
}

fn editor_guard() -> proc_macro2::TokenStream {
    quote! {
        const _: () = ::core::panic!("topcoat-css editor placeholders cannot be compiled; build without --cfg rust_analyzer and call BuildConfig::render() in build.rs");
    }
}

/// Internal expansion selected by topcoat-css under `cfg(rust_analyzer)`.
#[doc(hidden)]
#[proc_macro]
pub fn css_editor(input: TokenStream) -> TokenStream {
    editor::module(input.into())
        .and_then(|module| module_expansion(&module, true))
        .map(Into::into)
        .unwrap_or_else(|message| error(Span::call_site(), message))
}

/// Internal asset type placeholder; never depends on build-script output.
#[doc(hidden)]
#[proc_macro]
pub fn stylesheet_editor(input: TokenStream) -> TokenStream {
    let topcoat: syn::Path = if input.is_empty() {
        syn::parse_quote!(::topcoat)
    } else {
        match syn::parse(input) {
            Ok(path) => path,
            Err(e) => return e.to_compile_error().into(),
        }
    };
    let guard = editor_guard();
    quote! {{
        #guard
        #topcoat::asset::asset!("__topcoat_css_editor.css")
    }}
    .into()
}

/// Declare the generated stylesheet as a Topcoat asset. Link once in the layout.
/// Optionally pass a renamed Topcoat crate path: `stylesheet!(my_topcoat)`.
#[proc_macro]
pub fn stylesheet(input: TokenStream) -> TokenStream {
    let span = Span::call_site();
    let topcoat: syn::Path = if input.is_empty() {
        syn::parse_quote!(::topcoat)
    } else {
        match syn::parse(input) {
            Ok(path) => path,
            Err(e) => return e.to_compile_error().into(),
        }
    };
    if env::var_os(STYLESHEET_ENV).is_none() {
        return error(
            span,
            "stylesheet! requires topcoat_css_build::BuildConfig::new().render() in build.rs"
                .into(),
        );
    }
    quote! {{
        const _: &str = include_str!(env!("TOPCOAT_CSS_STYLESHEET"));
        #topcoat::asset::asset!(env!("TOPCOAT_CSS_STYLESHEET"))
    }}
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, process::Command};

    fn module(fields: &[(&str, &str)]) -> Module {
        Module {
            file: "/src/main.rs".into(),
            line: 1,
            column: 1,
            fingerprint: "same tokens".into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn editor_requires_matching_tokens_and_unambiguous_field_names() {
        let mut manifest = Manifest {
            version: MANIFEST_VERSION,
            modules: vec![module(&[("card", "card_tc_first")])],
        };
        assert!(
            editor_expansion(&manifest, "unsaved edit")
                .unwrap_err()
                .contains("save the Rust source")
        );
        manifest.modules.push(module(&[("card", "card_tc_second")]));
        assert!(editor_expansion(&manifest, "same tokens").is_ok());
        manifest
            .modules
            .push(module(&[("card_title", "card_title_tc_third")]));
        assert!(
            editor_expansion(&manifest, "same tokens")
                .unwrap_err()
                .contains("different exports")
        );
    }

    #[test]
    fn compiler_cannot_accept_editor_placeholders_even_when_unused() {
        let directory = tempfile::tempdir().unwrap();
        let manifest_path = directory.path().join("manifest.json");
        fs::write(&manifest_path, "{}").unwrap();
        // Include a module with no local exports: its guard must still run.
        for fields in [
            BTreeMap::from([("card".into(), "card_tc_first".into())]),
            BTreeMap::new(),
        ] {
            let module = Module {
                fields,
                ..module(&[])
            };
            for editor in [false, true] {
                let expansion = module_expansion(&module, editor).unwrap();
                let source = directory.path().join("main.rs");
                fs::write(&source, format!("fn main() {{ let _ = {expansion}; }}")).unwrap();
                let output = Command::new("rustc")
                    .arg("--edition=2024")
                    .arg("--emit=metadata")
                    .arg("--out-dir")
                    .arg(directory.path())
                    .arg(&source)
                    .env(MANIFEST_ENV, &manifest_path)
                    .output()
                    .expect("run rustc");
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert_eq!(output.status.success(), !editor, "{stderr}");
                if editor {
                    assert!(
                        stderr.contains("topcoat-css editor placeholders cannot be compiled"),
                        "{stderr}"
                    );
                }
            }
        }
    }
}
