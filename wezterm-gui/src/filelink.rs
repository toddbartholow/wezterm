//! Opens links to files in an editor.
//!
//! File links come from two places: `file://` URLs in OSC 8 hyperlinks,
//! and the `file:` links that the default hyperlink rules make from text
//! such as `src/main.rs:12:5`. The latter hold the path exactly as it was
//! printed, so relative paths are resolved against the pane's working
//! directory when the link is clicked.
use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use url::Url;

#[derive(Debug, PartialEq, Eq)]
pub struct FileLink {
    pub path: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Returns true if `uri` holds a path matched from terminal text, rather
/// than a URL. These can't be passed to the system's URL opener.
pub fn is_text_path_link(uri: &str) -> bool {
    uri.starts_with("file:") && !uri.starts_with("file://")
}

/// Turns `uri` into the location of an existing file or directory, or
/// returns None if it isn't a file link or nothing exists at that path.
/// `cwd` is the pane's working directory, used for relative paths.
pub fn resolve_file_link(
    uri: &str,
    cwd: Option<&Path>,
    home: &Path,
    exists: impl Fn(&Path) -> bool,
) -> Option<FileLink> {
    if uri.starts_with("file://") {
        let url = Url::parse(uri).ok()?;
        let (line, column) = url.fragment().map(parse_fragment).unwrap_or_default();
        let path = local_file_path(&url)?;
        return exists(&path).then_some(FileLink { path, line, column });
    }

    let text = uri.strip_prefix("file:")?;
    let (text, line, column) = split_line_column(text);

    let candidates: Vec<PathBuf> =
        if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
            vec![home.join(rest)]
        } else if Path::new(text).is_absolute() || text.starts_with('/') || text.starts_with('\\') {
            vec![PathBuf::from(text)]
        } else {
            let cwd = cwd?;
            let mut candidates = vec![cwd.join(text)];
            // `git diff` prefixes paths with a/ and b/
            if let Some(rest) = text.strip_prefix("a/").or_else(|| text.strip_prefix("b/")) {
                candidates.push(cwd.join(rest));
            }
            candidates
        };

    candidates
        .into_iter()
        .find(|path| exists(path))
        .map(|path| FileLink { path, line, column })
}

/// Converts a `file://` URL to a local path, treating a host that names
/// this machine as no host. Returns None for files on other machines.
fn local_file_path(url: &Url) -> Option<PathBuf> {
    let mut url = url.clone();
    if let Some(host) = url.host_str() {
        let is_local = host.eq_ignore_ascii_case("localhost")
            || hostname::get()
                .ok()
                .and_then(|name| name.into_string().ok())
                .map_or(false, |name| host.eq_ignore_ascii_case(&name));
        if !is_local {
            return None;
        }
        url.set_host(None).ok()?;
    }
    url.to_file_path().ok()
}

/// Splits a trailing `:line` or `:line:column` off `text`.
fn split_line_column(text: &str) -> (&str, Option<u32>, Option<u32>) {
    fn split_number(text: &str) -> Option<(&str, u32)> {
        let (head, tail) = text.rsplit_once(':')?;
        if head.is_empty() || tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((head, tail.parse().ok()?))
    }

    match split_number(text) {
        None => (text, None, None),
        Some((head, last)) => match split_number(head) {
            Some((path, line)) => (path, Some(line), Some(last)),
            None => (head, Some(last), None),
        },
    }
}

/// Parses a line and column from a URL fragment in the forms `L12`,
/// `L12C5`, `12` and `12:5`. Anything after them, such as the `-L20` of a
/// line range, is ignored.
fn parse_fragment(fragment: &str) -> (Option<u32>, Option<u32>) {
    fn leading_number(text: &str) -> (Option<u32>, &str) {
        let end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        (text[..end].parse().ok(), &text[end..])
    }

    let (line, rest) = leading_number(fragment.strip_prefix('L').unwrap_or(fragment));
    if line.is_none() {
        return (None, None);
    }
    let column = rest
        .strip_prefix('C')
        .or_else(|| rest.strip_prefix(':'))
        .and_then(|rest| leading_number(rest).0);
    (line, column)
}

/// Opens `link` with `editor`, a command whose arguments may contain
/// `{path}`, `{line}` and `{column}`. Without an editor, or for a
/// directory, it is opened with the system's default application.
pub fn open_file_link(link: &FileLink, editor: Option<&[String]>) {
    let editor = editor.filter(|_| !link.path.is_dir());
    let result = match editor {
        Some(editor) => spawn_editor(editor, link),
        None => Url::from_file_path(&link.path)
            .map(|url| wezterm_open_url::open_url(url.as_str()))
            .map_err(|()| anyhow::anyhow!("cannot make a URL from {}", link.path.display())),
    };
    if let Err(err) = result {
        log::error!("failed to open {}: {:#}", link.path.display(), err);
    }
}

fn editor_args(editor: &[String], link: &FileLink) -> Vec<String> {
    let path = link.path.to_string_lossy();
    let line = link.line.unwrap_or(1).to_string();
    let column = link.column.unwrap_or(1).to_string();
    editor
        .iter()
        .map(|arg| {
            arg.replace("{line}", &line)
                .replace("{column}", &column)
                .replace("{path}", &path)
        })
        .collect()
}

fn spawn_editor(editor: &[String], link: &FileLink) -> anyhow::Result<()> {
    let args = editor_args(editor, link);
    let (program, args) = args.split_first().context("file_link_editor is empty")?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Editors are often batch file wrappers (such as code.cmd), which
        // would otherwise flash a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .with_context(|| format!("running file_link_editor {program}"))?;
    // Reap the child when it exits, without blocking the GUI
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    fn resolve(uri: &str, existing: &[&str]) -> Option<FileLink> {
        let cwd = PathBuf::from(if cfg!(windows) { r"D:\proj" } else { "/proj" });
        let home = PathBuf::from(if cfg!(windows) {
            r"C:\Users\me"
        } else {
            "/home/me"
        });
        let existing: Vec<PathBuf> = existing.iter().map(PathBuf::from).collect();
        resolve_file_link(uri, Some(&cwd), &home, |path| {
            existing.iter().any(|e| e == path)
        })
    }

    fn link(path: &str, line: Option<u32>, column: Option<u32>) -> Option<FileLink> {
        Some(FileLink {
            path: PathBuf::from(path),
            line,
            column,
        })
    }

    #[test]
    fn line_and_column() {
        assert_eq!(split_line_column("a.rs"), ("a.rs", None, None));
        assert_eq!(split_line_column("a.rs:12"), ("a.rs", Some(12), None));
        assert_eq!(split_line_column("a.rs:12:5"), ("a.rs", Some(12), Some(5)));
        assert_eq!(
            split_line_column(r"D:\x\a.rs:12"),
            (r"D:\x\a.rs", Some(12), None)
        );
        assert_eq!(parse_fragment("L12"), (Some(12), None));
        assert_eq!(parse_fragment("L12C5"), (Some(12), Some(5)));
        assert_eq!(parse_fragment("L12-L20"), (Some(12), None));
        assert_eq!(parse_fragment("12:5"), (Some(12), Some(5)));
        assert_eq!(parse_fragment("section"), (None, None));
    }

    #[test]
    fn not_file_links() {
        assert_eq!(resolve("https://example.com/a.rs", &[]), None);
        assert_eq!(resolve("mailto:me@example.com", &[]), None);
        assert!(!is_text_path_link("file:///etc/hosts"));
        assert!(is_text_path_link("file:src/main.rs"));
    }

    #[cfg(windows)]
    #[test]
    fn resolve_windows() {
        assert_eq!(
            resolve(r"file:src\main.rs:12:5", &[r"D:\proj\src\main.rs"]),
            link(r"D:\proj\src\main.rs", Some(12), Some(5))
        );
        assert_eq!(
            resolve("file:src/main.rs:12", &[r"D:\proj\src/main.rs"]),
            link(r"D:\proj\src/main.rs", Some(12), None)
        );
        assert_eq!(
            resolve("file:b/src/main.rs", &[r"D:\proj\src/main.rs"]),
            link(r"D:\proj\src/main.rs", None, None)
        );
        assert_eq!(
            resolve(r"file:E:\other\a.go:3", &[r"E:\other\a.go"]),
            link(r"E:\other\a.go", Some(3), None)
        );
        assert_eq!(
            resolve("file:~/.config/x.lua", &[r"C:\Users\me\.config/x.lua"]),
            link(r"C:\Users\me\.config/x.lua", None, None)
        );
        assert_eq!(
            resolve("file:///D:/proj/a.rs#L7", &[r"D:\proj\a.rs"]),
            link(r"D:\proj\a.rs", Some(7), None)
        );
        assert_eq!(
            resolve("file://localhost/D:/proj/a.rs", &[r"D:\proj\a.rs"]),
            link(r"D:\proj\a.rs", None, None)
        );
        // Files on other machines are left to the system's URL handler
        assert_eq!(
            resolve("file://elsewhere/share/a.rs", &[r"\\elsewhere\share\a.rs"]),
            None
        );
        assert_eq!(resolve("file:src/missing.rs:1", &[]), None);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_unix() {
        assert_eq!(
            resolve("file:src/main.rs:12:5", &["/proj/src/main.rs"]),
            link("/proj/src/main.rs", Some(12), Some(5))
        );
        assert_eq!(
            resolve("file:a/src/main.rs", &["/proj/src/main.rs"]),
            link("/proj/src/main.rs", None, None)
        );
        assert_eq!(
            resolve("file:~/.config/x.lua", &["/home/me/.config/x.lua"]),
            link("/home/me/.config/x.lua", None, None)
        );
        assert_eq!(
            resolve("file:///proj/a.rs#L7C2", &["/proj/a.rs"]),
            link("/proj/a.rs", Some(7), Some(2))
        );
        assert_eq!(resolve("file:src/missing.rs:1", &[]), None);
    }

    #[test]
    fn substitutes_editor_args() {
        let link = FileLink {
            path: PathBuf::from("main.rs"),
            line: Some(12),
            column: None,
        };
        let editor = vec![
            "code".to_string(),
            "--goto".to_string(),
            "{path}:{line}:{column}".to_string(),
        ];
        assert_eq!(
            editor_args(&editor, &link),
            vec!["code", "--goto", "main.rs:12:1"]
        );
    }
}
