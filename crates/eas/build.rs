use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct PageSpec {
    page: u8,
    namespace: String,
    xmlns: String,
    #[serde(default)]
    tags: Vec<TagSpec>,
}

#[derive(Deserialize)]
struct TagSpec {
    token: u8,
    name: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = PathBuf::from(required_environment("CARGO_MANIFEST_DIR")?);
    let spec_dir = manifest.join("../../spec/codepages");
    println!("cargo:rerun-if-changed={}", spec_dir.display());
    let mut paths = read_paths(&spec_dir)?;
    paths.sort();

    let mut pages = paths.iter().map(|path| read_page(path)).collect::<Result<Vec<_>, _>>()?;
    pages.sort_by_key(|page| page.page);
    validate(&pages)?;

    let mut generated = String::from("pub static CODE_PAGES: &[CodePage] = &[\n");
    for page in pages {
        writeln!(
            generated,
            "    CodePage {{ id: {}, namespace: {:?}, xmlns: {:?}, tags: &[",
            page.page, page.namespace, page.xmlns
        )?;
        for tag in page.tags {
            writeln!(generated, "        ({}, {:?}),", tag.token, tag.name)?;
        }
        generated.push_str("    ] },\n");
    }
    generated.push_str("];\n");

    let output = PathBuf::from(required_environment("OUT_DIR")?).join("codepages.rs");
    fs::write(output, generated)?;
    Ok(())
}

fn read_paths(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|value| value.path()))
        .collect::<io::Result<Vec<_>>>()?;
    Ok(paths
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
        .collect())
}

fn read_page(path: &Path) -> Result<PageSpec, Box<dyn Error>> {
    let input = fs::read_to_string(path)?;
    toml::from_str(&input).map_err(Into::into)
}

fn validate(pages: &[PageSpec]) -> io::Result<()> {
    if pages.len() != 25 {
        return Err(io::Error::other("all 25 MS-ASWBXML code pages are required"));
    }
    for (expected, page) in pages.iter().enumerate() {
        if usize::from(page.page) != expected {
            return Err(io::Error::other("code pages must be contiguous"));
        }
        let mut tokens = page.tags.iter().map(|tag| tag.token).collect::<Vec<_>>();
        tokens.sort_unstable();
        tokens.dedup();
        if tokens.len() != page.tags.len() {
            return Err(io::Error::other(format!("duplicate token on page {}", page.page)));
        }
        let mut names = page.tags.iter().map(|tag| tag.name.as_str()).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        if names.len() != page.tags.len() {
            return Err(io::Error::other(format!("duplicate tag on page {}", page.page)));
        }
    }
    Ok(())
}

fn required_environment(name: &str) -> io::Result<std::ffi::OsString> {
    env::var_os(name).ok_or_else(|| io::Error::other(format!("{name} is not set")))
}
