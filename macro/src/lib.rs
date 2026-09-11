use std::{env, fs};

use proc_macro::{Span, TokenStream};
use quote::quote;
use topcoat_css_core::{MANIFEST_ENV, MANIFEST_VERSION, Manifest, STYLESHEET_ENV, fingerprint};

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
    let file = span
        .local_file()
        .ok_or("css! must appear directly in a scanned Rust source file")?;
    let file =
        fs::canonicalize(&file).map_err(|e| format!("cannot locate {}: {e}", file.display()))?;
    let tokens = fingerprint(input.into());
    let module = manifest.modules.iter().find(|module| {
        std::path::Path::new(&module.file) == file
            && module.line == span.line()
            && module.column == span.column()
            && module.fingerprint == tokens
    }).ok_or_else(|| format!(
        "css! at {}:{}:{} was not collected by the build script; add its source directory with BuildConfig::source(...), register a renamed macro with macro_name(...), and ensure the build script reruns when sources change. Macro-generated CSS is not supported",
        span.file(), span.line(), span.column()
    ))?;
    let fields = module
        .fields
        .keys()
        .map(|name| syn::parse_str::<syn::Ident>(name))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("invalid generated CSS field: {e}"))?;
    let values: Vec<_> = module.fields.values().collect();
    Ok(quote! {{
        // Track the artifact read during expansion for Cargo/rustc incremental builds.
        const _: &str = include_str!(env!("TOPCOAT_CSS_MANIFEST"));
        #[allow(dead_code, non_snake_case)]
        #[derive(Clone, Copy, Debug)]
        struct __TopcoatCssModule {
            #(pub #fields: &'static str,)*
        }
        __TopcoatCssModule { #(#fields: #values,)* }
    }}
    .into())
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
