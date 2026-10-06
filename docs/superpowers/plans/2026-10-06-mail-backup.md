# Mail Backup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** App desktop Windows (Tauri v2 + Svelte) que faz backup incremental de contas IMAP para `.eml` em árvore de pastas, permite navegar/buscar o backup e exporta/importa backups como `.zip` (AES-256 opcional).

**Architecture:** Backend Rust em módulos pequenos (`store`, `index`, `config`, `imap_sync`, `archive`, `message`, `commands`) com os `.eml` como fonte da verdade e um SQLite por conta como cache de cabeçalhos/estado de sync. O frontend Svelte só chama comandos Tauri com IDs e escuta eventos de progresso; o conteúdo do e-mail é exibido em iframe `sandbox` com CSP própria.

**Tech Stack:** Rust (tauri 2.12, async-imap 0.12 runtime-tokio, tokio-rustls 0.26 com ring, mail-parser 0.11, rusqlite 0.40 bundled, keyring 4, zip 8 com aes-crypto, utf7-imap 0.3, thiserror 2, uuid, base64), Svelte 5 + Vite 8 (JavaScript, sem TypeScript), GreenMail 1.6.15 standalone (testes de integração).

**Spec:** `docs/superpowers/specs/2026-10-06-mail-backup-design.md`

## Global Constraints

- Plataforma alvo: Windows 10/11 (WebView2). Toolchain Rust **MSVC** (`x86_64-pc-windows-msvc`).
- Pasta de dados: `%APPDATA%/mail-backup/` (`config.json` + `accounts/<id>/`). Pasta da conta alterável no cadastro.
- Senhas: só no Gerenciador de Credenciais (`keyring`, service `mail-backup`, user = id da conta). Nunca em arquivo, log ou mensagem de erro.
- IMAP: TLS na porta 993, `LOGIN` usuário/senha, sempre `EXAMINE` + `BODY.PEEK[]` (nunca marcar como lido no servidor).
- Layout: `<dir>/mail/<pasta>/<uidvalidity>-<uid:05>.eml` (bytes brutos do servidor) + `<dir>/index.sqlite` (`PRAGMA user_version = 1`).
- Gravação de arquivos: sempre `.tmp` + rename.
- Mensagens apagadas/pastas renomeadas no servidor permanecem no backup; flags não são re-sincronizadas.
- O front envia apenas IDs; caminhos são resolvidos no backend e precisam ficar dentro de `<dir>/mail`. Exceção aceita: o caminho do `.zip` escolhido no diálogo volta do front para `import_archive`.
- Iframe do e-mail: `sandbox="allow-popups allow-popups-to-escape-sandbox"`, CSP `default-src 'none'; img-src data:; style-src 'unsafe-inline'` (com "carregar imagens": `img-src data: https:`), `<base target="_blank">`.
- Extensões bloqueadas para "Abrir": `.exe .bat .cmd .com .js .jse .vbs .vbe .wsf .ps1 .msi .scr .lnk .hta .cpl .jar`.
- Export: `.zip` deflate, entradas `<email>/account.json` e `<email>/mail/**.eml`, sem `index.sqlite`, sem senha da conta; AES-256 por entrada quando houver senha.
- Mensagens de UI e de erro em português.
- Commits **sem** linha `Co-Authored-By` (hook do usuário bloqueia).

## Review Focus

1. Duas pastas no servidor que diferem só em maiúsculas/minúsculas (`Clientes` e `clientes`) → o NTFS não diferencia; devem virar diretórios distintos (`clientes~2`) e não misturar mensagens. Teste: Task 1 (`unique_path_is_case_insensitive`) e Task 2 (`folder_for_remote_case_collision`).
2. E-mail com cabeçalhos ausentes/quebrados (sem `From`, sem `Date`, sem `Message-ID`, bytes não-MIME) → salvo e indexado com campos vazios, sem pânico, ordenado no fim da lista. Teste: Task 2 (`record_from_garbage_does_not_panic`, `list_orders_null_dates_last`).
3. Busca com `%`, `_` ou `\` digitados pelo usuário → busca literal, não curinga. Teste: Task 2 (`search_treats_wildcards_literally`).
4. Usuário navegando a conta enquanto o backup grava nela → nenhum erro "database is locked". Teste: Task 2 (`reader_not_blocked_by_open_write_transaction`).
5. Exportar duas contas com o mesmo e-mail / importar arquivo de conta que já existe → nada é sobrescrito; contas distintas. Teste: Task 5 (`export_two_accounts_same_email_keeps_both`) e Task 7 (nome `"<email> (importado)"`).

---

## File Structure

```
mail-backup/
  package.json, vite.config.js, index.html, .gitignore
  src/
    main.js                 # monta App
    App.svelte              # navegação Contas | Navegador + CSS global
    lib/api.js              # wrappers de invoke/listen
    lib/Accounts.svelte     # cadastro, backup, exportar/importar
    lib/Browser.svelte      # pastas + busca + lista + leitor
    lib/MessageList.svelte  # lista com scroll virtual
    lib/Reader.svelte       # cabeçalhos, iframe, anexos
  src-tauri/
    Cargo.toml, build.rs, tauri.conf.json, capabilities/default.json, icons/
    src/main.rs             # chama mail_backup_lib::run()
    src/lib.rs              # módulos, setup, janela, handlers
    src/error.rs            # AppError
    src/store.rs            # caminhos, gravação atômica, varredura
    src/index.rs            # SQLite: esquema, sync state, lista/busca, reindex
    src/config.rs           # config.json + keyring
    src/imap_sync.rs        # conexão, listagem, fetch incremental
    src/archive.rs          # export/import zip
    src/message.rs          # renderização segura + anexos
    src/commands.rs         # comandos Tauri
    tests/greenmail.rs      # integração IMAP (#[ignore])
  tools/greenmail.jar       # baixado, ignorado no git
```

---

### Task 0: Toolchain e esqueleto do projeto

**Files:**
- Create: `package.json`, `vite.config.js`, `index.html`, `src/main.js`, `src/App.svelte`, `.gitignore`
- Create (via `tauri init`): `src-tauri/**`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`
- Create: `src-tauri/src/error.rs`

**Interfaces:**
- Produces: `crate::error::{AppError, Result}`; `AppError::kind() -> &'static str`; `AppError::is_retryable() -> bool`; `AppError` serializa como `{"kind": "...", "message": "..."}`.

- [ ] **Step 1: Toolchain MSVC (uma vez por máquina)**

Hoje a máquina tem Rust do Chocolatey com host `x86_64-pc-windows-gnu` sem `dlltool`, e **não compila** crates com `windows-sys`. Rodar no PowerShell (como usuário; o Build Tools pede elevação):

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
choco uninstall rust -y
winget install --id Rustlang.Rustup
rustup default stable-msvc
```

Abrir um terminal novo e verificar:

```bash
rustc -vV | grep host
```
Expected: `host: x86_64-pc-windows-msvc`

- [ ] **Step 2: Frontend mínimo**

`package.json`:
```json
{
  "name": "mail-backup",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.12.1"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^7.3.1",
    "@tauri-apps/cli": "^2.12.1",
    "svelte": "^5.57.2",
    "vite": "^8.3.3"
  }
}
```

`vite.config.js`:
```js
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
})
```

`index.html`:
```html
<!doctype html>
<html lang="pt-BR">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Mail Backup</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.js"></script>
  </body>
</html>
```

`src/main.js`:
```js
import { mount } from 'svelte'
import App from './App.svelte'

mount(App, { target: document.getElementById('app') })
```

`src/App.svelte` (provisório, substituído na Task 8):
```svelte
<h1>Mail Backup</h1>
```

`.gitignore`:
```
node_modules/
dist/
src-tauri/target/
src-tauri/gen/
tools/
```

Run: `npm install`
Expected: termina sem erros.

- [ ] **Step 3: Gerar `src-tauri`**

```bash
npx tauri init --ci --app-name mail-backup --window-title "Mail Backup" --frontend-dist ../dist --dev-url http://localhost:5173 --before-dev-command "npm run dev" --before-build-command "npm run build"
```
Expected: cria `src-tauri/` com `Cargo.toml`, `build.rs`, `tauri.conf.json`, `capabilities/default.json`, `icons/`, `src/main.rs`, `src/lib.rs`.

- [ ] **Step 4: Dependências e perfil de release**

Substituir as seções `[dependencies]`, `[build-dependencies]` e adicionar as demais em `src-tauri/Cargo.toml` (manter `[package]` e `[lib]` gerados; o `[lib]` deve ter `name = "mail_backup_lib"`):

```toml
[build-dependencies]
tauri-build = { version = "2.7", features = [] }

[dependencies]
tauri = { version = "2.12", features = [] }
tauri-plugin-opener = "2.7"
tauri-plugin-dialog = "2.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["net", "time", "macros", "rt-multi-thread"] }
tokio-rustls = { version = "0.26", default-features = false, features = ["logging", "tls12", "ring"] }
webpki-roots = "1"
async-imap = { version = "0.12", default-features = false, features = ["runtime-tokio"] }
futures = "0.3"
utf7-imap = "0.3"
mail-parser = "0.11"
rusqlite = { version = "0.40", features = ["bundled"] }
keyring = "4"
zip = { version = "8", default-features = false, features = ["deflate", "aes-crypto"] }
uuid = { version = "1", features = ["v4"] }
base64 = "0.22"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "s"
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

Em `src-tauri/tauri.conf.json`, deixar `"app": { "windows": [], "security": { "csp": null } }` (a janela é criada no `setup` na Task 7 para registrar `on_new_window`; a CSP fica no iframe do e-mail). Manter `identifier` gerado.

- [ ] **Step 5: Teste do `AppError`**

Criar `src-tauri/src/error.rs` só com o teste primeiro:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_kind_and_message() {
        let json = serde_json::to_string(&AppError::AuthFailed).unwrap();
        assert_eq!(json, r#"{"kind":"AuthFailed","message":"usuário ou senha inválidos"}"#);
        assert!(AppError::Network("x".into()).is_retryable());
        assert!(!AppError::WrongPassword.is_retryable());
    }
}
```

Substituir `src-tauri/src/lib.rs` por:

```rust
pub mod error;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o aplicativo");
}
```

Run: `cd src-tauri && cargo test error`
Expected: FAIL (compilação: `AppError` não definido).

- [ ] **Step 6: Implementar `error.rs`** (acima do módulo de testes)

```rust
use serde::{ser::SerializeStruct, Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("usuário ou senha inválidos")]
    AuthFailed,
    #[error("falha de rede: {0}")]
    Network(String),
    #[error("falha de TLS: {0}")]
    Tls(String),
    #[error("erro de disco: {0}")]
    Io(#[from] std::io::Error),
    #[error("senha do arquivo incorreta")]
    WrongPassword,
    #[error("arquivo de backup corrompido: {0}")]
    CorruptArchive(String),
    #[error("caminho inseguro rejeitado: {0}")]
    UnsafePath(String),
    #[error("erro no índice: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("erro nas credenciais: {0}")]
    Keyring(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::AuthFailed => "AuthFailed",
            AppError::Network(_) => "Network",
            AppError::Tls(_) => "Tls",
            AppError::Io(_) => "Io",
            AppError::WrongPassword => "WrongPassword",
            AppError::CorruptArchive(_) => "CorruptArchive",
            AppError::UnsafePath(_) => "UnsafePath",
            AppError::Db(_) => "Db",
            AppError::Keyring(_) => "Keyring",
            AppError::Other(_) => "Other",
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, AppError::Network(_) | AppError::Tls(_))
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", self.kind())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
```

Run: `cd src-tauri && cargo test error`
Expected: PASS (1 test).

- [ ] **Step 7: App abre**

Run: `npm run tauri dev`
Expected: compila e abre **nenhuma** janela (lista `windows` vazia) — confirmar no log que o processo subiu sem erro e encerrar com Ctrl+C. (A janela passa a existir na Task 7.)

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "chore: esqueleto Tauri v2 + Svelte e AppError"
```

---

### Task 1: `store.rs` — caminhos seguros e gravação atômica

**Files:**
- Create: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod store;`)

**Interfaces:**
- Consumes: `crate::error::{AppError, Result}`
- Produces:
  - `pub fn sanitize_component(s: &str) -> String`
  - `pub fn folder_rel_path(imap_name: &str, delimiter: Option<&str>) -> String` (componentes unidos por `/`)
  - `pub fn unique_path(candidate: &str, taken: &HashSet<String>) -> String` (`taken` em minúsculas)
  - `pub fn eml_name(uidvalidity: u32, uid: u32) -> String`
  - `pub fn uid_from_name(name: &str) -> Option<u32>`
  - `pub fn is_safe_rel(rel: &str) -> bool`
  - `pub fn join_rel(root: &Path, rel: &str) -> PathBuf`
  - `pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf>`
  - `pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()>`
  - `pub fn walk_files(root: &Path) -> Result<Vec<(PathBuf, String)>>` (ordenado pelo relativo; relativo usa `/`)
  - `pub fn remove_tmp_files(root: &Path) -> Result<()>`

- [ ] **Step 1: Testes**

`src-tauri/src/store.rs`:
```rust
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
        for bad in ["", "../x", "a/../b", "/abs", "C:/x", "a\\b", "a//b", "./a"] {
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
```

Adicionar `pub mod store;` em `lib.rs`.

Run: `cd src-tauri && cargo test store`
Expected: FAIL (funções não definidas).

- [ ] **Step 2: Implementação** (acima dos testes)

```rust
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
        && rel.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
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
```

Run: `cd src-tauri && cargo test store`
Expected: PASS (8 tests).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/store.rs src-tauri/src/lib.rs
git commit -m "feat(store): mapeamento de pastas, caminhos seguros e gravação atômica"
```

---

### Task 2: `index.rs` — SQLite, estado de sync, lista/busca e reindexação

**Files:**
- Create: `src-tauri/src/index.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod index;`)

**Interfaces:**
- Consumes: `store::{walk_files, unique_path, uid_from_name}`, `error::Result`
- Produces:
  - `pub const SCHEMA_VERSION: i64 = 1;`
  - `pub struct FolderState { pub id: i64, pub imap_name: Option<String>, pub path: String, pub uidvalidity: Option<u32>, pub last_uid: u32 }` (Serialize, Deserialize, Clone, PartialEq, Debug)
  - `pub struct FolderInfo { pub id: i64, pub path: String, pub count: i64 }` (Serialize)
  - `pub struct MessageRow { pub id: i64, pub folder_id: i64, pub date: Option<i64>, pub from_addr: Option<String>, pub from_name: Option<String>, pub subject: Option<String>, pub flags: String, pub size: i64, pub has_attach: bool }` (Serialize)
  - `pub struct Query { pub folder_id: Option<i64>, pub text: Option<String>, pub date_from: Option<i64>, pub date_to: Option<i64>, pub offset: i64, pub limit: i64 }` (Deserialize, Default)
  - `pub struct MessagePage { pub rows: Vec<MessageRow>, pub total: i64 }` (Serialize)
  - `pub struct MessageRecord { pub uid: Option<u32>, pub file: String, pub date: Option<i64>, pub from_addr: Option<String>, pub from_name: Option<String>, pub subject: Option<String>, pub message_id: Option<String>, pub flags: String, pub size: i64, pub has_attach: bool }`
  - `MessageRecord::from_raw(raw: &[u8], uid: Option<u32>, file: String, flags: String, internal_date: Option<i64>) -> MessageRecord`
  - `Index::open(path: &Path) -> Result<(Index, bool)>` (bool = precisa reindexar)
  - `Index::folder_for_remote(&self, imap_name: &str, candidate_path: &str) -> Result<FolderState>`
  - `Index::reset_folder(&self, folder_id: i64, uidvalidity: u32) -> Result<()>`
  - `Index::has_duplicate(&self, folder_id: i64, rec: &MessageRecord) -> Result<bool>`
  - `Index::commit_batch(&mut self, folder_id: i64, recs: &[MessageRecord], last_uid: Option<u32>) -> Result<()>`
  - `Index::folders_state(&self) -> Result<Vec<FolderState>>`
  - `Index::restore_folders(&self, folders: &[FolderState]) -> Result<()>`
  - `Index::list_folders(&self) -> Result<Vec<FolderInfo>>`
  - `Index::list_messages(&self, q: &Query) -> Result<MessagePage>`
  - `Index::message_file(&self, message_id: i64) -> Result<String>` (relativo a `mail/`)
  - `Index::reindex(&mut self, mail_root: &Path, progress: &mut dyn FnMut(u64)) -> Result<u64>`

- [ ] **Step 1: Testes**

`src-tauri/src/index.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn raw(subject: &str, date: &str, mid: &str) -> Vec<u8> {
        format!("From: Joao <joao@x.com>\r\nSubject: {subject}\r\nDate: {date}\r\nMessage-ID: <{mid}>\r\n\r\ncorpo\r\n").into_bytes()
    }

    fn rec(file: &str, subject: &str, date: &str, mid: &str) -> MessageRecord {
        MessageRecord::from_raw(&raw(subject, date, mid), store::uid_from_name(file.rsplit('/').next().unwrap()), file.to_string(), String::new(), None)
    }

    fn q(folder_id: Option<i64>, text: Option<&str>) -> Query {
        Query { folder_id, text: text.map(str::to_string), offset: 0, limit: 100, ..Default::default() }
    }

    #[test]
    fn open_reports_reindex_for_new_and_old_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("index.sqlite");
        assert!(Index::open(&p).unwrap().1);
        assert!(!Index::open(&p).unwrap().1);
        {
            let (idx, _) = Index::open(&p).unwrap();
            idx.conn.pragma_update(None, "user_version", 0).unwrap();
        }
        assert!(Index::open(&p).unwrap().1);
    }

    #[test]
    fn record_parses_headers() {
        let r = rec("INBOX/7-00003.eml", "Olá", "Mon, 6 Oct 2026 10:00:00 +0000", "a@b");
        assert_eq!(r.uid, Some(3));
        assert_eq!(r.subject.as_deref(), Some("Olá"));
        assert_eq!(r.from_addr.as_deref(), Some("joao@x.com"));
        assert_eq!(r.from_name.as_deref(), Some("Joao"));
        assert_eq!(r.message_id.as_deref(), Some("a@b"));
        assert_eq!(r.date, Some(1791280800));
    }

    #[test]
    fn record_from_garbage_does_not_panic() {
        let r = MessageRecord::from_raw(b"\x00\xffsem cabecalhos", None, "X/1.eml".into(), String::new(), None);
        assert_eq!(r.size, 16);
        assert!(r.subject.is_none() && r.from_addr.is_none() && r.date.is_none());
    }

    #[test]
    fn folder_for_remote_creates_adopts_and_avoids_case_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let (idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let a = idx.folder_for_remote("Clientes", "Clientes").unwrap();
        assert_eq!(a.path, "Clientes");
        assert_eq!(idx.folder_for_remote("Clientes", "Clientes").unwrap().id, a.id);
        let b = idx.folder_for_remote("clientes", "clientes").unwrap();
        assert_eq!(b.path, "clientes~2");
        // pasta vinda de reindexação (imap_name NULL) é adotada
        idx.restore_folders(&[FolderState { id: 0, imap_name: None, path: "INBOX".into(), uidvalidity: None, last_uid: 0 }]).unwrap();
        let inbox = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        assert_eq!(inbox.path, "INBOX");
        assert_eq!(inbox.imap_name.as_deref(), Some("INBOX"));
    }

    #[test]
    fn commit_batch_updates_last_uid_and_lists_by_date() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        idx.reset_folder(f.id, 7).unwrap();
        idx.commit_batch(f.id, &[
            rec("INBOX/7-00001.eml", "velha", "Mon, 1 Jan 2024 10:00:00 +0000", "1@x"),
            rec("INBOX/7-00002.eml", "nova", "Mon, 6 Oct 2026 10:00:00 +0000", "2@x"),
        ], Some(2)).unwrap();
        let st = idx.folders_state().unwrap();
        assert_eq!((st[0].uidvalidity, st[0].last_uid), (Some(7), 2));
        let page = idx.list_messages(&q(Some(f.id), None)).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.rows[0].subject.as_deref(), Some("nova"));
        assert_eq!(idx.message_file(page.rows[0].id).unwrap(), "INBOX/7-00002.eml");
        assert_eq!(idx.list_folders().unwrap()[0].count, 2);
    }

    #[test]
    fn list_orders_null_dates_last() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        let garbage = MessageRecord::from_raw(b"lixo", Some(9), "INBOX/1-00009.eml".into(), String::new(), None);
        idx.commit_batch(f.id, &[garbage, rec("INBOX/1-00001.eml", "ok", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x")], None).unwrap();
        let rows = idx.list_messages(&q(None, None)).unwrap().rows;
        assert_eq!(rows[0].subject.as_deref(), Some("ok"));
        assert!(rows[1].date.is_none());
    }

    #[test]
    fn search_matches_subject_and_sender_and_treats_wildcards_literally() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        idx.commit_batch(f.id, &[
            rec("INBOX/1-00001.eml", "50% off", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x"),
            rec("INBOX/1-00002.eml", "500 coisas", "Mon, 6 Oct 2026 11:00:00 +0000", "2@x"),
            rec("INBOX/1-00003.eml", "a_b", "Mon, 6 Oct 2026 12:00:00 +0000", "3@x"),
            rec("INBOX/1-00004.eml", "axb", "Mon, 6 Oct 2026 13:00:00 +0000", "4@x"),
        ], None).unwrap();
        assert_eq!(idx.list_messages(&q(None, Some("50%"))).unwrap().total, 1);
        assert_eq!(idx.list_messages(&q(None, Some("a_b"))).unwrap().total, 1);
        assert_eq!(idx.list_messages(&q(None, Some("JOAO@x"))).unwrap().total, 4);
        assert_eq!(idx.list_messages(&q(None, Some("  "))).unwrap().total, 4);
    }

    #[test]
    fn has_duplicate_requires_message_id() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut idx, _) = Index::open(&tmp.path().join("i.sqlite")).unwrap();
        let f = idx.folder_for_remote("INBOX", "INBOX").unwrap();
        let r1 = rec("INBOX/1-00001.eml", "s", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x");
        idx.commit_batch(f.id, &[r1.clone()], None).unwrap();
        let again = rec("INBOX/2-00001.eml", "s", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x");
        assert!(idx.has_duplicate(f.id, &again).unwrap());
        let no_id = MessageRecord { message_id: None, ..again };
        assert!(!idx.has_duplicate(f.id, &no_id).unwrap());
    }

    #[test]
    fn reindex_rebuilds_from_files_keeping_folder_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mail = tmp.path().join("mail");
        store::write_atomic(&mail.join("INBOX").join("5-00001.eml"), &raw("um", "Mon, 6 Oct 2026 10:00:00 +0000", "1@x")).unwrap();
        store::write_atomic(&mail.join("INBOX").join("Sub").join("5-00002.eml"), &raw("dois", "Mon, 6 Oct 2026 10:00:00 +0000", "2@x")).unwrap();
        store::write_atomic(&mail.join("INBOX").join("ignorar.txt"), b"x").unwrap();
        let (mut idx, needs) = Index::open(&tmp.path().join("index.sqlite")).unwrap();
        assert!(needs);
        idx.restore_folders(&[FolderState { id: 0, imap_name: Some("INBOX".into()), path: "INBOX".into(), uidvalidity: Some(5), last_uid: 1 }]).unwrap();
        let mut seen = 0;
        assert_eq!(idx.reindex(&mail, &mut |n| seen = n).unwrap(), 2);
        assert_eq!(seen, 2);
        let folders = idx.list_folders().unwrap();
        assert_eq!(folders.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["INBOX", "INBOX/Sub"]);
        let st = idx.folders_state().unwrap();
        assert_eq!(st.iter().find(|f| f.path == "INBOX").unwrap().last_uid, 1);
        assert_eq!(idx.reindex(&mail, &mut |_| {}).unwrap(), 2);
        assert_eq!(idx.list_messages(&q(None, None)).unwrap().total, 2);
    }

    #[test]
    fn reader_not_blocked_by_open_write_transaction() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("i.sqlite");
        let (mut writer, _) = Index::open(&p).unwrap();
        let f = writer.folder_for_remote("INBOX", "INBOX").unwrap();
        let (reader, _) = Index::open(&p).unwrap();
        let tx = writer.conn.transaction().unwrap();
        tx.execute("INSERT INTO messages (folder_id, file, flags, size, has_attach) VALUES (?1, 'INBOX/x.eml', '', 0, 0)", [f.id]).unwrap();
        assert_eq!(reader.list_messages(&q(None, None)).unwrap().total, 0);
        tx.commit().unwrap();
        assert_eq!(reader.list_messages(&q(None, None)).unwrap().total, 1);
    }
}
```

Adicionar `pub mod index;` em `lib.rs`.

Run: `cd src-tauri && cargo test index`
Expected: FAIL (tipos não definidos).

- [ ] **Step 2: Implementação** (acima dos testes)

```rust
use crate::error::Result;
use crate::store;
use mail_parser::MessageParser;
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS folders (
  id INTEGER PRIMARY KEY,
  imap_name TEXT UNIQUE,
  path TEXT UNIQUE NOT NULL,
  uidvalidity INTEGER,
  last_uid INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER NOT NULL REFERENCES folders(id),
  uid INTEGER,
  file TEXT NOT NULL,
  date INTEGER,
  from_addr TEXT, from_name TEXT, subject TEXT,
  message_id TEXT,
  flags TEXT NOT NULL DEFAULT '', size INTEGER NOT NULL DEFAULT 0, has_attach INTEGER NOT NULL DEFAULT 0,
  UNIQUE(folder_id, file)
);
CREATE INDEX IF NOT EXISTS messages_folder_date ON messages(folder_id, date DESC);
";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FolderState {
    pub id: i64,
    pub imap_name: Option<String>,
    pub path: String,
    pub uidvalidity: Option<u32>,
    pub last_uid: u32,
}

#[derive(Debug, Serialize)]
pub struct FolderInfo {
    pub id: i64,
    pub path: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct MessageRow {
    pub id: i64,
    pub folder_id: i64,
    pub date: Option<i64>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub flags: String,
    pub size: i64,
    pub has_attach: bool,
}

#[derive(Debug, Default, Deserialize)]
pub struct Query {
    pub folder_id: Option<i64>,
    pub text: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct MessagePage {
    pub rows: Vec<MessageRow>,
    pub total: i64,
}

#[derive(Debug, Clone)]
pub struct MessageRecord {
    pub uid: Option<u32>,
    pub file: String,
    pub date: Option<i64>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub message_id: Option<String>,
    pub flags: String,
    pub size: i64,
    pub has_attach: bool,
}

impl MessageRecord {
    pub fn from_raw(raw: &[u8], uid: Option<u32>, file: String, flags: String, internal_date: Option<i64>) -> Self {
        let mut rec = MessageRecord {
            uid, file, date: internal_date, from_addr: None, from_name: None, subject: None,
            message_id: None, flags, size: raw.len() as i64, has_attach: false,
        };
        if let Some(m) = MessageParser::default().parse(raw) {
            let from = m.from().and_then(|a| a.first());
            rec.date = internal_date.or_else(|| m.date().map(|d| d.to_timestamp()));
            rec.from_addr = from.and_then(|a| a.address()).map(str::to_string);
            rec.from_name = from.and_then(|a| a.name()).map(str::to_string);
            rec.subject = m.subject().map(str::to_string);
            rec.message_id = m.message_id().map(str::to_string);
            rec.has_attach = m.attachment_count() > 0;
        }
        rec
    }
}

pub struct Index {
    conn: Connection,
}

fn folder_from_row(r: &rusqlite::Row) -> rusqlite::Result<FolderState> {
    Ok(FolderState { id: r.get(0)?, imap_name: r.get(1)?, path: r.get(2)?, uidvalidity: r.get(3)?, last_uid: r.get(4)? })
}

const FOLDER_COLS: &str = "id, imap_name, path, uidvalidity, last_uid";

impl Index {
    pub fn open(path: &Path) -> Result<(Index, bool)> {
        let existed = path.exists();
        let mut conn = Connection::open(path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        let mut needs_reindex = !existed;
        if existed && version != SCHEMA_VERSION {
            drop(conn);
            for suffix in ["", "-wal", "-shm"] {
                let mut p = path.as_os_str().to_owned();
                p.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(p));
            }
            conn = Connection::open(path)?;
            needs_reindex = true;
        }
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok((Index { conn }, needs_reindex))
    }

    fn taken_paths(&self) -> Result<HashSet<String>> {
        let mut st = self.conn.prepare("SELECT path FROM folders")?;
        let paths = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paths.into_iter().map(|p| p.to_lowercase()).collect())
    }

    fn folder_where(&self, clause: &str, arg: &str) -> Result<Option<FolderState>> {
        Ok(self.conn
            .query_row(&format!("SELECT {FOLDER_COLS} FROM folders WHERE {clause}"), [arg], folder_from_row)
            .optional()?)
    }

    pub fn folder_for_remote(&self, imap_name: &str, candidate_path: &str) -> Result<FolderState> {
        if let Some(f) = self.folder_where("imap_name = ?1", imap_name)? {
            return Ok(f);
        }
        if let Some(mut f) = self.folder_where("path = ?1 AND imap_name IS NULL", candidate_path)? {
            self.conn.execute("UPDATE folders SET imap_name = ?1 WHERE id = ?2", params![imap_name, f.id])?;
            f.imap_name = Some(imap_name.to_string());
            return Ok(f);
        }
        let path = store::unique_path(candidate_path, &self.taken_paths()?);
        self.conn.execute("INSERT INTO folders (imap_name, path) VALUES (?1, ?2)", params![imap_name, path])?;
        Ok(FolderState { id: self.conn.last_insert_rowid(), imap_name: Some(imap_name.to_string()), path, uidvalidity: None, last_uid: 0 })
    }

    fn folder_id_by_path(&self, path: &str) -> Result<i64> {
        if let Some(f) = self.folder_where("path = ?1", path)? {
            return Ok(f.id);
        }
        self.conn.execute("INSERT INTO folders (path) VALUES (?1)", [path])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn reset_folder(&self, folder_id: i64, uidvalidity: u32) -> Result<()> {
        self.conn.execute("UPDATE folders SET uidvalidity = ?1, last_uid = 0 WHERE id = ?2", params![uidvalidity, folder_id])?;
        Ok(())
    }

    pub fn has_duplicate(&self, folder_id: i64, rec: &MessageRecord) -> Result<bool> {
        let Some(mid) = &rec.message_id else { return Ok(false) };
        Ok(self.conn
            .query_row(
                "SELECT 1 FROM messages WHERE folder_id = ?1 AND message_id = ?2 AND date IS ?3 AND size = ?4 LIMIT 1",
                params![folder_id, mid, rec.date, rec.size],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn commit_batch(&mut self, folder_id: i64, recs: &[MessageRecord], last_uid: Option<u32>) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare_cached(
                "INSERT OR REPLACE INTO messages (folder_id, uid, file, date, from_addr, from_name, subject, message_id, flags, size, has_attach)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for r in recs {
                st.execute(params![folder_id, r.uid, r.file, r.date, r.from_addr, r.from_name, r.subject, r.message_id, r.flags, r.size, r.has_attach])?;
            }
        }
        if let Some(uid) = last_uid {
            tx.execute("UPDATE folders SET last_uid = ?1 WHERE id = ?2", params![uid, folder_id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn folders_state(&self) -> Result<Vec<FolderState>> {
        let mut st = self.conn.prepare(&format!("SELECT {FOLDER_COLS} FROM folders ORDER BY path"))?;
        let rows = st.query_map([], folder_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn restore_folders(&self, folders: &[FolderState]) -> Result<()> {
        for f in folders {
            self.conn.execute(
                "INSERT OR IGNORE INTO folders (imap_name, path, uidvalidity, last_uid) VALUES (?1, ?2, ?3, ?4)",
                params![f.imap_name, f.path, f.uidvalidity, f.last_uid],
            )?;
        }
        Ok(())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderInfo>> {
        let mut st = self.conn.prepare(
            "SELECT f.id, f.path, COUNT(m.id) FROM folders f LEFT JOIN messages m ON m.folder_id = f.id
             GROUP BY f.id ORDER BY f.path COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([], |r| Ok(FolderInfo { id: r.get(0)?, path: r.get(1)?, count: r.get(2)? }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_messages(&self, q: &Query) -> Result<MessagePage> {
        let mut sql = String::from(" FROM messages WHERE 1 = 1");
        let mut args: Vec<Value> = Vec::new();
        if let Some(f) = q.folder_id {
            sql.push_str(" AND folder_id = ?");
            args.push(f.into());
        }
        if let Some(t) = q.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let escaped = t.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            sql.push_str(" AND (subject LIKE ? ESCAPE '\\' OR from_name LIKE ? ESCAPE '\\' OR from_addr LIKE ? ESCAPE '\\')");
            for _ in 0..3 {
                args.push(pattern.clone().into());
            }
        }
        if let Some(d) = q.date_from {
            sql.push_str(" AND date >= ?");
            args.push(d.into());
        }
        if let Some(d) = q.date_to {
            sql.push_str(" AND date < ?");
            args.push(d.into());
        }
        let total: i64 = self.conn.query_row(&format!("SELECT COUNT(*){sql}"), params_from_iter(args.iter()), |r| r.get(0))?;
        args.push(q.limit.clamp(1, 1000).into());
        args.push(q.offset.max(0).into());
        let mut st = self.conn.prepare(&format!(
            "SELECT id, folder_id, date, from_addr, from_name, subject, flags, size, has_attach{sql} ORDER BY date DESC, id DESC LIMIT ? OFFSET ?"
        ))?;
        let rows = st
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(MessageRow {
                    id: r.get(0)?, folder_id: r.get(1)?, date: r.get(2)?, from_addr: r.get(3)?, from_name: r.get(4)?,
                    subject: r.get(5)?, flags: r.get(6)?, size: r.get(7)?, has_attach: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(MessagePage { rows, total })
    }

    pub fn message_file(&self, message_id: i64) -> Result<String> {
        Ok(self.conn.query_row("SELECT file FROM messages WHERE id = ?1", [message_id], |r| r.get(0))?)
    }

    // ponytail: flags não são recuperáveis do .eml; reindexação as deixa vazias
    pub fn reindex(&mut self, mail_root: &Path, progress: &mut dyn FnMut(u64)) -> Result<u64> {
        self.conn.execute("DELETE FROM messages", [])?;
        let mut batch: Vec<MessageRecord> = Vec::new();
        let mut current: Option<(String, i64)> = None;
        let mut n = 0u64;
        for (abs, rel) in store::walk_files(mail_root)? {
            if !rel.ends_with(".eml") {
                continue;
            }
            let Some((folder, name)) = rel.rsplit_once('/') else { continue };
            let folder_id = match &current {
                Some((p, id)) if p == folder => *id,
                _ => {
                    if let Some((_, id)) = current.take() {
                        self.commit_batch(id, &batch, None)?;
                        batch.clear();
                    }
                    let id = self.folder_id_by_path(folder)?;
                    current = Some((folder.to_string(), id));
                    id
                }
            };
            let raw = std::fs::read(&abs)?;
            batch.push(MessageRecord::from_raw(&raw, store::uid_from_name(name), rel.clone(), String::new(), None));
            n += 1;
            if batch.len() >= 500 {
                self.commit_batch(folder_id, &batch, None)?;
                batch.clear();
                progress(n);
            }
        }
        if let Some((_, id)) = current {
            self.commit_batch(id, &batch, None)?;
        }
        progress(n);
        Ok(n)
    }
}
```

Nota: o valor `1791280800` do teste `record_parses_headers` é `2026-10-06T10:00:00Z`; se divergir, confira com `date -d @1791280800 -u` e corrija o literal do teste, não a implementação.

Run: `cd src-tauri && cargo test index`
Expected: PASS (10 tests).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/index.rs src-tauri/src/lib.rs
git commit -m "feat(index): índice SQLite com estado de sync, busca e reindexação"
```

---

### Task 3: `config.rs` — contas e senhas

**Files:**
- Create: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod config;`)

**Interfaces:**
- Consumes: `store::write_atomic`, `error::{AppError, Result}`
- Produces:
  - `pub struct LastRun { pub at: i64, pub new_messages: u64, pub failed_folders: Vec<String>, pub error: Option<String> }`
  - `pub struct Account { pub id: String, pub name: String, pub email: String, pub host: String, pub port: u16, pub username: String, pub dir: PathBuf, pub has_credentials: bool, pub last_run: Option<LastRun> }` (Serialize, Deserialize, Clone, PartialEq, Debug)
  - `pub struct Config { pub accounts: Vec<Account> }` com `load(&Path) -> Result<Config>`, `save(&self, &Path) -> Result<()>`, `get(&self, id) -> Result<&Account>`, `get_mut(&mut self, id) -> Result<&mut Account>`
  - `pub fn set_password(id: &str, password: &str) -> Result<()>`, `get_password(id: &str) -> Result<String>`, `delete_password(id: &str) -> Result<()>`

- [ ] **Step 1: Testes**

`src-tauri/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> Account {
        Account {
            id: id.into(), name: "Joao".into(), email: "joao@x.com".into(), host: "imap.uhserver.com".into(),
            port: 993, username: "joao@x.com".into(), dir: PathBuf::from("C:/tmp/x"), has_credentials: true, last_run: None,
        }
    }

    #[test]
    fn load_missing_returns_empty_and_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        assert_eq!(Config::load(&p).unwrap(), Config::default());
        let mut c = Config::default();
        c.accounts.push(account("a1"));
        c.save(&p).unwrap();
        let loaded = Config::load(&p).unwrap();
        assert_eq!(loaded, c);
        assert!(loaded.get("a1").is_ok());
        assert!(loaded.get("nope").is_err());
    }

    #[test]
    fn invalid_json_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, b"{nao json").unwrap();
        assert!(matches!(Config::load(&p), Err(AppError::Other(_))));
    }

    #[test]
    fn keyring_roundtrip() {
        let id = format!("teste-{}", uuid::Uuid::new_v4());
        set_password(&id, "s3nha").unwrap();
        assert_eq!(get_password(&id).unwrap(), "s3nha");
        delete_password(&id).unwrap();
        assert!(get_password(&id).is_err());
        delete_password(&id).unwrap(); // apagar o que não existe não é erro
    }
}
```

Adicionar `pub mod config;` em `lib.rs`.

Run: `cd src-tauri && cargo test config`
Expected: FAIL (tipos não definidos).

- [ ] **Step 2: Implementação** (acima dos testes)

```rust
use crate::error::{AppError, Result};
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "mail-backup";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LastRun {
    pub at: i64,
    pub new_messages: u64,
    pub failed_folders: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub dir: PathBuf,
    pub has_credentials: bool,
    pub last_run: Option<LastRun>,
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub accounts: Vec<Account>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| AppError::Other(format!("config.json inválido: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| AppError::Other(e.to_string()))?;
        store::write_atomic(path, &json)
    }

    pub fn get(&self, id: &str) -> Result<&Account> {
        self.accounts.iter().find(|a| a.id == id).ok_or_else(|| AppError::Other(format!("conta não encontrada: {id}")))
    }

    pub fn get_mut(&mut self, id: &str) -> Result<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id).ok_or_else(|| AppError::Other(format!("conta não encontrada: {id}")))
    }
}

fn entry(id: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, id).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn set_password(id: &str, password: &str) -> Result<()> {
    entry(id)?.set_password(password).map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn get_password(id: &str) -> Result<String> {
    entry(id)?.get_password().map_err(|e| AppError::Keyring(e.to_string()))
}

pub fn delete_password(id: &str) -> Result<()> {
    match entry(id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}
```

Run: `cd src-tauri && cargo test config`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "feat(config): contas em config.json e senha no Gerenciador de Credenciais"
```

---

### Task 4: `imap_sync.rs` — backup incremental

**Files:**
- Create: `src-tauri/src/imap_sync.rs`
- Create: `src-tauri/tests/greenmail.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod imap_sync;`)

**Interfaces:**
- Consumes: `store::{folder_rel_path, eml_name, join_rel, write_atomic, remove_tmp_files}`, `index::{Index, MessageRecord}` (`folder_for_remote`, `reset_folder`, `has_duplicate`, `commit_batch`), `error::{AppError, Result}`
- Produces:
  - `pub type TlsSession = async_imap::Session<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;`
  - `pub struct Progress { pub account_id: String, pub folder: String, pub done: u32, pub total: u32 }` (Serialize, Clone)
  - `pub struct SyncReport { pub new_messages: u64, pub failed_folders: Vec<String>, pub cancelled: bool }` (Serialize, Clone, Default)
  - `pub struct SyncCtx<'a> { pub account_id: &'a str, pub mail_root: &'a Path, pub cancel: &'a AtomicBool, pub progress: &'a (dyn Fn(Progress) + Send + Sync) }`
  - `pub async fn login<T>(stream: T, user: &str, pass: &str) -> Result<Session<T>>` (lê a saudação, mapeia NO/BAD para `AuthFailed`)
  - `pub async fn connect_tls(host: &str, port: u16, user: &str, pass: &str) -> Result<TlsSession>`
  - `pub async fn sync_account<T, C, F>(connect: C, ctx: &SyncCtx<'_>, index: &mut Index) -> Result<SyncReport>` onde `C: FnMut() -> F`, `F: Future<Output = Result<Session<T>>>`

- [ ] **Step 1: GreenMail local**

```bash
mkdir -p tools
curl -L -o tools/greenmail.jar https://repo1.maven.org/maven2/com/icegreen/greenmail-standalone/1.6.15/greenmail-standalone-1.6.15.jar
java -Dgreenmail.setup.test.imap -Dgreenmail.hostname=127.0.0.1 -Dgreenmail.auth.disabled -jar tools/greenmail.jar
```
Rodar em segundo plano (terminal separado). Expected: log com `Started imap:127.0.0.1:3143`. Com `auth.disabled`, qualquer usuário/senha é aceito e o usuário é criado na hora, então cada teste usa um usuário próprio e não interfere nos outros. (GreenMail 1.6.x roda no Java 8 instalado.)

- [ ] **Step 2: Testes de integração**

`src-tauri/tests/greenmail.rs`:
```rust
//! Requer GreenMail em 127.0.0.1:3143 (ver Task 4, Step 1). Rodar com:
//! cargo test --test greenmail -- --ignored --test-threads=1
use futures::TryStreamExt;
use mail_backup_lib::error::{AppError, Result};
use mail_backup_lib::imap_sync::{self, Progress, SyncCtx, SyncReport};
use mail_backup_lib::index::{Index, Query};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::TcpStream;

async fn plain(user: &str) -> Result<async_imap::Session<TcpStream>> {
    let tcp = TcpStream::connect(("127.0.0.1", 3143)).await.map_err(|e| AppError::Network(e.to_string()))?;
    imap_sync::login(tcp, user, "x").await
}

fn user(tag: &str) -> String {
    format!("{tag}{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())
}

fn msg(i: usize) -> String {
    format!("From: a{i}@x.com\r\nTo: t@x.com\r\nSubject: Teste {i}\r\nMessage-ID: <m{i}@x>\r\nDate: Mon, 6 Oct 2026 10:00:00 +0000\r\n\r\nCorpo {i}\r\n")
}

async fn seed(user: &str, folder: &str, range: std::ops::Range<usize>) {
    let mut s = plain(user).await.unwrap();
    if folder != "INBOX" {
        let _ = s.create(folder).await;
    }
    for i in range {
        s.append(folder, None, None, msg(i)).await.unwrap();
    }
    s.logout().await.unwrap();
}

async fn run(user: &str, dir: &Path, cancel_after_first_batch: bool) -> (SyncReport, Index) {
    let (mut index, _) = Index::open(&dir.join("index.sqlite")).unwrap();
    let cancel = AtomicBool::new(false);
    let progress = |_: Progress| if cancel_after_first_batch { cancel.store(true, Ordering::SeqCst) };
    let mail = dir.join("mail");
    let ctx = SyncCtx { account_id: "t", mail_root: &mail, cancel: &cancel, progress: &progress };
    let report = imap_sync::sync_account(|| plain(user), &ctx, &mut index).await.unwrap();
    (report, index)
}

fn eml_count(dir: &Path) -> usize {
    mail_backup_lib::store::walk_files(&dir.join("mail")).unwrap().iter().filter(|f| f.1.ends_with(".eml")).count()
}

fn total(index: &Index) -> i64 {
    index.list_messages(&Query { limit: 1, ..Default::default() }).unwrap().total
}

#[tokio::test]
#[ignore]
async fn full_then_incremental() {
    let u = user("inc");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..5).await;
    seed(&u, "Clientes", 0..3).await;
    let (r, _) = run(&u, tmp.path(), false).await;
    assert_eq!(r.new_messages, 8);
    assert!(r.failed_folders.is_empty());
    assert!(tmp.path().join("mail").join("Clientes").exists());
    seed(&u, "INBOX", 5..7).await;
    assert_eq!(run(&u, tmp.path(), false).await.0.new_messages, 2);
    let (r, index) = run(&u, tmp.path(), false).await; // "N:*" devolve a última mensagem: não pode baixar de novo
    assert_eq!(r.new_messages, 0);
    assert_eq!(total(&index), 10);
    assert_eq!(eml_count(tmp.path()), 10);
}

#[tokio::test]
#[ignore]
async fn interrupted_backup_resumes_without_duplicates() {
    let u = user("cancel");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..250).await;
    let (r, _) = run(&u, tmp.path(), true).await;
    assert!(r.cancelled);
    assert!(r.new_messages < 250);
    let (_, index) = run(&u, tmp.path(), false).await;
    assert_eq!(total(&index), 250);
    assert_eq!(eml_count(tmp.path()), 250);
}

#[tokio::test]
#[ignore]
async fn uidvalidity_change_does_not_duplicate() {
    let u = user("uv");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "Pasta", 0..3).await;
    run(&u, tmp.path(), false).await;
    let before = plain(&u).await.unwrap().examine("Pasta").await.unwrap().uid_validity;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let mut s = plain(&u).await.unwrap();
    s.delete("Pasta").await.unwrap();
    s.logout().await.unwrap();
    seed(&u, "Pasta", 0..3).await;
    let after = plain(&u).await.unwrap().examine("Pasta").await.unwrap().uid_validity;
    assert_ne!(before, after, "GreenMail não mudou o UIDVALIDITY; o teste não é válido");
    let (r, index) = run(&u, tmp.path(), false).await;
    assert_eq!(r.new_messages, 0);
    assert_eq!(total(&index), 3);
}

#[tokio::test]
#[ignore]
async fn backup_never_marks_messages_as_seen() {
    let u = user("seen");
    let tmp = tempfile::tempdir().unwrap();
    seed(&u, "INBOX", 0..3).await;
    run(&u, tmp.path(), false).await;
    let mut s = plain(&u).await.unwrap();
    s.examine("INBOX").await.unwrap();
    let fetches: Vec<_> = s.fetch("1:*", "FLAGS").await.unwrap().try_collect().await.unwrap();
    assert_eq!(fetches.len(), 3);
    for f in &fetches {
        assert!(!f.flags().any(|fl| matches!(fl, async_imap::types::Flag::Seen)));
    }
}
```

Adicionar `pub mod imap_sync;` em `lib.rs`.

Run: `cd src-tauri && cargo test --test greenmail -- --ignored --test-threads=1`
Expected: FAIL (compilação: `imap_sync` sem os itens).

- [ ] **Step 3: Implementação**

`src-tauri/src/imap_sync.rs`:
```rust
use crate::error::{AppError, Result};
use crate::index::{Index, MessageRecord};
use crate::store;
use async_imap::types::{Fetch, NameAttribute};
use async_imap::{Client, Session};
use futures::TryStreamExt;
use serde::Serialize;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::rustls;

pub type TlsSession = Session<tokio_rustls::client::TlsStream<TcpStream>>;

const BATCH: u32 = 100;
const BACKOFF_SECS: [u64; 3] = [1, 4, 16];
const FETCH_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])";
const READ_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub account_id: String,
    pub folder: String,
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncReport {
    pub new_messages: u64,
    pub failed_folders: Vec<String>,
    pub cancelled: bool,
}

pub struct SyncCtx<'a> {
    pub account_id: &'a str,
    pub mail_root: &'a Path,
    pub cancel: &'a AtomicBool,
    pub progress: &'a (dyn Fn(Progress) + Send + Sync),
}

struct RemoteFolder {
    name: String,
    delimiter: Option<String>,
}

fn net(e: impl std::fmt::Display) -> AppError {
    AppError::Network(e.to_string())
}

pub async fn login<T>(stream: T, user: &str, pass: &str) -> Result<Session<T>>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let mut client = Client::new(stream);
    client
        .read_response()
        .await
        .map_err(net)?
        .ok_or_else(|| net("o servidor fechou a conexão antes da saudação"))?;
    client.login(user, pass).await.map_err(|(e, _)| match e {
        async_imap::error::Error::No(_) | async_imap::error::Error::Bad(_) => AppError::AuthFailed,
        other => net(other),
    })
}

pub async fn connect_tls(host: &str, port: u16, user: &str, pass: &str) -> Result<TlsSession> {
    let tls = |e: rustls::Error| AppError::Tls(e.to_string());
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::time::timeout(Duration::from_secs(20), TcpStream::connect((host, port)))
        .await
        .map_err(|_| net("tempo esgotado ao conectar"))?
        .map_err(net)?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| AppError::Tls(e.to_string()))?;
    let stream = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(name, tcp)
        .await
        .map_err(|e| AppError::Tls(e.to_string()))?;
    login(stream, user, pass).await
}

async fn list_folders<T>(s: &mut Session<T>) -> Result<Vec<RemoteFolder>>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let names: Vec<async_imap::types::Name> = s.list(Some(""), Some("*")).await.map_err(net)?.try_collect().await.map_err(net)?;
    Ok(names
        .iter()
        .filter(|n| !n.attributes().iter().any(|a| matches!(a, NameAttribute::NoSelect)))
        .map(|n| RemoteFolder { name: n.name().to_string(), delimiter: n.delimiter().map(str::to_string) })
        .collect())
}

fn flags_string(f: &Fetch) -> String {
    f.flags().map(|fl| format!("{fl:?}")).collect::<Vec<_>>().join(" ")
}

async fn sync_folder<T>(s: &mut Session<T>, remote: &RemoteFolder, ctx: &SyncCtx<'_>, index: &mut Index) -> Result<u64>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let candidate = store::folder_rel_path(&remote.name, remote.delimiter.as_deref());
    let mut folder = index.folder_for_remote(&remote.name, &candidate)?;
    let mailbox = s.examine(&remote.name).await.map_err(net)?;
    let uidvalidity = mailbox.uid_validity.unwrap_or(0);
    let dedupe = folder.uidvalidity.is_some_and(|v| v != uidvalidity);
    if folder.uidvalidity != Some(uidvalidity) {
        index.reset_folder(folder.id, uidvalidity)?;
        folder.last_uid = 0;
    }
    if mailbox.exists == 0 || mailbox.uid_next.is_some_and(|next| next <= folder.last_uid + 1) {
        return Ok(0);
    }
    let total = mailbox.uid_next.map_or(mailbox.exists, |n| n - folder.last_uid - 1).min(mailbox.exists);
    let report = |done: u32| {
        (ctx.progress)(Progress { account_id: ctx.account_id.to_string(), folder: folder.path.clone(), done, total })
    };

    let mut stream = s.uid_fetch(format!("{}:*", folder.last_uid + 1), FETCH_QUERY).await.map_err(net)?;
    let mut batch = Vec::with_capacity(BATCH as usize);
    let (mut last_uid, mut done, mut saved) = (folder.last_uid, 0u32, 0u64);
    loop {
        let next = tokio::time::timeout(READ_TIMEOUT, stream.try_next())
            .await
            .map_err(|_| net("tempo esgotado aguardando o servidor"))?
            .map_err(net)?;
        let Some(fetch) = next else { break };
        let Some(uid) = fetch.uid else { continue };
        if uid <= folder.last_uid {
            continue; // "N:*" devolve a última mensagem mesmo sem novidades
        }
        last_uid = last_uid.max(uid);
        done += 1;
        // ponytail: resposta sem corpo (mensagem removida durante o fetch) é ignorada
        if let Some(raw) = fetch.body() {
            let file = format!("{}/{}", folder.path, store::eml_name(uidvalidity, uid));
            let internal = fetch.internal_date().map(|d| d.timestamp());
            let rec = MessageRecord::from_raw(raw, Some(uid), file, flags_string(&fetch), internal);
            if !(dedupe && index.has_duplicate(folder.id, &rec)?) {
                store::write_atomic(&store::join_rel(ctx.mail_root, &rec.file), raw)?;
                batch.push(rec);
                saved += 1;
            }
        }
        if done % BATCH == 0 {
            index.commit_batch(folder.id, &batch, Some(last_uid))?;
            batch.clear();
            report(done);
            if ctx.cancel.load(Ordering::SeqCst) {
                return Ok(saved);
            }
        }
    }
    index.commit_batch(folder.id, &batch, Some(last_uid))?;
    report(done);
    Ok(saved)
}

pub async fn sync_account<T, C, F>(mut connect: C, ctx: &SyncCtx<'_>, index: &mut Index) -> Result<SyncReport>
where
    T: AsyncRead + AsyncWrite + Unpin + std::fmt::Debug + Send,
    C: FnMut() -> F,
    F: Future<Output = Result<Session<T>>>,
{
    store::remove_tmp_files(ctx.mail_root)?;
    let mut report = SyncReport::default();
    let mut session = connect().await?;
    let folders = list_folders(&mut session).await?;
    for remote in &folders {
        if ctx.cancel.load(Ordering::SeqCst) {
            report.cancelled = true;
            return Ok(report); // sessão descartada sem LOGOUT: pode haver respostas pendentes
        }
        let mut attempt = 0;
        loop {
            match sync_folder(&mut session, remote, ctx, index).await {
                Ok(n) => {
                    report.new_messages += n;
                    break;
                }
                Err(e) if e.is_retryable() && attempt < BACKOFF_SECS.len() => {
                    tokio::time::sleep(Duration::from_secs(BACKOFF_SECS[attempt])).await;
                    attempt += 1;
                    match connect().await {
                        Ok(s) => session = s,
                        Err(e) if e.is_retryable() => continue,
                        Err(e) => return Err(e),
                    }
                }
                Err(e) if e.is_retryable() => {
                    report.failed_folders.push(store::folder_rel_path(&remote.name, remote.delimiter.as_deref()));
                    break;
                }
                Err(e) => return Err(e),
            }
        }
    }
    if ctx.cancel.load(Ordering::SeqCst) {
        report.cancelled = true;
        return Ok(report);
    }
    let _ = session.logout().await;
    Ok(report)
}
```

Run: `cd src-tauri && cargo build && cargo test --test greenmail -- --ignored --test-threads=1`
Expected: PASS (4 tests). Se `uidvalidity_change_does_not_duplicate` falhar na asserção "GreenMail não mudou o UIDVALIDITY", aumentar o `sleep` para 2100 ms (o GreenMail deriva o valor do relógio).

Run também: `cd src-tauri && cargo test --lib`
Expected: PASS (todos os testes unitários anteriores).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/imap_sync.rs src-tauri/tests/greenmail.rs src-tauri/src/lib.rs
git commit -m "feat(imap): backup incremental com retomada, reconexão e somente leitura"
```

---

### Task 5: `archive.rs` — exportar/importar `.zip`

**Files:**
- Create: `src-tauri/src/archive.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod archive;`)

**Interfaces:**
- Consumes: `store::{walk_files, sanitize_component, unique_path, is_safe_rel, join_rel}`, `index::FolderState`, `error::{AppError, Result}`
- Produces:
  - `pub struct AccountManifest { pub name: String, pub email: String, pub host: String, pub port: u16, pub username: String, pub folders: Vec<FolderState> }` (Serialize, Deserialize, Clone, PartialEq, Debug)
  - `pub struct ExportItem<'a> { pub manifest: AccountManifest, pub dir: &'a Path }`
  - `pub struct ImportedAccount { pub dir: PathBuf, pub top: String, pub manifest: Option<AccountManifest> }`
  - `pub fn export(dest: &Path, items: &[ExportItem], password: Option<&str>) -> Result<()>`
  - `pub fn import(src: &Path, accounts_root: &Path, password: Option<&str>, new_id: impl FnMut() -> String) -> Result<Vec<ImportedAccount>>`

- [ ] **Step 1: Testes**

`src-tauri/src/archive.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn manifest(email: &str) -> AccountManifest {
        AccountManifest {
            name: "Joao".into(), email: email.into(), host: "imap.uhserver.com".into(), port: 993, username: email.into(),
            folders: vec![FolderState { id: 1, imap_name: Some("INBOX".into()), path: "INBOX".into(), uidvalidity: Some(7), last_uid: 2 }],
        }
    }

    fn account_dir(root: &Path) -> PathBuf {
        let dir = root.join("conta");
        store::write_atomic(&dir.join("mail/INBOX/7-00001.eml"), b"um").unwrap();
        store::write_atomic(&dir.join("mail/Itens Enviados/A\u{e7}\u{e3}o/7-00002.eml"), b"dois").unwrap();
        store::write_atomic(&dir.join("index.sqlite"), b"nao exportar").unwrap();
        dir
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || { n += 1; format!("id{n}") }
    }

    #[test]
    fn roundtrip_with_password_preserves_bytes_and_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], Some("s3nha")).unwrap();
        assert!(!tmp.path().join("b.zip.tmp").exists());
        let out = tmp.path().join("accounts");
        let imported = import(&zip_path, &out, Some("s3nha"), ids()).unwrap();
        assert_eq!(imported.len(), 1);
        let acc = &imported[0];
        assert_eq!(acc.top, "a@b.com");
        assert_eq!(acc.manifest.as_ref().unwrap(), &manifest("a@b.com"));
        assert_eq!(std::fs::read(acc.dir.join("mail/INBOX/7-00001.eml")).unwrap(), b"um");
        assert_eq!(std::fs::read(acc.dir.join("mail/Itens Enviados/A\u{e7}\u{e3}o/7-00002.eml")).unwrap(), b"dois");
        assert!(!acc.dir.join("index.sqlite").exists());
    }

    #[test]
    fn wrong_or_missing_password_fails_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], Some("s3nha")).unwrap();
        let out = tmp.path().join("accounts");
        assert!(matches!(import(&zip_path, &out, Some("errada"), ids()), Err(AppError::WrongPassword)));
        assert!(matches!(import(&zip_path, &out, None, ids()), Err(AppError::WrongPassword)));
        assert!(!out.exists() || std::fs::read_dir(&out).unwrap().next().is_none());
    }

    #[test]
    fn export_without_password_imports_without_password() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        export(&zip_path, &[ExportItem { manifest: manifest("a@b.com"), dir: &dir }], None).unwrap();
        let imported = import(&zip_path, &tmp.path().join("accounts"), None, ids()).unwrap();
        assert_eq!(imported.len(), 1);
    }

    #[test]
    fn export_two_accounts_same_email_keeps_both() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = account_dir(tmp.path());
        let zip_path = tmp.path().join("b.zip");
        let items = [ExportItem { manifest: manifest("a@b.com"), dir: &dir }, ExportItem { manifest: manifest("A@b.com"), dir: &dir }];
        export(&zip_path, &items, None).unwrap();
        let imported = import(&zip_path, &tmp.path().join("accounts"), None, ids()).unwrap();
        let tops: Vec<&str> = imported.iter().map(|a| a.top.as_str()).collect();
        assert_eq!(tops, vec!["a@b.com", "A@b.com~2"]);
        assert_ne!(imported[0].dir, imported[1].dir);
    }

    #[test]
    fn zip_slip_entries_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("evil.zip");
        {
            let mut w = ZipWriter::new(File::create(&zip_path).unwrap());
            for name in ["../evil.eml", "acc/mail/../../evil2.eml", "acc/mail/INBOX/1-00001.eml"] {
                w.start_file(name, SimpleFileOptions::default()).unwrap();
                w.write_all(b"x").unwrap();
            }
            w.finish().unwrap();
        }
        let out = tmp.path().join("accounts");
        let imported = import(&zip_path, &out, None, ids()).unwrap();
        assert_eq!(imported.len(), 1);
        assert!(imported[0].dir.join("mail/INBOX/1-00001.eml").exists());
        assert!(!tmp.path().join("evil.eml").exists());
        assert!(!out.join("evil2.eml").exists());
        assert!(!imported[0].dir.join("evil2.eml").exists());
    }

    #[test]
    fn garbage_file_is_corrupt_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.zip");
        std::fs::write(&p, b"nao sou zip").unwrap();
        assert!(matches!(import(&p, &tmp.path().join("a"), None, ids()), Err(AppError::CorruptArchive(_))));
    }
}
```

Adicionar `pub mod archive;` em `lib.rs`.

Run: `cd src-tauri && cargo test archive`
Expected: FAIL (itens não definidos).

- [ ] **Step 2: Implementação** (acima dos testes)

```rust
use crate::error::{AppError, Result};
use crate::index::FolderState;
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use zip::result::ZipError;
use zip::write::{FileOptions, SimpleFileOptions};
use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountManifest {
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub folders: Vec<FolderState>,
}

pub struct ExportItem<'a> {
    pub manifest: AccountManifest,
    pub dir: &'a Path,
}

#[derive(Debug)]
pub struct ImportedAccount {
    pub dir: PathBuf,
    pub top: String,
    pub manifest: Option<AccountManifest>,
}

fn zip_err(e: ZipError) -> AppError {
    match e {
        ZipError::Io(io) => AppError::Io(io),
        ZipError::InvalidPassword => AppError::WrongPassword,
        other => AppError::CorruptArchive(other.to_string()),
    }
}

fn options(password: Option<&str>) -> FileOptions<'_, ()> {
    let o = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    match password {
        Some(p) => o.with_aes_encryption(AesMode::Aes256, p),
        None => o,
    }
}

pub fn export(dest: &Path, items: &[ExportItem], password: Option<&str>) -> Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| -> Result<()> {
        let mut zip = ZipWriter::new(File::create(&tmp)?);
        let opts = options(password);
        let mut used = HashSet::new();
        for item in items {
            let top = store::unique_path(&store::sanitize_component(&item.manifest.email), &used);
            used.insert(top.to_lowercase());
            zip.start_file(format!("{top}/account.json"), opts.clone()).map_err(zip_err)?;
            serde_json::to_writer_pretty(&mut zip, &item.manifest).map_err(|e| AppError::Other(e.to_string()))?;
            for (abs, rel) in store::walk_files(&item.dir.join("mail"))? {
                if !rel.ends_with(".eml") {
                    continue;
                }
                zip.start_file(format!("{top}/mail/{rel}"), opts.clone()).map_err(zip_err)?;
                io::copy(&mut File::open(&abs)?, &mut zip)?;
            }
        }
        drop(zip.finish().map_err(zip_err)?);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(fs::rename(&tmp, dest)?),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn import(src: &Path, accounts_root: &Path, password: Option<&str>, mut new_id: impl FnMut() -> String) -> Result<Vec<ImportedAccount>> {
    let mut created: Vec<ImportedAccount> = Vec::new();
    let result = (|| -> Result<()> {
        let mut archive = ZipArchive::new(File::open(src)?).map_err(zip_err)?;
        let mut by_top: HashMap<String, usize> = HashMap::new();
        for i in 0..archive.len() {
            let mut entry = match password {
                Some(p) => archive.by_index_decrypt(i, p.as_bytes()),
                None => archive.by_index(i),
            }
            .map_err(zip_err)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            if !store::is_safe_rel(&name) {
                continue; // UnsafePath: entrada ignorada (zip-slip)
            }
            let Some((top, rest)) = name.split_once('/') else { continue };
            let idx = *by_top.entry(top.to_string()).or_insert_with(|| {
                created.push(ImportedAccount { dir: accounts_root.join(new_id()), top: top.to_string(), manifest: None });
                created.len() - 1
            });
            let acc = &mut created[idx];
            if rest == "account.json" {
                acc.manifest = Some(serde_json::from_reader(&mut entry).map_err(|e| AppError::CorruptArchive(e.to_string()))?);
            } else if let Some(rel) = rest.strip_prefix("mail/") {
                let out = store::join_rel(&acc.dir.join("mail"), rel);
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                io::copy(&mut entry, &mut File::create(&out)?).map_err(|e| match e.kind() {
                    io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => AppError::CorruptArchive(e.to_string()),
                    _ => AppError::Io(e),
                })?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(created),
        Err(e) => {
            for a in &created {
                let _ = fs::remove_dir_all(&a.dir);
            }
            Err(e)
        }
    }
}
```

Run: `cd src-tauri && cargo test archive`
Expected: PASS (6 tests). Se `wrong_or_missing_password_fails_and_cleans_up` falhar no caso `None` com outro erro do `zip` (ex.: `UnsupportedArchive("Password required to decrypt file")`), mapear esse caso específico para `AppError::WrongPassword` em `zip_err` — o front depende disso para pedir a senha.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/archive.rs src-tauri/src/lib.rs
git commit -m "feat(archive): exportar/importar zip com AES-256 opcional e proteção zip-slip"
```

---

### Task 6: `message.rs` — leitura segura e anexos

**Files:**
- Create: `src-tauri/src/message.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `pub mod message;`)

**Interfaces:**
- Produces:
  - `pub struct AttachmentInfo { pub index: usize, pub name: String, pub mime: String, pub size: usize, pub blocked: bool }` (Serialize)
  - `pub struct MessageView { pub from: String, pub to: String, pub cc: String, pub date: Option<i64>, pub subject: String, pub html: String, pub has_remote_images: bool, pub attachments: Vec<AttachmentInfo> }` (Serialize, Default)
  - `pub fn render(raw: &[u8], allow_remote_images: bool) -> MessageView`
  - `pub fn attachment(raw: &[u8], index: usize) -> Option<(String, Vec<u8>)>`
  - `pub fn is_blocked(name: &str) -> bool`

- [ ] **Step 1: Testes**

`src-tauri/src/message.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const HTML_MSG: &str = "From: Ana <ana@x.com>\r\nTo: Joao <j@x.com>, b@x.com\r\nSubject: Oi\r\nDate: Mon, 6 Oct 2026 10:00:00 +0000\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: multipart/related; boundary=R\r\n\r\n--R\r\nContent-Type: text/html\r\n\r\n<p>Ol\u{e1}</p><img src=\"cid:img1\"><img src=\"https://track.example/p.gif\"><script>alert(1)</script>\r\n--R\r\nContent-Type: image/png\r\nContent-ID: <img1>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n--R--\r\n--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"nota.pdf\"\r\n\r\nPDF\r\n--B\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"virus.EXE\"\r\n\r\nMZ\r\n--B--\r\n";

    #[test]
    fn html_gets_csp_base_and_inline_cid() {
        let v = render(HTML_MSG.as_bytes(), false);
        assert_eq!(v.subject, "Oi");
        assert_eq!(v.from, "Ana <ana@x.com>");
        assert_eq!(v.to, "Joao <j@x.com>, b@x.com");
        assert!(v.html.contains("default-src 'none'; img-src data:;"));
        assert!(v.html.contains(r#"<base target="_blank">"#));
        assert!(v.html.contains("data:image/png;base64,"));
        assert!(!v.html.contains("cid:img1"));
        assert!(v.has_remote_images);
        assert!(render(HTML_MSG.as_bytes(), true).html.contains("img-src data: https:;"));
    }

    #[test]
    fn attachments_listed_and_executables_blocked() {
        let v = render(HTML_MSG.as_bytes(), false);
        let names: Vec<(&str, bool)> = v.attachments.iter().map(|a| (a.name.as_str(), a.blocked)).collect();
        assert!(names.contains(&("nota.pdf", false)));
        assert!(names.contains(&("virus.EXE", true)));
        let pdf = v.attachments.iter().find(|a| a.name == "nota.pdf").unwrap();
        let (name, bytes) = attachment(HTML_MSG.as_bytes(), pdf.index).unwrap();
        assert_eq!(name, "nota.pdf");
        assert_eq!(bytes, b"PDF");
    }

    #[test]
    fn plain_text_is_escaped() {
        let raw = b"Subject: t\r\n\r\n<script>alert(1)</script> & coisas\r\n";
        let v = render(raw, false);
        assert!(!v.html.contains("<script>"));
        assert!(v.html.contains("&lt;script&gt;"));
        assert!(!v.has_remote_images);
    }

    #[test]
    fn garbage_does_not_panic() {
        let v = render(b"\x00\x01\xff", false);
        assert!(v.html.contains("Content-Security-Policy"));
    }

    #[test]
    fn blocked_extensions() {
        for name in ["a.exe", "b.EXE", "c.exe.", "d.ps1", "e.lnk", "f.js"] {
            assert!(is_blocked(name), "{name}");
        }
        for name in ["a.pdf", "b.docx", "semextensao", "exe"] {
            assert!(!is_blocked(name), "{name}");
        }
    }
}
```

Adicionar `pub mod message;` em `lib.rs`.

Run: `cd src-tauri && cargo test message`
Expected: FAIL (itens não definidos).

- [ ] **Step 2: Implementação** (acima dos testes)

```rust
use base64::Engine;
use mail_parser::{Address, Message, MessageParser, MimeHeaders};
use serde::Serialize;

const BLOCKED: &[&str] = &["exe", "bat", "cmd", "com", "js", "jse", "vbs", "vbe", "wsf", "ps1", "msi", "scr", "lnk", "hta", "cpl", "jar"];

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
    pub blocked: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct MessageView {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: Option<i64>,
    pub subject: String,
    pub html: String,
    pub has_remote_images: bool,
    pub attachments: Vec<AttachmentInfo>,
}

pub fn is_blocked(name: &str) -> bool {
    let name = name.trim_end_matches([' ', '.']);
    name.rsplit_once('.').is_some_and(|(_, ext)| BLOCKED.contains(&ext.to_ascii_lowercase().as_str()))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fmt_addr(a: Option<&Address>) -> String {
    a.map(|a| {
        a.iter()
            .map(|x| match (x.name(), x.address()) {
                (Some(n), Some(e)) => format!("{n} <{e}>"),
                (None, Some(e)) => e.to_string(),
                (Some(n), None) => n.to_string(),
                (None, None) => String::new(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
}

fn mime_of(part: &mail_parser::MessagePart) -> String {
    part.content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into())
}

fn wrap(body: &str, allow_remote_images: bool) -> String {
    let img = if allow_remote_images { "data: https:" } else { "data:" };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {img}; style-src 'unsafe-inline'"><base target="_blank"><style>body{{font-family:system-ui,sans-serif;margin:12px;overflow-wrap:anywhere}}pre{{white-space:pre-wrap}}</style></head><body>{body}</body></html>"#
    )
}

// ponytail: imagens cid são embutidas sem limite de tamanho; limitar se e-mails gigantes travarem a UI
fn inline_cids(html: &str, m: &Message) -> String {
    let mut out = html.to_string();
    for part in &m.parts {
        let Some(cid) = part.content_id() else { continue };
        let needle = format!("cid:{}", cid.trim_matches(['<', '>']));
        if out.contains(&needle) {
            let data = base64::engine::general_purpose::STANDARD.encode(part.contents());
            out = out.replace(&needle, &format!("data:{};base64,{data}", mime_of(part)));
        }
    }
    out
}

fn attachment_name(part: &mail_parser::MessagePart, i: usize) -> String {
    part.attachment_name().map(str::to_string).unwrap_or_else(|| format!("anexo-{}", i + 1))
}

pub fn render(raw: &[u8], allow_remote_images: bool) -> MessageView {
    let Some(m) = MessageParser::default().parse(raw) else {
        let body = format!("<pre>{}</pre>", escape(&String::from_utf8_lossy(raw)));
        return MessageView { html: wrap(&body, false), ..Default::default() };
    };
    let body = m
        .body_html(0)
        .map(|h| h.into_owned())
        .or_else(|| m.body_text(0).map(|t| format!("<pre>{}</pre>", escape(&t))))
        .unwrap_or_default();
    let body = inline_cids(&body, &m);
    let lower = body.to_ascii_lowercase();
    // ponytail: detecção por substring; um falso positivo só exibe o botão "Carregar imagens"
    let has_remote_images = ["src=\"http", "src='http", "src=http", "url(http", "url('http", "url(\"http"].iter().any(|p| lower.contains(p));
    let attachments = m
        .attachments()
        .enumerate()
        .map(|(i, p)| {
            let name = attachment_name(p, i);
            AttachmentInfo { index: i, blocked: is_blocked(&name), mime: mime_of(p), size: p.contents().len(), name }
        })
        .collect();
    MessageView {
        from: fmt_addr(m.from()),
        to: fmt_addr(m.to()),
        cc: fmt_addr(m.cc()),
        date: m.date().map(|d| d.to_timestamp()),
        subject: m.subject().unwrap_or("").to_string(),
        html: wrap(&body, allow_remote_images),
        has_remote_images,
        attachments,
    }
}

pub fn attachment(raw: &[u8], index: usize) -> Option<(String, Vec<u8>)> {
    let m = MessageParser::default().parse(raw)?;
    let part = m.attachments().nth(index)?;
    Some((attachment_name(part, index), part.contents().to_vec()))
}
```

Run: `cd src-tauri && cargo test message`
Expected: PASS (5 tests). Se `html_gets_csp_base_and_inline_cid` falhar porque a imagem `cid` aparece na lista de anexos, tudo bem — o teste só exige que `nota.pdf` e `virus.EXE` estejam presentes.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/message.rs src-tauri/src/lib.rs
git commit -m "feat(message): renderização isolada com CSP, imagens cid e anexos"
```

---

### Task 7: `commands.rs` + `lib.rs` — comandos Tauri e janela

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (versão final abaixo)

**Interfaces:**
- Consumes: tudo das Tasks 1–6.
- Produces (comandos chamados pelo front; nomes de argumentos JS em camelCase):

| Comando | Args | Retorno |
|---|---|---|
| `list_accounts` | — | `Account[]` |
| `save_account` | `input: AccountInput` | `Account` |
| `delete_account` | `id, deleteFiles: bool` | `null` |
| `test_connection` | `input: AccountInput` | `null` |
| `pick_folder` | — | `string \| null` |
| `run_backup` | `ids: string[]` | `BackupResult[]` |
| `cancel_backup` | — | `null` |
| `list_folders` | `accountId` | `FolderInfo[]` |
| `list_messages` | `accountId, query: Query` | `MessagePage` |
| `get_message` | `accountId, messageId, allowRemoteImages` | `MessageView` |
| `open_eml` | `accountId, messageId` | `null` |
| `save_attachment` | `accountId, messageId, index` | `bool` |
| `open_attachment` | `accountId, messageId, index` | `null` |
| `reindex_account` | `accountId` | `number` |
| `export_accounts` | `ids, password: string \| null` | `bool` |
| `pick_archive` | — | `string \| null` |
| `import_archive` | `path, password: string \| null` | `Account[]` |

Evento: `backup://progress` com `Progress`. `AccountInput` (campos snake_case no objeto): `{ id: string|null, name, email, host, port, username, password: string|null, dir: string|null }`. `BackupResult`: `{ account_id, report: SyncReport|null, error: {kind,message}|null }`.

- [ ] **Step 1: Implementação de `commands.rs`**

```rust
use crate::archive::{self, AccountManifest, ExportItem};
use crate::config::{self, Account, Config, LastRun};
use crate::error::{AppError, Result};
use crate::imap_sync::{self, Progress, SyncCtx, SyncReport};
use crate::index::{FolderInfo, Index, MessagePage, Query};
use crate::message::{self, MessageView};
use crate::store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
    pub data_dir: PathBuf,
    pub config: Mutex<Config>,
    pub cancel: AtomicBool,
    pub running: AtomicBool,
}

impl AppState {
    fn account(&self, id: &str) -> Result<Account> {
        self.config.lock().unwrap().get(id).cloned()
    }

    fn update<R>(&self, f: impl FnOnce(&mut Config) -> Result<R>) -> Result<R> {
        let mut c = self.config.lock().unwrap();
        let r = f(&mut c)?;
        c.save(&self.data_dir.join("config.json"))?;
        Ok(r)
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountInput {
    pub id: Option<String>,
    pub name: String,
    pub email: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub dir: Option<String>,
}

#[derive(Serialize)]
pub struct BackupResult {
    pub account_id: String,
    pub report: Option<SyncReport>,
    pub error: Option<AppError>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn other(e: impl std::fmt::Display) -> AppError {
    AppError::Other(e.to_string())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(other)?
}

fn open_index(acc: &Account) -> Result<Index> {
    std::fs::create_dir_all(&acc.dir)?;
    let (mut index, needs_reindex) = Index::open(&acc.dir.join("index.sqlite"))?;
    if needs_reindex {
        index.reindex(&acc.dir.join("mail"), &mut |_| {})?;
    }
    Ok(index)
}

fn message_path(acc: &Account, message_id: i64) -> Result<PathBuf> {
    let rel = open_index(acc)?.message_file(message_id)?;
    store::resolve(&acc.dir.join("mail"), &rel)
}

fn password_for(input: &AccountInput) -> Result<String> {
    match (&input.password, &input.id) {
        (Some(p), _) if !p.is_empty() => Ok(p.clone()),
        (_, Some(id)) => config::get_password(id),
        _ => Err(other("informe a senha")),
    }
}

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>> {
    Ok(state.config.lock().unwrap().accounts.clone())
}

#[tauri::command]
pub async fn save_account(state: State<'_, AppState>, input: AccountInput) -> Result<Account> {
    let acc = state.update(|c| {
        if let Some(id) = &input.id {
            let a = c.get_mut(id)?;
            a.name = input.name.clone();
            a.email = input.email.clone();
            a.host = input.host.clone();
            a.port = input.port;
            a.username = input.username.clone();
            return Ok(a.clone());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = input.dir.as_ref().map(PathBuf::from).unwrap_or_else(|| state.data_dir.join("accounts").join(&id));
        let a = Account {
            id, name: input.name.clone(), email: input.email.clone(), host: input.host.clone(), port: input.port,
            username: input.username.clone(), dir, has_credentials: false, last_run: None,
        };
        c.accounts.push(a.clone());
        Ok(a)
    })?;
    match input.password.as_deref().filter(|p| !p.is_empty()) {
        Some(pw) => {
            config::set_password(&acc.id, pw)?;
            state.update(|c| {
                let a = c.get_mut(&acc.id)?;
                a.has_credentials = true;
                Ok(a.clone())
            })
        }
        None => Ok(acc),
    }
}

#[tauri::command]
pub async fn delete_account(state: State<'_, AppState>, id: String, delete_files: bool) -> Result<()> {
    let acc = state.account(&id)?;
    state.update(|c| {
        c.accounts.retain(|a| a.id != id);
        Ok(())
    })?;
    config::delete_password(&id)?;
    if delete_files {
        // só apaga o que o app criou; a pasta pode ter sido escolhida pelo usuário
        let mail = acc.dir.join("mail");
        if mail.exists() {
            std::fs::remove_dir_all(mail)?;
        }
        for f in ["index.sqlite", "index.sqlite-wal", "index.sqlite-shm"] {
            let _ = std::fs::remove_file(acc.dir.join(f));
        }
        let _ = std::fs::remove_dir(&acc.dir); // só remove se estiver vazia
    }
    Ok(())
}

#[tauri::command]
pub async fn test_connection(input: AccountInput) -> Result<()> {
    let pw = password_for(&input)?;
    let mut s = imap_sync::connect_tls(&input.host, input.port, &input.username, &pw).await?;
    let _ = s.logout().await;
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>> {
    Ok(app.dialog().file().blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned()))
}

async fn backup_one(app: &AppHandle, state: &AppState, id: &str) -> Result<SyncReport> {
    let acc = state.account(id)?;
    if !acc.has_credentials {
        return Err(other("conta sem credenciais (somente leitura)"));
    }
    let pw = config::get_password(id)?;
    let mut index = open_index(&acc)?;
    let mail_root = acc.dir.join("mail");
    let progress = |p: Progress| {
        let _ = app.emit("backup://progress", p);
    };
    let ctx = SyncCtx { account_id: id, mail_root: &mail_root, cancel: &state.cancel, progress: &progress };
    let (host, user, pass, port) = (acc.host.as_str(), acc.username.as_str(), pw.as_str(), acc.port);
    imap_sync::sync_account(move || imap_sync::connect_tls(host, port, user, pass), &ctx, &mut index).await
}

#[tauri::command]
pub async fn run_backup(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> Result<Vec<BackupResult>> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(other("já existe um backup em andamento"));
    }
    state.cancel.store(false, Ordering::SeqCst);
    let mut results = Vec::new();
    for id in ids {
        if state.cancel.load(Ordering::SeqCst) {
            break;
        }
        let outcome = backup_one(&app, &state, &id).await;
        let last = match &outcome {
            Ok(r) => LastRun { at: now(), new_messages: r.new_messages, failed_folders: r.failed_folders.clone(), error: None },
            Err(e) => LastRun { at: now(), new_messages: 0, failed_folders: vec![], error: Some(e.to_string()) },
        };
        let _ = state.update(|c| {
            c.get_mut(&id)?.last_run = Some(last);
            Ok(())
        });
        results.push(match outcome {
            Ok(r) => BackupResult { account_id: id, report: Some(r), error: None },
            Err(e) => BackupResult { account_id: id, report: None, error: Some(e) },
        });
    }
    state.running.store(false, Ordering::SeqCst);
    Ok(results)
}

#[tauri::command]
pub async fn cancel_backup(state: State<'_, AppState>) -> Result<()> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn list_folders(state: State<'_, AppState>, account_id: String) -> Result<Vec<FolderInfo>> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.list_folders()).await
}

#[tauri::command]
pub async fn list_messages(state: State<'_, AppState>, account_id: String, query: Query) -> Result<MessagePage> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.list_messages(&query)).await
}

#[tauri::command]
pub async fn get_message(state: State<'_, AppState>, account_id: String, message_id: i64, allow_remote_images: bool) -> Result<MessageView> {
    let acc = state.account(&account_id)?;
    blocking(move || Ok(message::render(&std::fs::read(message_path(&acc, message_id)?)?, allow_remote_images))).await
}

#[tauri::command]
pub async fn open_eml(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64) -> Result<()> {
    let path = message_path(&state.account(&account_id)?, message_id)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(other)
}

fn load_attachment(state: &AppState, account_id: &str, message_id: i64, index: usize) -> Result<(String, Vec<u8>)> {
    let raw = std::fs::read(message_path(&state.account(account_id)?, message_id)?)?;
    message::attachment(&raw, index).ok_or_else(|| other("anexo não encontrado"))
}

#[tauri::command]
pub async fn save_attachment(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64, index: usize) -> Result<bool> {
    let (name, bytes) = load_attachment(&state, &account_id, message_id, index)?;
    let Some(dest) = app.dialog().file().set_file_name(store::sanitize_component(&name)).blocking_save_file() else {
        return Ok(false);
    };
    std::fs::write(dest.into_path().map_err(other)?, bytes)?;
    Ok(true)
}

#[tauri::command]
pub async fn open_attachment(app: AppHandle, state: State<'_, AppState>, account_id: String, message_id: i64, index: usize) -> Result<()> {
    let (name, bytes) = load_attachment(&state, &account_id, message_id, index)?;
    if message::is_blocked(&name) {
        return Err(other("por segurança, este tipo de arquivo só pode ser salvo, não aberto"));
    }
    let path = std::env::temp_dir().join("mail-backup").join(uuid::Uuid::new_v4().to_string()).join(store::sanitize_component(&name));
    store::write_atomic(&path, &bytes)?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(other)
}

#[tauri::command]
pub async fn reindex_account(state: State<'_, AppState>, account_id: String) -> Result<u64> {
    let acc = state.account(&account_id)?;
    blocking(move || open_index(&acc)?.reindex(&acc.dir.join("mail"), &mut |_| {})).await
}

#[tauri::command]
pub async fn export_accounts(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>, password: Option<String>) -> Result<bool> {
    let accounts = ids.iter().map(|id| state.account(id)).collect::<Result<Vec<_>>>()?;
    let Some(dest) = app.dialog().file().add_filter("Backup de e-mails", &["zip"]).set_file_name("backup-emails.zip").blocking_save_file() else {
        return Ok(false);
    };
    let dest = dest.into_path().map_err(other)?;
    blocking(move || {
        let manifests = accounts
            .iter()
            .map(|a| {
                Ok(AccountManifest {
                    name: a.name.clone(), email: a.email.clone(), host: a.host.clone(), port: a.port,
                    username: a.username.clone(), folders: open_index(a)?.folders_state()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let items: Vec<ExportItem> = accounts.iter().zip(manifests).map(|(a, manifest)| ExportItem { manifest, dir: &a.dir }).collect();
        archive::export(&dest, &items, password.as_deref().filter(|p| !p.is_empty()))
    })
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn pick_archive(app: AppHandle) -> Result<Option<String>> {
    Ok(app
        .dialog()
        .file()
        .add_filter("Backup de e-mails", &["zip"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}

#[tauri::command]
pub async fn import_archive(state: State<'_, AppState>, path: String, password: Option<String>) -> Result<Vec<Account>> {
    let root = state.data_dir.join("accounts");
    let imported = blocking(move || {
        archive::import(Path::new(&path), &root, password.as_deref().filter(|p| !p.is_empty()), || uuid::Uuid::new_v4().to_string())
    })
    .await?;
    let mut added = Vec::new();
    for imp in imported {
        let id = imp.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let m = imp.manifest;
        let email = m.as_ref().map_or_else(|| imp.top.clone(), |m| m.email.clone());
        let exists = state.config.lock().unwrap().accounts.iter().any(|a| a.email.eq_ignore_ascii_case(&email));
        let name = if exists { format!("{email} (importado)") } else { m.as_ref().map_or_else(|| email.clone(), |m| m.name.clone()) };
        let acc = Account {
            id, name, email,
            host: m.as_ref().map_or_else(String::new, |m| m.host.clone()),
            port: m.as_ref().map_or(993, |m| m.port),
            username: m.as_ref().map_or_else(String::new, |m| m.username.clone()),
            dir: imp.dir.clone(), has_credentials: false, last_run: None,
        };
        let folders = m.map(|m| m.folders).unwrap_or_default();
        let dir = acc.dir.clone();
        blocking(move || {
            let (mut index, _) = Index::open(&dir.join("index.sqlite"))?;
            index.restore_folders(&folders)?;
            index.reindex(&dir.join("mail"), &mut |_| {})?;
            Ok(())
        })
        .await?;
        state.update(|c| {
            c.accounts.push(acc.clone());
            Ok(())
        })?;
        added.push(acc);
    }
    Ok(added)
}
```

- [ ] **Step 2: `lib.rs` final**

```rust
pub mod archive;
pub mod commands;
pub mod config;
pub mod error;
pub mod imap_sync;
pub mod index;
pub mod message;
pub mod store;

use commands::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().data_dir()?.join("mail-backup");
            std::fs::create_dir_all(&data_dir)?;
            let config = config::Config::load(&data_dir.join("config.json"))?;
            app.manage(AppState { data_dir, config: Mutex::new(config), cancel: AtomicBool::new(false), running: AtomicBool::new(false) });
            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
                .title("Mail Backup")
                .inner_size(1200.0, 800.0)
                .on_new_window(move |url, _| {
                    // links do e-mail (iframe com <base target="_blank">) abrem no navegador padrão
                    if matches!(url.scheme(), "http" | "https" | "mailto") {
                        let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                    }
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::save_account,
            commands::delete_account,
            commands::test_connection,
            commands::pick_folder,
            commands::run_backup,
            commands::cancel_backup,
            commands::list_folders,
            commands::list_messages,
            commands::get_message,
            commands::open_eml,
            commands::save_attachment,
            commands::open_attachment,
            commands::reindex_account,
            commands::export_accounts,
            commands::pick_archive,
            commands::import_archive,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o aplicativo");
}
```

- [ ] **Step 3: Compilar e rodar todos os testes**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: build sem erros; todos os testes unitários PASS. Se o compilador reclamar que o future de `run_backup` não é `Send`, o culpado provável é algo `!Send` vivo entre `.await`s em `sync_folder`; mover a criação desse valor para dentro de um bloco `{}` sem `.await`.

Run: `npm run tauri dev`
Expected: abre a janela "Mail Backup" com o `<h1>` provisório; no console do DevTools (F12), `await window.__TAURI_INTERNALS__.invoke('list_accounts')` retorna `[]`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: comandos Tauri, estado do app e janela com links externos"
```

---

### Task 8: Frontend — API e tela de Contas

**Files:**
- Create: `src/lib/api.js`, `src/lib/Accounts.svelte`
- Modify: `src/App.svelte`

**Interfaces:**
- Consumes: comandos da Task 7.
- Produces: `api` (objeto com um método por comando + `onProgress(cb)`), `errMsg(e)`; `App.svelte` alterna entre `<Accounts onbrowse={(account) => ...}>` e `<Browser {account} />` (Browser vem na Task 9 — até lá, mostrar placeholder).

- [ ] **Step 1: `src/lib/api.js`**

```js
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export const api = {
  listAccounts: () => invoke('list_accounts'),
  saveAccount: (input) => invoke('save_account', { input }),
  deleteAccount: (id, deleteFiles) => invoke('delete_account', { id, deleteFiles }),
  testConnection: (input) => invoke('test_connection', { input }),
  pickFolder: () => invoke('pick_folder'),
  runBackup: (ids) => invoke('run_backup', { ids }),
  cancelBackup: () => invoke('cancel_backup'),
  listFolders: (accountId) => invoke('list_folders', { accountId }),
  listMessages: (accountId, query) => invoke('list_messages', { accountId, query }),
  getMessage: (accountId, messageId, allowRemoteImages) => invoke('get_message', { accountId, messageId, allowRemoteImages }),
  openEml: (accountId, messageId) => invoke('open_eml', { accountId, messageId }),
  saveAttachment: (accountId, messageId, index) => invoke('save_attachment', { accountId, messageId, index }),
  openAttachment: (accountId, messageId, index) => invoke('open_attachment', { accountId, messageId, index }),
  reindex: (accountId) => invoke('reindex_account', { accountId }),
  exportAccounts: (ids, password) => invoke('export_accounts', { ids, password }),
  pickArchive: () => invoke('pick_archive'),
  importArchive: (path, password) => invoke('import_archive', { path, password }),
  onProgress: (cb) => listen('backup://progress', (e) => cb(e.payload)),
}

export const errMsg = (e) => (e && e.message) || String(e)
export const fmtDate = (ts) => (ts ? new Date(ts * 1000).toLocaleString('pt-BR') : '')
```

- [ ] **Step 2: `src/lib/Accounts.svelte`**

```svelte
<script>
  import { api, errMsg, fmtDate } from './api.js'

  let { onbrowse } = $props()
  let accounts = $state([])
  let error = $state('')
  let info = $state('')
  let form = $state(null)
  let running = $state(false)
  let progress = $state(null)
  let selected = $state([])
  let exportPw = $state('')
  let pendingImport = $state(null)
  let importPw = $state('')

  const blank = () => ({ id: null, name: '', email: '', host: 'imap.uhserver.com', port: 993, username: '', password: '', dir: null })
  const nameOf = (id) => accounts.find((a) => a.id === id)?.name ?? id
  const input = () => ({ ...form, port: Number(form.port), password: form.password || null })

  async function refresh() {
    try { accounts = await api.listAccounts() } catch (e) { error = errMsg(e) }
  }
  refresh()

  $effect(() => {
    const unlisten = api.onProgress((p) => (progress = p))
    return () => unlisten.then((f) => f())
  })

  async function save() {
    error = ''
    try { await api.saveAccount(input()); form = null; await refresh() } catch (e) { error = errMsg(e) }
  }

  async function test() {
    error = ''; info = ''
    try { await api.testConnection(input()); info = 'Conexão OK' } catch (e) { error = errMsg(e) }
  }

  async function pickDir() {
    const d = await api.pickFolder()
    if (d) form.dir = d
  }

  async function backup(ids) {
    running = true; error = ''; info = ''
    try {
      const res = await api.runBackup(ids)
      info = res.map((r) => r.error
        ? `${nameOf(r.account_id)}: ${r.error.message}`
        : `${nameOf(r.account_id)}: ${r.report.new_messages} novas` +
          (r.report.failed_folders.length ? `, falharam: ${r.report.failed_folders.join(', ')}` : '') +
          (r.report.cancelled ? ' (cancelado)' : '')).join(' · ')
    } catch (e) { error = errMsg(e) } finally { running = false; progress = null; refresh() }
  }

  async function remove(a) {
    if (!confirm(`Remover a conta ${a.email}?`)) return
    const files = confirm('Apagar também os e-mails salvos no disco?')
    try { await api.deleteAccount(a.id, files); refresh() } catch (e) { error = errMsg(e) }
  }

  async function exportSelected() {
    error = ''; info = ''
    try { if (await api.exportAccounts(selected, exportPw || null)) info = 'Backup exportado.' } catch (e) { error = errMsg(e) }
  }

  async function doImport(path, password) {
    error = ''; info = ''
    try {
      const added = await api.importArchive(path, password)
      info = `${added.length} conta(s) importada(s).`
      pendingImport = null; importPw = ''
      refresh()
    } catch (e) {
      if (e.kind === 'WrongPassword') { pendingImport = path; error = password ? 'Senha incorreta.' : 'Este arquivo tem senha.' }
      else error = errMsg(e)
    }
  }

  async function startImport() {
    const path = await api.pickArchive()
    if (path) doImport(path, null)
  }
</script>

<section class="accounts">
  <div class="toolbar">
    <button onclick={() => (form = blank())} disabled={running}>Adicionar conta</button>
    <button onclick={() => backup(accounts.filter((a) => a.has_credentials).map((a) => a.id))} disabled={running || !accounts.length}>Backup de todas</button>
    {#if running}<button onclick={() => api.cancelBackup()}>Cancelar</button>{/if}
    <button onclick={startImport} disabled={running}>Importar backup…</button>
  </div>

  {#if progress}<p class="progress">{progress.folder}: {progress.done}/{progress.total}</p>{/if}
  {#if error}<p class="err">{error}</p>{/if}
  {#if info}<p class="info">{info}</p>{/if}

  {#if pendingImport}
    <div class="inline">
      <input type="password" placeholder="Senha do arquivo" bind:value={importPw} />
      <button onclick={() => doImport(pendingImport, importPw)}>Importar</button>
      <button onclick={() => { pendingImport = null; error = '' }}>Cancelar</button>
    </div>
  {/if}

  {#if form}
    <form class="card" onsubmit={(e) => { e.preventDefault(); save() }}>
      <label>Nome <input bind:value={form.name} required /></label>
      <label>E-mail <input type="email" bind:value={form.email} required /></label>
      <label>Servidor IMAP <input bind:value={form.host} required /></label>
      <label>Porta <input type="number" bind:value={form.port} required /></label>
      <label>Usuário <input bind:value={form.username} required /></label>
      <label>Senha <input type="password" bind:value={form.password} placeholder={form.id ? '(manter a atual)' : ''} /></label>
      {#if !form.id}
        <label>Pasta do backup <span>{form.dir ?? '(padrão do app)'}</span> <button type="button" onclick={pickDir}>Escolher…</button></label>
      {/if}
      <div class="inline">
        <button type="submit">Salvar</button>
        <button type="button" onclick={test}>Testar conexão</button>
        <button type="button" onclick={() => (form = null)}>Cancelar</button>
      </div>
    </form>
  {/if}

  <table>
    <thead><tr><th></th><th>Conta</th><th>Último backup</th><th></th></tr></thead>
    <tbody>
      {#each accounts as a (a.id)}
        <tr>
          <td><input type="checkbox" bind:group={selected} value={a.id} /></td>
          <td><b>{a.name}</b><br /><small>{a.email}{a.has_credentials ? '' : ' · somente leitura'}</small></td>
          <td>
            {#if a.last_run}
              {fmtDate(a.last_run.at)} — {a.last_run.error ?? `${a.last_run.new_messages} novas`}
              {#if a.last_run.failed_folders.length}<br /><small class="err">Falharam: {a.last_run.failed_folders.join(', ')}</small>{/if}
            {:else}—{/if}
          </td>
          <td class="actions">
            <button onclick={() => onbrowse(a)}>Abrir</button>
            {#if a.has_credentials}<button onclick={() => backup([a.id])} disabled={running}>Backup</button>{/if}
            <button onclick={() => (form = { ...blank(), ...a, password: '' })} disabled={running}>Editar</button>
            <button onclick={() => remove(a)} disabled={running}>Remover</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#if selected.length}
    <div class="inline">
      <input type="password" placeholder="Senha do arquivo (opcional)" bind:value={exportPw} />
      <button onclick={exportSelected} disabled={running}>Exportar {selected.length} conta(s)…</button>
    </div>
  {/if}
</section>
```

- [ ] **Step 3: `src/App.svelte`**

```svelte
<script>
  import Accounts from './lib/Accounts.svelte'

  let account = $state(null)
</script>

<header>
  <h1>Mail Backup</h1>
  {#if account}<button onclick={() => (account = null)}>← Contas</button> <span>{account.name}</span>{/if}
</header>

{#if account}
  <p>Navegador (Task 9)</p>
{:else}
  <Accounts onbrowse={(a) => (account = a)} />
{/if}

<style>
  :global(body) { margin: 0; font: 14px system-ui, sans-serif; color: #1d1d1f; background: #fafafa; }
  :global(button) { padding: 4px 10px; cursor: pointer; }
  :global(input) { padding: 4px 6px; }
  :global(.err) { color: #b00020; }
  :global(.info) { color: #1b5e20; }
  :global(.inline) { display: flex; gap: 8px; align-items: center; margin: 8px 0; }
  :global(.card) { display: grid; gap: 6px; max-width: 480px; padding: 12px; background: #fff; border: 1px solid #ddd; margin: 8px 0; }
  :global(.card label) { display: grid; grid-template-columns: 130px 1fr; gap: 8px; align-items: center; }
  :global(table) { width: 100%; border-collapse: collapse; }
  :global(td), :global(th) { padding: 6px; border-bottom: 1px solid #eee; text-align: left; vertical-align: top; }
  :global(.actions) { white-space: nowrap; }
  header { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid #ddd; background: #fff; }
  header h1 { font-size: 16px; margin: 0; }
  :global(.accounts) { padding: 12px; }
</style>
```

- [ ] **Step 4: Verificação manual**

Run: `npm run tauri dev`
Expected, nesta ordem:
1. Lista vazia; "Adicionar conta" abre o formulário com `imap.uhserver.com`/993.
2. Salvar uma conta com senha → aparece na tabela sem "somente leitura"; `%APPDATA%\mail-backup\config.json` **não** contém a senha.
3. "Testar conexão" com senha errada → mensagem "usuário ou senha inválidos".
4. "Editar" → salvar sem senha mantém `has_credentials`.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.js src/lib/Accounts.svelte src/App.svelte
git commit -m "feat(ui): tela de contas com backup, exportar e importar"
```

---

### Task 9: Frontend — Navegador e Leitor

**Files:**
- Create: `src/lib/Browser.svelte`, `src/lib/MessageList.svelte`, `src/lib/Reader.svelte`
- Modify: `src/App.svelte` (trocar o placeholder por `<Browser {account} />`)

**Interfaces:**
- Consumes: `api.listFolders`, `api.listMessages`, `api.getMessage`, `api.openEml`, `api.saveAttachment`, `api.openAttachment`, `api.reindex`, `errMsg`, `fmtDate`
- Produces: `<Browser account={Account} />`, `<MessageList accountId folderId text dateFrom dateTo selectedId onselect />` (datas em segundos Unix ou `null`), `<Reader accountId messageId />`

- [ ] **Step 1: `src/lib/MessageList.svelte`** (scroll virtual com páginas de 200)

```svelte
<script>
  import { untrack } from 'svelte'
  import { api, errMsg, fmtDate } from './api.js'

  let { accountId, folderId, text, dateFrom, dateTo, selectedId, onselect } = $props()
  const ROW = 52
  const PAGE = 200
  let total = $state(0)
  let pages = $state({})
  let scrollTop = $state(0)
  let height = $state(600)
  let error = $state('')
  let box
  let gen = 0

  $effect(() => {
    accountId; folderId; text; dateFrom; dateTo
    untrack(() => {
      gen++
      pages = {}
      total = 0
      scrollTop = 0
      if (box) box.scrollTop = 0
      load(0)
    })
  })

  async function load(p) {
    if (pages[p] !== undefined) return
    pages[p] = null
    const g = gen
    try {
      const res = await api.listMessages(accountId, { folder_id: folderId, text: text || null, date_from: dateFrom, date_to: dateTo, offset: p * PAGE, limit: PAGE })
      if (g !== gen) return
      pages[p] = res.rows
      total = res.total
    } catch (e) { error = errMsg(e) }
  }

  let first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 5))
  let last = $derived(Math.min(total, Math.ceil((scrollTop + height) / ROW) + 5))

  $effect(() => {
    const a = first, b = last
    untrack(() => {
      for (let p = Math.floor(a / PAGE); p <= Math.floor(Math.max(a, b - 1) / PAGE); p++) load(p)
    })
  })

  const row = (i) => pages[Math.floor(i / PAGE)]?.[i % PAGE] ?? null
</script>

<div class="list" bind:this={box} bind:clientHeight={height} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
  <div class="spacer" style="height:{total * ROW}px">
    {#each Array.from({ length: Math.max(0, last - first) }, (_, k) => first + k) as i (i)}
      {@const m = row(i)}
      <button class="row" class:sel={m && m.id === selectedId} style="top:{i * ROW}px;height:{ROW}px" onclick={() => m && onselect(m.id)}>
        {#if m}
          <span class="top"><b>{m.from_name || m.from_addr || '(sem remetente)'}</b><small>{fmtDate(m.date)}</small></span>
          <span class="subj">{m.has_attach ? '📎 ' : ''}{m.subject || '(sem assunto)'}</span>
        {:else}
          <span class="subj">…</span>
        {/if}
      </button>
    {/each}
  </div>
  {#if !total}<p class="empty">Nenhuma mensagem</p>{/if}
</div>
{#if error}<p class="err">{error}</p>{/if}

<style>
  .list { height: 100%; overflow-y: auto; position: relative; }
  .spacer { position: relative; }
  .row { position: absolute; left: 0; right: 0; display: grid; gap: 2px; text-align: left; border: 0; border-bottom: 1px solid #eee; background: #fff; padding: 6px 10px; }
  .row.sel { background: #e3f2fd; }
  .top { display: flex; justify-content: space-between; gap: 8px; overflow: hidden; white-space: nowrap; }
  .subj { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: #555; }
  .empty { padding: 12px; color: #888; }
</style>
```

- [ ] **Step 2: `src/lib/Reader.svelte`**

```svelte
<script>
  import { api, errMsg, fmtDate } from './api.js'

  let { accountId, messageId } = $props()
  let view = $state(null)
  let remote = $state(false)
  let error = $state('')

  async function load(a, m, allowRemote) {
    error = ''
    try { view = await api.getMessage(a, m, allowRemote) } catch (e) { error = errMsg(e); view = null }
  }

  $effect(() => {
    const a = accountId, m = messageId
    remote = false
    load(a, m, false)
  })

  const kb = (n) => (n < 1024 ? `${n} B` : `${Math.round(n / 1024)} KB`)
  const act = (p) => p.catch((e) => (error = errMsg(e)))
</script>

{#if error}<p class="err">{error}</p>{/if}
{#if view}
  <div class="reader">
    <div class="head">
      <h2>{view.subject || '(sem assunto)'}</h2>
      <div>De: {view.from}</div>
      <div>Para: {view.to}</div>
      {#if view.cc}<div>Cc: {view.cc}</div>{/if}
      <div><small>{fmtDate(view.date)}</small></div>
      <div class="inline">
        {#if view.has_remote_images && !remote}
          <button onclick={() => { remote = true; load(accountId, messageId, true) }}>Carregar imagens</button>
        {/if}
        <button onclick={() => act(api.openEml(accountId, messageId))}>Abrir no programa de e-mail</button>
      </div>
      {#if view.attachments.length}
        <ul class="att">
          {#each view.attachments as a (a.index)}
            <li>
              {a.name} <small>({kb(a.size)})</small>
              <button onclick={() => act(api.saveAttachment(accountId, messageId, a.index))}>Salvar como…</button>
              {#if !a.blocked}<button onclick={() => act(api.openAttachment(accountId, messageId, a.index))}>Abrir</button>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <iframe title="Conteúdo do e-mail" sandbox="allow-popups allow-popups-to-escape-sandbox" srcdoc={view.html}></iframe>
  </div>
{/if}

<style>
  .reader { display: flex; flex-direction: column; height: 100%; }
  .head { padding: 10px 12px; border-bottom: 1px solid #ddd; background: #fff; }
  .head h2 { font-size: 16px; margin: 0 0 6px; }
  .att { margin: 6px 0 0; padding-left: 18px; }
  iframe { flex: 1; border: 0; width: 100%; background: #fff; }
</style>
```

- [ ] **Step 3: `src/lib/Browser.svelte`**

```svelte
<script>
  import { api, errMsg } from './api.js'
  import MessageList from './MessageList.svelte'
  import Reader from './Reader.svelte'

  let { account } = $props()
  let folders = $state([])
  let folderId = $state(null)
  let search = $state('')
  let text = $state('')
  let from = $state('')
  let to = $state('')
  let selectedId = $state(null)
  let error = $state('')
  let timer

  $effect(() => {
    const q = search
    clearTimeout(timer)
    timer = setTimeout(() => (text = q.trim()), 300)
  })

  async function loadFolders() {
    try { folders = await api.listFolders(account.id) } catch (e) { error = errMsg(e) }
  }

  $effect(() => { account.id; loadFolders() })

  async function reindex() {
    error = ''
    try { await api.reindex(account.id); await loadFolders(); folderId = null; selectedId = null } catch (e) { error = errMsg(e) }
  }

  // datas locais do <input type="date">; "até" inclui o dia inteiro
  const ts = (d, plusDay = 0) => (d ? Math.floor(new Date(`${d}T00:00:00`).getTime() / 1000) + plusDay * 86400 : null)

  const depth = (p) => p.split('/').length - 1
  const leaf = (p) => p.split('/').at(-1)
</script>

<div class="browser">
  <aside>
    <button class:sel={folderId === null} onclick={() => { folderId = null; selectedId = null }}>Todas as pastas</button>
    {#each folders as f (f.id)}
      <button class:sel={folderId === f.id} style="padding-left:{10 + depth(f.path) * 14}px" onclick={() => { folderId = f.id; selectedId = null }}>
        {leaf(f.path)} <small>{f.count}</small>
      </button>
    {/each}
    <button class="reindex" onclick={reindex}>Reindexar</button>
    {#if error}<p class="err">{error}</p>{/if}
  </aside>
  <section>
    <input class="search" placeholder="Buscar remetente ou assunto…" bind:value={search} />
    <div class="dates"><label>De <input type="date" bind:value={from} /></label><label>Até <input type="date" bind:value={to} /></label></div>
    <div class="listwrap">
      <MessageList accountId={account.id} {folderId} {text} dateFrom={ts(from)} dateTo={ts(to, 1)} {selectedId} onselect={(id) => (selectedId = id)} />
    </div>
  </section>
  <main>
    {#if selectedId}<Reader accountId={account.id} messageId={selectedId} />{:else}<p class="empty">Selecione uma mensagem</p>{/if}
  </main>
</div>

<style>
  .browser { display: grid; grid-template-columns: 220px 380px 1fr; height: calc(100vh - 45px); }
  aside { overflow-y: auto; border-right: 1px solid #ddd; display: flex; flex-direction: column; background: #f3f3f3; }
  aside button { text-align: left; border: 0; background: none; padding: 6px 10px; display: flex; justify-content: space-between; }
  aside button.sel { background: #dde7f3; }
  .reindex { margin-top: auto; color: #555; }
  section { display: flex; flex-direction: column; border-right: 1px solid #ddd; min-height: 0; }
  .search { margin: 8px 8px 4px; }
  .dates { display: flex; gap: 8px; margin: 0 8px 8px; font-size: 12px; }
  .listwrap { flex: 1; min-height: 0; }
  main { min-width: 0; min-height: 0; }
  .empty { padding: 16px; color: #888; }
</style>
```

- [ ] **Step 4: Ligar no `App.svelte`**

Em `src/App.svelte`: adicionar `import Browser from './lib/Browser.svelte'` e trocar `<p>Navegador (Task 9)</p>` por `<Browser {account} />`.

- [ ] **Step 5: Verificação manual** (precisa de uma conta com mensagens: rode antes o Step 1 da Task 10 com a conta UOL real — o GreenMail não serve aqui porque o app só conecta via TLS)

Run: `npm run tauri dev`
Expected:
1. "Abrir" numa conta mostra a árvore de pastas com contagens.
2. Rolar uma pasta com milhares de mensagens é fluido; só ~30 linhas existem no DOM (DevTools → Elements).
3. Busca por parte do assunto/remetente filtra após ~300 ms; `50%` busca literalmente. Os campos "De/Até" restringem por data (o dia do "Até" entra inteiro).
4. E-mail HTML aparece no iframe; imagens remotas só após "Carregar imagens".
5. **Spike dos links:** clicar num link dentro do e-mail abre o **navegador padrão** e não navega o app. Se não abrir nada, trocar o `sandbox` para `"allow-popups allow-popups-to-escape-sandbox allow-top-navigation-by-user-activation"` não é aceitável (navegaria o app); em vez disso, aplicar o fallback da spec: em `message.rs::wrap`, acrescentar ao `<style>` `a{pointer-events:none;color:inherit}` e registrar no plano de QA que links aparecem como texto.
6. Anexo `.pdf` → "Salvar como…" grava o arquivo; "Abrir" abre no leitor padrão. Anexo `.exe` não tem botão "Abrir".
7. "Abrir no programa de e-mail" abre o `.eml` no cliente padrão (ou pergunta qual app usar).

- [ ] **Step 6: Commit**

```bash
git add src/lib/Browser.svelte src/lib/MessageList.svelte src/lib/Reader.svelte src/App.svelte
git commit -m "feat(ui): navegador de pastas, lista virtual, busca e leitor isolado"
```

---

### Task 10: QA real (UOL), build de release e peso

**Files:**
- Create: `docs/qa-checklist.md`

- [ ] **Step 1: Backup real UOL**

Com `npm run tauri dev`, cadastrar uma conta real (`imap.uhserver.com`, 993, usuário = e-mail completo). Verificar e anotar no `docs/qa-checklist.md`:
1. "Testar conexão" OK; senha errada → "usuário ou senha inválidos".
2. Backup completo: todas as pastas do webmail aparecem em `<dir>/mail/` com nomes em português corretos (Enviados, Lixeira, Rascunhos, acentos).
3. No webmail, uma mensagem **não lida** continua não lida após o backup.
4. Segundo backup imediato → "0 novas" e rápido.
5. Enviar um e-mail para a conta, rodar backup → "1 novas".
6. Durante um backup grande, desligar o Wi-Fi por ~10 s e religar → o backup continua ou a pasta aparece em "Falharam"; rodar de novo completa sem duplicar (conferir contagem no navegador vs. webmail).
7. "Cancelar" durante o backup → para entre lotes; rodar de novo completa.
8. Navegar a conta **enquanto** o backup roda → sem erro "database is locked".

- [ ] **Step 2: Exportar/importar real**

1. Selecionar a conta, senha `teste123`, exportar → abrir o `.zip` no 7-Zip: pede senha; com a senha, os `.eml` abrem.
2. "Importar backup…" do mesmo arquivo → pede senha; senha errada → "Senha incorreta."; correta → conta "`<email>` (importado)", somente leitura, mesma contagem de mensagens.

- [ ] **Step 3: Release e peso**

Run: `npx tauri build --no-bundle`
Expected: gera `src-tauri/target/release/mail-backup.exe`. Anotar no checklist o tamanho (`ls -la src-tauri/target/release/mail-backup.exe`) e a RAM do processo no Gerenciador de Tarefas em repouso e durante um backup (somando `mail-backup.exe` + processos `msedgewebview2.exe` filhos).

Run: `npx tauri build`
Expected: gera instaladores em `src-tauri/target/release/bundle/` (NSIS/MSI). Instalar e repetir o item 1 do Step 1.

- [ ] **Step 4: Commit**

```bash
git add docs/qa-checklist.md
git commit -m "docs: checklist de QA com resultados do teste real"
```
