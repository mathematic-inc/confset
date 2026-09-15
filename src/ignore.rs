//! The project-owned Git ignore block, preserving all text outside it.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

const BEGIN: &str = "# BEGIN CONFSET GENERATED FILES";
const END: &str = "# END CONFSET GENERATED FILES";

pub(crate) fn enabled(bytes: Option<&[u8]>) -> Result<bool> {
    let Some(bytes) = bytes else { return Ok(false) };
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_)
            if !bytes
                .windows(BEGIN.len())
                .any(|window| window == BEGIN.as_bytes()) =>
        {
            return Ok(false);
        }
        Err(error) => {
            return Err(error).context(".gitignore must be UTF-8 to update its managed block");
        }
    };
    Ok(block(text)?.is_some())
}

pub(crate) fn update(root: &Path, paths: &[PathBuf], original: Option<&[u8]>) -> Result<Vec<u8>> {
    let original = original.unwrap_or_default();
    let text = std::str::from_utf8(original)
        .context(".gitignore must be UTF-8 to update its managed block")?;
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut patterns = BTreeSet::from(["/.confset/".to_owned(), ".confset-stage-*".to_owned()]);
    for path in paths {
        match path.strip_prefix(root) {
            Ok(relative) => {
                patterns.insert(pattern(relative)?);
            }
            Err(_) => eprintln!(
                "confset: {} is outside this project's .gitignore scope",
                path.display()
            ),
        }
    }
    let mut managed = format!("{BEGIN}{newline}");
    for pattern in patterns {
        managed.push_str(&pattern);
        managed.push_str(newline);
    }
    managed.push_str(END);
    managed.push_str(newline);
    let output = match block(text)? {
        Some((start, end)) => {
            let mut output = text[..start].to_owned();
            output.push_str(&managed);
            output.push_str(&text[end..]);
            output
        }
        None => {
            let mut output = text.to_owned();
            if !output.is_empty() && !output.ends_with('\n') {
                output.push_str(newline);
            }
            if !output.is_empty() && !output.ends_with(&format!("{newline}{newline}")) {
                output.push_str(newline);
            }
            output.push_str(&managed);
            output
        }
    };
    Ok(output.into_bytes())
}

fn block(text: &str) -> Result<Option<(usize, usize)>> {
    let mut begin = None;
    let mut end = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        match trimmed {
            BEGIN if begin.is_none() && end.is_none() => begin = Some(offset),
            END if begin.is_some() && end.is_none() => end = Some(offset + line.len()),
            BEGIN | END => bail!("ambiguous Confset markers in .gitignore; fix the marked block"),
            _ => {}
        }
        offset += line.len();
    }
    match (begin, end) {
        (None, None) => Ok(None),
        (Some(start), Some(end)) => Ok(Some((start, end))),
        _ => bail!("incomplete Confset block in .gitignore; restore both markers"),
    }
}

fn pattern(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(name) = component else {
            bail!("ignore paths must stay inside the project")
        };
        let name = name.to_str().context("ignore paths must be UTF-8")?;
        if name.contains(['\n', '\r', '\0']) {
            bail!("Git cannot ignore a path containing line breaks or NUL");
        }
        let mut escaped = String::new();
        for ch in name.chars() {
            if matches!(ch, '\\' | '*' | '?' | '[' | ']' | '#' | '!' | ' ') {
                escaped.push('\\');
            }
            escaped.push(ch);
        }
        parts.push(escaped);
    }
    if parts.is_empty() {
        bail!("cannot ignore the project root");
    }
    Ok(format!("/{}", parts.join("/")))
}
