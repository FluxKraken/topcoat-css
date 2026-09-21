//! Type information from the current editor buffer, independent of build output.
//! Reconstructed token bodies are used ONLY to discover fields. The build helper
//! still reads the original source to preserve whitespace and generate real CSS.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use topcoat_css_core::{Module, compiler};

pub(crate) fn module(input: TokenStream) -> Result<Module, String> {
    let css = match syn::parse2::<syn::LitStr>(input.clone()) {
        Ok(literal) => literal.value(),
        Err(_) => css_tokens(input),
    };
    // An empty body is common while typing. Cargo continues to reject it.
    let fields = if css.trim().is_empty() {
        Default::default()
    } else {
        compiler::compile(&css, "editor", "editor.rs", 0, false)?.fields
    };
    Ok(Module {
        file: String::new(),
        line: 0,
        column: 0,
        fingerprint: String::new(),
        fields,
    })
}

// TokenStream::to_string inserts spaces in CSS identifiers such as `user-card`
// and selectors such as `.card`. Rejoin CSS punctuation while retaining spaces
// between words. Without source spans, selector whitespace is ambiguous; none
// of the reconstructed text or scoped names may be used for application CSS.
fn css_tokens(input: TokenStream) -> String {
    let mut result = String::new();
    let mut join_next = true;
    for token in input {
        let join_previous = matches!(&token, TokenTree::Punct(p) if p.as_char() == '-')
            || matches!(&token, TokenTree::Group(g) if matches!(g.delimiter(), Delimiter::Parenthesis | Delimiter::Bracket | Delimiter::None));
        if !join_next && !join_previous {
            result.push(' ');
        }
        join_next = matches!(&token, TokenTree::Punct(p) if matches!(p.as_char(), '.' | '#' | '@' | ':' | '-'));
        match token {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                result.push_str(open);
                result.push_str(&css_tokens(group.stream()));
                result.push_str(close);
            }
            token => result.push_str(&token.to_string()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_fields_match_compiled_css_for_tokens_and_literals() {
        for css in [
            ".user-card, .type, #some-id { color: red; }",
            ".card:hover .title { padding: 0.5rem; }",
            ":global(.theme) .card { animation: fade-in 1s ease; } @keyframes fade-in { to { opacity: 1; } }",
            ".card { --foreground: green; &:hover { color: var(--foreground); } }",
            "@media (width < 800px) { @supports (display: grid) { .card { display: grid; } } }",
            ".base { color: red; } .card { composes: base; composes: other from global; }",
            ".grid { display: grid; container-name: panel; }",
            ".card { width: calc(100% - 2rem); margin: -1px; } .card:nth-child(2n + 1) { color: red; }",
            ".card[data-label=\".literal\"] { content: \".literal\"; background: url(\"https://example.com/a.png\"); }",
        ] {
            let expected = compiler::compile(css, "test", "src/main.rs", 0, false)
                .unwrap_or_else(|e| panic!("{css}: {e}"))
                .fields;
            for input in [css.parse().unwrap(), quote::quote!(#css)] {
                let actual = module(input).unwrap_or_else(|e| panic!("{css}: {e}"));
                assert!(
                    actual.fields.keys().eq(expected.keys()),
                    "{css}: {:?}",
                    actual.fields
                );
            }
        }
    }

    #[test]
    fn empty_edits_and_real_css_errors_do_not_need_a_manifest() {
        assert!(module(TokenStream::new()).unwrap().fields.is_empty());
        assert!(module(quote::quote!("")).unwrap().fields.is_empty());
        let error = module(quote::quote!(.card { color red; })).unwrap_err();
        assert!(error.contains("invalid CSS"), "{error}");
        let error = module(quote::quote!(.user-card, .user_card { color: red; })).unwrap_err();
        assert!(error.contains("both produce Rust field"), "{error}");
    }
}
