//! Extract Rust expressions without interpreting CSS strings or Rust expressions as CSS.

use proc_macro2::{Delimiter, Group, Span, TokenStream, TokenTree};

pub struct Interpolation {
    pub expression: syn::Expr,
    pub start: Span,
    pub end: Span,
}

/// Temporary variable names, replaced with module-specific names by the compiler.
pub fn placeholder(index: usize) -> String {
    format!("--topcoat-css-interpolation-{index}")
}

pub fn extract(input: TokenStream) -> Result<(TokenStream, Vec<Interpolation>), String> {
    let mut expressions = Vec::new();
    let tokens = visit(input, &mut expressions)?;
    Ok((tokens, expressions))
}

fn visit(input: TokenStream, expressions: &mut Vec<Interpolation>) -> Result<TokenStream, String> {
    let mut input = input.into_iter().peekable();
    let mut output = TokenStream::new();
    while let Some(token) = input.next() {
        if let TokenTree::Punct(dollar) = &token
            && dollar.as_char() == '$'
            && !matches!(input.peek(), Some(TokenTree::Punct(p)) if p.as_char() == '=')
        {
            let Some(TokenTree::Group(group)) = input.next() else {
                return Err("CSS interpolation requires a Rust expression in $(...)".into());
            };
            if group.delimiter() != Delimiter::Parenthesis {
                return Err("CSS interpolation requires a Rust expression in $(...)".into());
            }
            let expression = syn::parse2(group.stream())
                .map_err(|e| format!("invalid Rust expression in CSS interpolation: {e}"))?;
            let replacement = format!("var({})", placeholder(expressions.len()));
            expressions.push(Interpolation {
                expression,
                start: dollar.span(),
                end: group.span_close(),
            });
            output.extend(replacement.parse::<TokenStream>().expect("variable tokens"));
        } else if let TokenTree::Group(group) = token {
            let mut replacement =
                Group::new(group.delimiter(), visit(group.stream(), expressions)?);
            replacement.set_span(group.span());
            output.extend([TokenTree::Group(replacement)]);
        } else {
            output.extend([token]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_expressions_in_order_and_leaves_strings_alone() {
        let input = r#".card[data-label$="suffix"] { content: "$(literal)"; color: $(color.as_str()); width: calc($(width) * 1px); --x: $({ let x = (1, 2); x.0 }); }"#;
        let (tokens, expressions) = extract(input.parse().unwrap()).unwrap();
        assert_eq!(expressions.len(), 3);
        assert!(tokens.to_string().contains("$(literal)"));
        assert!(!tokens.to_string().contains("color . as_str"));
    }

    #[test]
    fn rejects_missing_or_invalid_expressions() {
        for input in [
            ".a { color: $x; }",
            ".a { color: $[x]; }",
            ".a { color: $(); }",
            ".a { color: $(a b); }",
        ] {
            assert!(extract(input.parse().unwrap()).is_err(), "{input}");
        }
    }
}
