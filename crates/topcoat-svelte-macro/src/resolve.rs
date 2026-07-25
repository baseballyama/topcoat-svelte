//! Path resolution, component naming, and content hashing for the `svelte!`
//! macro.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Resolves the macro argument to an existing absolute path, mirroring
/// `asset!`'s resolution rules:
///
/// - `./` and `../` resolve against the calling source file's directory,
/// - an absolute path is used as-is,
/// - any other relative path resolves against `CARGO_MANIFEST_DIR`.
pub(crate) fn resolve_arg(arg: &str) -> Result<PathBuf, String> {
    let joined = if arg.starts_with("./") || arg.starts_with("../") {
        source_dir()?.join(arg)
    } else {
        let path = Path::new(arg);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            manifest_dir()?.join(arg)
        }
    };
    canonicalize(&joined)
}

/// Resolves a relative import `specifier` (always `./` or `../`) against the
/// directory of the file that imported it, returning an existing absolute path.
pub(crate) fn resolve_import(importer: &Path, specifier: &str) -> Result<PathBuf, String> {
    let base = importer
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", importer.display()))?;
    canonicalize(&base.join(specifier))
}

/// Canonicalizes `path`, which also serves as its existence check.
fn canonicalize(path: &Path) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|err| format!("could not find {}: {err}", path.display()))
}

/// The directory of the source file that invoked the macro.
fn source_dir() -> Result<PathBuf, String> {
    let file = proc_macro::Span::call_site()
        .local_file()
        .ok_or("could not determine the calling source file")?;
    let file = if file.is_absolute() {
        file
    } else {
        std::env::current_dir()
            .map_err(|err| format!("could not read the current directory: {err}"))?
            .join(file)
    };
    file.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the calling source file has no parent directory".to_owned())
}

fn manifest_dir() -> Result<PathBuf, String> {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map_err(|_| "CARGO_MANIFEST_DIR is not set".to_owned())
}

/// Derives a PascalCase component name from a file stem, so `counter.svelte`
/// and `my-widget.svelte` become `Counter` and `MyWidget`.
///
/// The derived name must be ASCII: a non-ASCII name would produce a
/// `module_url` that does not match the percent-encoded path browsers send,
/// causing the served module to 404. Rename the file to ASCII instead.
pub(crate) fn component_name(path: &Path) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("could not read a file name from {}", path.display()))?;

    let mut name = String::with_capacity(stem.len());
    let mut capitalize_next = true;
    for ch in stem.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                name.extend(ch.to_uppercase());
            } else {
                name.push(ch);
            }
            capitalize_next = false;
        } else {
            capitalize_next = true;
        }
    }

    if name.is_empty() || name.starts_with(|c: char| c.is_numeric()) {
        return Err(format!(
            "{} does not yield a valid component name",
            path.display()
        ));
    }
    if !name.is_ascii() {
        return Err(format!(
            "{} yields a non-ASCII component name ({name:?}); rename the file to \
             use only ASCII characters",
            path.display()
        ));
    }
    Ok(name)
}

/// The first 16 hex characters of the SHA-256 of `js`.
pub(crate) fn short_hash(js: &str) -> String {
    let digest = Sha256::digest(js.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{component_name, short_hash};
    use std::path::Path;

    #[test]
    fn pascal_cases_simple_stems() {
        assert_eq!(
            component_name(Path::new("counter.svelte")).unwrap(),
            "Counter"
        );
        assert_eq!(
            component_name(Path::new("my-widget.svelte")).unwrap(),
            "MyWidget"
        );
        assert_eq!(
            component_name(Path::new("my_widget.svelte")).unwrap(),
            "MyWidget"
        );
    }

    #[test]
    fn rejects_names_starting_with_a_digit() {
        assert!(component_name(Path::new("42widget.svelte")).is_err());
    }

    #[test]
    fn rejects_empty_names() {
        assert!(component_name(Path::new("---.svelte")).is_err());
    }

    #[test]
    fn rejects_non_ascii_stems() {
        let err = component_name(Path::new("café.svelte")).unwrap_err();
        assert!(
            err.contains("non-ASCII"),
            "expected a non-ASCII error, got: {err}"
        );

        let err = component_name(Path::new("ボタン.svelte")).unwrap_err();
        assert!(
            err.contains("non-ASCII"),
            "expected a non-ASCII error, got: {err}"
        );
    }

    #[test]
    fn short_hash_is_16_hex_chars() {
        let hash = short_hash("export default function C() {}");
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
