use crate::{BuildError, Result, io_error};
use proc_macro2::{Group, LineColumn, TokenStream, TokenTree};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
use topcoat_css_core::fingerprint;

pub(crate) struct Input {
    pub css: String,
    pub fingerprint: String,
    pub line: usize,
    pub column: usize,
}

pub(crate) fn collect_files(path: &Path, files: &mut BTreeSet<PathBuf>) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
    // Do not follow directory symlinks into target/ or into recursive loops.
    if metadata.file_type().is_symlink() {
        return Err(BuildError(format!(
            "symlinked source {} is unsupported; use a real source path",
            path.display()
        )));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| io_error(path, e))? {
            let entry = entry.map_err(|e| io_error(path, e))?;
            collect_files(&entry.path(), files)?;
        }
    } else if path.extension().is_some_and(|extension| extension == "rs") {
        files.insert(path.to_path_buf());
    }
    Ok(())
}

pub(crate) fn scan(
    source: &str,
    names: &BTreeSet<String>,
) -> std::result::Result<Vec<Input>, String> {
    // Neutralize Rust's optional BOM/shebang without changing character positions.
    let mut lex_source = source.to_owned();
    if lex_source.starts_with('\u{feff}') {
        lex_source.replace_range(..3, " ");
    }
    let start = usize::from(source.starts_with('\u{feff}'));
    if lex_source[start..].starts_with("#!") && !lex_source[start..].starts_with("#![") {
        let end = lex_source.find('\n').unwrap_or(lex_source.len());
        let spaces = " ".repeat(lex_source[start..end].chars().count());
        lex_source.replace_range(start..end, &spaces);
    }
    let tokens = lex_source.parse::<TokenStream>().map_err(|e| format!(
        "cannot tokenize Rust source: {e}; for CSS that is not valid Rust tokens, use css!(r#\"...\"#)"
    ))?;
    let mut inputs = Vec::new();
    visit(tokens, source, names, &mut inputs)?;
    Ok(inputs)
}

fn visit(
    tokens: TokenStream,
    source: &str,
    names: &BTreeSet<String>,
    inputs: &mut Vec<Input>,
) -> std::result::Result<(), String> {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut index = 0;
    while index < tokens.len() {
        if let (
            Some(TokenTree::Ident(name)),
            Some(TokenTree::Punct(bang)),
            Some(TokenTree::Group(group)),
        ) = (
            tokens.get(index),
            tokens.get(index + 1),
            tokens.get(index + 2),
        ) && names.contains(&name.to_string())
            && bang.as_char() == '!'
        {
            inputs.push(extract(group, source)?);
            index += 3;
            continue;
        }
        if let TokenTree::Group(group) = &tokens[index] {
            visit(group.stream(), source, names, inputs)?;
        }
        index += 1;
    }
    Ok(())
}

fn extract(group: &Group, source: &str) -> std::result::Result<Input, String> {
    let tokens = group.stream();
    let first = tokens
        .clone()
        .into_iter()
        .next()
        .ok_or("css! requires a CSS body or a string literal")?;
    let start = first.span().start();
    let css = if let Ok(literal) = syn::parse2::<syn::LitStr>(tokens.clone()) {
        literal.value()
    } else {
        let from = offset(source, group.span_open().end())?;
        let to = offset(source, group.span_close().start())?;
        source
            .get(from..to)
            .ok_or("CSS source range is invalid")?
            .to_owned()
    };
    Ok(Input {
        css,
        fingerprint: fingerprint(tokens),
        line: start.line,
        column: start.column + 1,
    })
}

fn offset(source: &str, position: LineColumn) -> std::result::Result<usize, String> {
    let mut byte = 0;
    for (index, line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == position.line {
            let column = line
                .char_indices()
                .nth(position.column)
                .map(|(byte, _)| byte)
                .or_else(|| (line.chars().count() == position.column).then_some(line.len()))
                .ok_or("source column is out of range")?;
            return Ok(byte + column);
        }
        byte += line.len();
    }
    Err("source line is out of range".into())
}
