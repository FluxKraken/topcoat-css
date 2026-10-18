use std::{env, fs};

mod editor;

use proc_macro::{Span, TokenStream};
use quote::quote;
use topcoat_css_core::{
    MANIFEST_ENV, MANIFEST_VERSION, Manifest, Module, STYLESHEET_ENV, fingerprint, interpolation,
};

/// Return checked local CSS exports. Use `$(expression)` in declaration values
/// and apply the returned `.style()` to the element for runtime values.
/// Without interpolation, the result is const-compatible and Copy.
/// Requires the `topcoat-css-build` build helper.
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
    // Some editor hosts reach the ordinary proc macro rather than css_editor,
    // and omit local source paths. Use the current input before consulting any
    // build output: the manifest can be missing or stale throughout an edit.
    let Some(file) = span.local_file() else {
        return editor_expansion(input.into()).map(Into::into);
    };
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
    let input: proc_macro2::TokenStream = input.into();
    let tokens = fingerprint(input.clone());
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
    module_expansion(module, false, expressions(input)?).map(Into::into)
}

// Both editor entrypoints derive fields from the current buffer and use guarded
// placeholders. Neither generated CSS nor its manifest participates in analysis.
fn editor_expansion(input: proc_macro2::TokenStream) -> Result<proc_macro2::TokenStream, String> {
    module_expansion(&editor::module(input.clone())?, true, expressions(input)?)
}

fn expressions(input: proc_macro2::TokenStream) -> Result<Vec<syn::Expr>, String> {
    if syn::parse2::<syn::LitStr>(input.clone()).is_ok() {
        return Ok(Vec::new());
    }
    Ok(interpolation::extract(input)?
        .1
        .into_iter()
        .map(|item| item.expression)
        .collect())
}

fn runtime_crate() -> Result<proc_macro2::TokenStream, String> {
    match proc_macro_crate::crate_name("topcoat-css").map_err(|e| e.to_string())? {
        proc_macro_crate::FoundCrate::Itself => Ok(quote!(::topcoat_css)),
        proc_macro_crate::FoundCrate::Name(name) => {
            let ident = syn::parse_str::<syn::Ident>(&name).map_err(|e| e.to_string())?;
            Ok(quote!(::#ident))
        }
    }
}

fn module_expansion(
    module: &Module,
    editor: bool,
    expressions: Vec<syn::Expr>,
) -> Result<proc_macro2::TokenStream, String> {
    if expressions.len() != module.variables.len() {
        return Err("CSS interpolation does not match the build manifest; rerun the build script with matching topcoat-css and topcoat-css-build versions".into());
    }
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
    let mut derives = quote!(Clone, Copy, Debug);
    let mut storage = quote!();
    let mut initialize = quote!();
    let mut style = quote!("");
    if !expressions.is_empty() {
        derives = quote!(Clone, Debug);
        // CSS exports can use any valid field name, including our preferred one.
        let mut private_name = "__topcoat_css_style".to_owned();
        while module.fields.contains_key(&private_name) {
            private_name.push('_');
        }
        let private = syn::Ident::new(&private_name, proc_macro2::Span::mixed_site());
        storage = quote!(#private: ::std::string::String,);
        style = quote!(&self.#private);
        let format: String = module
            .variables
            .iter()
            .map(|name| format!("{name}:{{}};"))
            .collect();
        let values = if editor {
            expressions
                .iter()
                .map(|expr| quote!(&(#expr)))
                .collect::<Vec<_>>()
        } else {
            let runtime = runtime_crate()?;
            expressions
                .iter()
                .map(|expr| quote!(#runtime::__private::value(&::std::format!("{}", &(#expr)))))
                .collect()
        };
        initialize = quote!(#private: ::std::format!(#format, #(#values),*),);
    }
    Ok(quote! {{
        #guard
        // Track the artifact read during expansion for Cargo/rustc incremental builds.
        #manifest_dependency
        #[allow(dead_code, non_snake_case)]
        #[derive(#derives)]
        struct __TopcoatCssModule {
            #(pub #fields: &'static str,)*
            #storage
        }
        impl __TopcoatCssModule {
            /// Generated CSS variables to apply with `style=(styles.style())`.
            #[allow(dead_code)]
            pub fn style(&self) -> &str { #style }
        }
        __TopcoatCssModule { #(#fields: #values,)* #initialize }
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
    editor_expansion(input.into())
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
    if span.local_file().is_none() {
        return stylesheet_editor(input);
    }
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
            variables: Vec::new(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn editor_expansion_uses_current_input_and_guards_placeholder_values() {
        for input in [
            quote!(.new-card { color: blue; }),
            quote!(".new-card { color: blue; }"),
        ] {
            let expansion = editor_expansion(input).unwrap().to_string();
            assert!(expansion.contains("pub new_card"), "{expansion}");
            assert!(expansion.contains("new_card : \"\""), "{expansion}");
            assert!(
                expansion.contains("editor placeholders cannot be compiled"),
                "{expansion}"
            );
            assert!(!expansion.contains("TOPCOAT_CSS_MANIFEST"), "{expansion}");
        }
        assert!(editor_expansion(quote!()).is_ok());
        assert!(
            editor_expansion(quote!(.card { color red; }))
                .unwrap_err()
                .contains("invalid CSS")
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
                let expansion = module_expansion(&module, editor, Vec::new()).unwrap();
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

    #[test]
    fn editor_interpolation_keeps_fields_styles_and_rust_expressions() {
        let expansion = editor_expansion(quote!(
            .header { --bg: $(bg.as_str()); width: calc($(width) * 1px); }
            .style { color: red; }
        ))
        .unwrap()
        .to_string();
        assert!(expansion.contains("pub header"), "{expansion}");
        assert!(expansion.contains("pub style"), "{expansion}");
        assert!(expansion.contains("fn style"), "{expansion}");
        assert!(expansion.contains("bg . as_str"), "{expansion}");
        assert!(expansion.contains("width"), "{expansion}");
        assert!(!expansion.contains("TOPCOAT_CSS_MANIFEST"), "{expansion}");
    }
}
