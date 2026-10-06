use crate::error::{AppError, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn sanitize_component(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect();
    while out.ends_with(' ') || out.ends_with('.') {
        out.pop();
    }
    if out.is_empty() {
        out.push('_');
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    out
}

pub fn folder_rel_path(imap_name: &str, delimiter: Option<&str>) -> String {
    let decoded = utf7_imap::decode_utf7_imap(imap_name.to_string());
    let parts: Vec<&str> = match delimiter {
        Some(d) if !d.is_empty() => decoded.split(d).collect(),
        _ => vec![decoded.as_str()],
    };
    parts.iter().map(|p| sanitize_component(p)).collect::<Vec<_>>().join("/")
}

pub fn unique_path(candidate: &str, taken: &HashSet<String>) -> String {
    let mut path = candidate.to_string();
    let mut n = 2;
    while taken.contains(&path.to_lowercase()) {
        path = format!("{candidate}~{n}");
        n += 1;
    }
    path
}

pub fn eml_name(uidvalidity: u32, uid: u32) -> String {
    format!("{uidvalidity}-{uid:05}.eml")
}

pub fn uid_from_name(name: &str) -> Option<u32> {
    name.strip_suffix(".eml")?.split_once('-')?.1.parse().ok()
}

pub fn is_safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && !rel.contains(':')
        && rel.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && c == sanitize_component(c))
}

pub fn join_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf> {
    if !is_safe_rel(rel) {
        return Err(AppError::UnsafePath(rel.to_string()));
    }
    let full = join_rel(root, rel);
    if !fs::canonicalize(&full)?.starts_with(fs::canonicalize(root)?) {
        return Err(AppError::UnsafePath(rel.to_string()));
    }
    Ok(full)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn walk_files(root: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    if !root.exists() {
        return Ok(out);
    }
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if entry.file_type()?.is_dir() {
                stack.push((entry.path(), r));
            } else {
                out.push((entry.path(), r));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

pub fn remove_tmp_files(root: &Path) -> Result<()> {
    for (path, rel) in walk_files(root)? {
        if rel.ends_with(".tmp") {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_component_rules() {
        assert_eq!(sanitize_component("a<b>c:d"), "a_b_c_d");
        assert_eq!(sanitize_component("nome. "), "nome");
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("con.txt"), "_con.txt");
        assert_eq!(sanitize_component(".."), "_");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component("tab\there"), "tab_here");
    }

    #[test]
    fn folder_rel_path_decodes_and_splits() {
        assert_eq!(folder_rel_path("INBOX.Clientes.Projeto X", Some(".")), "INBOX/Clientes/Projeto X");
        assert_eq!(folder_rel_path("Itens Enviados", Some("/")), "Itens Enviados");
        assert_eq!(folder_rel_path("Rascunhos &AOk-", Some(".")), "Rascunhos é");
        assert_eq!(folder_rel_path("A/B", None), "A_B");
    }

    #[test]
    fn unique_path_is_case_insensitive() {
        let mut taken = HashSet::new();
        assert_eq!(unique_path("INBOX", &taken), "INBOX");
        taken.insert("inbox".to_string());
        assert_eq!(unique_path("Inbox", &taken), "Inbox~2");
        taken.insert("inbox~2".to_string());
        assert_eq!(unique_path("INBOX", &taken), "INBOX~3");
    }

    #[test]
    fn eml_names() {
        assert_eq!(eml_name(4521, 42), "4521-00042.eml");
        assert_eq!(eml_name(1, 123456), "1-123456.eml");
        assert_eq!(uid_from_name("4521-00042.eml"), Some(42));
        assert_eq!(uid_from_name("lixo.eml"), None);
    }

    #[test]
    fn safe_rel_rules() {
        assert!(is_safe_rel("INBOX/1-00001.eml"));
        assert!(is_safe_rel("a@b.com~2/mail/Itens Enviados/Ação/7-00002.eml"));
        assert!(is_safe_rel("a@b.com/account.json"));
        for bad in [
            "", "../x", "a/../b", "/abs", "C:/x", "a\\b", "a//b", "./a", ".. /x", "a/.../b", "a/x.",
            "a/CON", "a/nul.eml", "a\u{0}b",
        ] {
            assert!(!is_safe_rel(bad), "{bad} deveria ser inseguro");
        }
    }

    #[test]
    fn resolve_rejects_escape_and_accepts_inside() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mail");
        write_atomic(&root.join("INBOX").join("1-00001.eml"), b"x").unwrap();
        std::fs::write(tmp.path().join("fora.eml"), b"y").unwrap();
        assert!(resolve(&root, "INBOX/1-00001.eml").is_ok());
        assert!(matches!(resolve(&root, "../fora.eml"), Err(AppError::UnsafePath(_))));
    }

    #[test]
    fn resolve_rejects_junction_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("mail");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("f.eml"), b"y").unwrap();
        let link = root.join("link");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("junction nao criada; teste ignorado");
            return;
        }
        assert!(matches!(resolve(&root, "link/f.eml"), Err(AppError::UnsafePath(_))));
    }

    #[test]
    fn write_atomic_overwrites_and_leaves_no_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a").join("b.eml");
        write_atomic(&p, b"1").unwrap();
        write_atomic(&p, b"2").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"2");
        let files = walk_files(tmp.path()).unwrap();
        assert_eq!(files.iter().map(|f| f.1.as_str()).collect::<Vec<_>>(), vec!["a/b.eml"]);
    }

    #[test]
    fn walk_and_remove_tmp() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("X/Y")).unwrap();
        std::fs::write(tmp.path().join("X/Y/2.eml"), b"").unwrap();
        std::fs::write(tmp.path().join("X/1.eml"), b"").unwrap();
        std::fs::write(tmp.path().join("X/3.eml.tmp"), b"").unwrap();
        remove_tmp_files(tmp.path()).unwrap();
        let rels: Vec<String> = walk_files(tmp.path()).unwrap().into_iter().map(|f| f.1).collect();
        assert_eq!(rels, vec!["X/1.eml", "X/Y/2.eml"]);
        assert!(walk_files(&tmp.path().join("nao-existe")).unwrap().is_empty());
    }
}
