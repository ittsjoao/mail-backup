# Backup paralelo e nova tela de Contas — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rodar o backup de várias contas ao mesmo tempo (limite configurável, padrão 5) e mostrar o estado de cada conta numa tela de Contas em cards.

**Architecture:** `run_backup` passa as contas por um helper `run_limited` (`buffer_unordered(limit)`), e cada conta roda numa task própria (`tauri::async_runtime::spawn`). Um evento novo `backup://status` avisa início e fim de cada conta; `backup://progress` continua igual. No front, `Accounts.svelte` mantém um mapa `account_id → status` e renderiza `AccountCard.svelte`; o formulário vira o modal `AccountForm.svelte`. Estilo com tokens CSS em `App.svelte`, sem dependências novas.

**Tech Stack:** Tauri 2, Rust (tokio, futures, rusqlite), Svelte 5 (runes), Vite. WebView2 (Chromium) — `<dialog>`, `popover` e CSS anchor positioning são nativos.

**Spec:** `docs/superpowers/specs/2026-10-08-backup-paralelo-design.md`

## Global Constraints

- Limite de contas simultâneas: padrão `5`, faixa `1..=20`, mensagem de erro exata: `limite deve ficar entre 1 e 20`.
- `config.json` antigo (sem `max_parallel`) precisa carregar com `5`.
- Evento novo: `backup://status`, payload `{ account_id, state: "running" | "done" | "failed" | "cancelled", new_messages?, error? }`.
- Nenhuma dependência nova (nem Rust, nem npm).
- `imap_sync.rs` não muda.
- Commits: `tipo(escopo): assunto`, só assunto, sem corpo, **sem** `Co-Authored-By` (o hook da Auster recusa).
- Branch: `feat/backup-paralelo` (já criada).
- No Git Bash, rodar `export PATH="$HOME/.cargo/bin:$PATH"` antes de `cargo`.
- Textos da UI em português, como os existentes.

## Review Focus

1. **Tema escuro nas telas não redesenhadas** — Navegador, Lista e Leitor têm cores fixas claras (`#fff`, `#f3f3f3`); com o texto herdando a cor clara do tema escuro, ficariam ilegíveis. A Task 4 troca essas cores por tokens e tem um passo de verificação por `grep`.
2. **Cancelar com contas na fila** — as contas que ainda não começaram não podem iniciar e o card precisa sair de "Na fila" para "Cancelado". Coberto no código da Task 3 (checa `cancel` antes de iniciar e emite `cancelled`) e no QA da Task 8.
3. **Conta lenta não pode segurar a fila** — com `buffered` ordenado, uma conta enorme na frente travaria as outras vagas. O teste da Task 2 faz o primeiro item ser o mais lento e exige pico de concorrência = limite com resultados na ordem.
4. **`config.json` existente sem o campo novo** — teste na Task 1.
5. **Trocar de tela durante o backup** — `Accounts` fica montada (oculta) enquanto o usuário navega; os eventos precisam continuar atualizando os cards. Mantido pelo `App.svelte` (não mudar o `<div hidden>`), item de QA na Task 8.

---

### Task 1: Limite configurável no `Config` e comandos

**Files:**
- Modify: `src-tauri/src/config.rs` (struct `Config`, testes)
- Modify: `src-tauri/src/commands.rs` (dois comandos novos)
- Modify: `src-tauri/src/lib.rs` (registrar comandos)

**Interfaces:**
- Produces:
  - `config::DEFAULT_MAX_PARALLEL: u32 = 5`
  - `config::check_max_parallel(n: u32) -> Result<u32>`
  - `Config.max_parallel: u32`
  - comandos Tauri `get_max_parallel() -> u32` e `set_max_parallel(n: u32) -> u32`

- [ ] **Step 1: Write the failing tests** — em `src-tauri/src/config.rs`, dentro de `mod tests`, acrescentar:

```rust
    #[test]
    fn max_parallel_defaults_to_5() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, br#"{"accounts":[]}"#).unwrap();
        assert_eq!(Config::load(&p).unwrap().max_parallel, 5);
        assert_eq!(Config::default().max_parallel, 5);
    }

    #[test]
    fn max_parallel_range() {
        assert!(check_max_parallel(0).is_err());
        assert!(check_max_parallel(21).is_err());
        assert_eq!(check_max_parallel(1).unwrap(), 1);
        assert_eq!(check_max_parallel(20).unwrap(), 20);
        assert_eq!(check_max_parallel(0).unwrap_err().to_string(), "limite deve ficar entre 1 e 20");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib config`
Expected: erro de compilação — `no field max_parallel` / `cannot find function check_max_parallel`.

- [ ] **Step 3: Implement** — em `src-tauri/src/config.rs`, trocar o bloco do `Config`:

```rust
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub accounts: Vec<Account>,
}
```

por:

```rust
pub const DEFAULT_MAX_PARALLEL: u32 = 5;

fn default_max_parallel() -> u32 {
    DEFAULT_MAX_PARALLEL
}

/// Quantas contas fazem backup ao mesmo tempo; teto evita bloqueio por excesso de conexões no provedor.
pub fn check_max_parallel(n: u32) -> Result<u32> {
    if (1..=20).contains(&n) { Ok(n) } else { Err(AppError::Other("limite deve ficar entre 1 e 20".into())) }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Config {
    pub accounts: Vec<Account>,
    #[serde(default = "default_max_parallel")]
    pub max_parallel: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config { accounts: Vec::new(), max_parallel: DEFAULT_MAX_PARALLEL }
    }
}
```

Conferir que ninguém monta `Config { .. }` literal fora daqui: `grep -rn "Config {" src-tauri/src` deve mostrar só `config.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib config`
Expected: PASS (inclusive os testes antigos `load_missing_returns_empty_and_roundtrips` e `invalid_json_is_reported`).

- [ ] **Step 5: Add the commands** — em `src-tauri/src/commands.rs`, logo depois de `cancel_backup`:

```rust
#[tauri::command]
pub async fn get_max_parallel(state: State<'_, AppState>) -> Result<u32> {
    Ok(state.config.lock().unwrap().max_parallel)
}

#[tauri::command]
pub async fn set_max_parallel(state: State<'_, AppState>, n: u32) -> Result<u32> {
    ensure_idle(&state)?;
    let n = config::check_max_parallel(n)?;
    state.update(|c| {
        c.max_parallel = n;
        Ok(n)
    })
}
```

Em `src-tauri/src/lib.rs`, no `generate_handler![...]`, depois de `commands::cancel_backup,`:

```rust
            commands::get_max_parallel,
            commands::set_max_parallel,
```

- [ ] **Step 6: Build and test**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: compila; todos os testes passam.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/config.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(config): limite de contas simultâneas (padrão 5, 1 a 20)"
```

---

### Task 2: Helper `run_limited`

**Files:**
- Modify: `src-tauri/src/commands.rs` (helper + teste)

**Interfaces:**
- Produces: `async fn run_limited<T, F, Fut>(ids: Vec<String>, limit: usize, f: F) -> Vec<T> where F: Fn(String) -> Fut, Fut: Future<Output = T>` — no máximo `limit` futuros ativos; `f` só é chamado quando há vaga; resultado na ordem de `ids`; `limit = 0` é tratado como 1.

- [ ] **Step 1: Write the failing test** — em `src-tauri/src/commands.rs`, dentro de `mod tests`:

```rust
    #[tokio::test]
    async fn run_limited_caps_concurrency_and_keeps_order() {
        use std::sync::atomic::AtomicUsize;
        use std::time::Duration;
        let (active, peak) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let ids: Vec<String> = (0..12).map(|i| i.to_string()).collect();
        let out = run_limited(ids.clone(), 3, |id| {
            let (active, peak) = (&active, &peak);
            async move {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                // o primeiro é o mais lento: com fila ordenada ele seguraria as outras vagas
                let n: u64 = id.parse().unwrap();
                tokio::time::sleep(Duration::from_millis(5 * (12 - n))).await;
                active.fetch_sub(1, Ordering::SeqCst);
                id
            }
        })
        .await;
        assert_eq!(out, ids);
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib run_limited`
Expected: erro de compilação — `cannot find function run_limited`.

- [ ] **Step 3: Implement** — em `src-tauri/src/commands.rs`, acrescentar aos imports:

```rust
use futures::StreamExt;
use std::future::Future;
```

e, logo antes de `async fn backup_one`:

```rust
/// Roda `f` para cada id com no máximo `limit` ao mesmo tempo; resultados na ordem de `ids`.
/// `buffer_unordered` (e não `buffered`) para uma conta lenta não segurar as vagas das outras.
async fn run_limited<T, F, Fut>(ids: Vec<String>, limit: usize, f: F) -> Vec<T>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = T>,
{
    let mut out: Vec<(usize, T)> = futures::stream::iter(ids.into_iter().enumerate())
        .map(|(i, id)| {
            let fut = f(id);
            async move { (i, fut.await) }
        })
        .buffer_unordered(limit.max(1))
        .collect()
        .await;
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, t)| t).collect()
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib run_limited`
Expected: PASS. (`run_limited` ainda não é usado fora do teste; um aviso `dead_code` é esperado até a Task 3.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(backup): helper run_limited com limite de concorrência"
```

---

### Task 3: `run_backup` paralelo e evento `backup://status`

**Files:**
- Modify: `src-tauri/src/commands.rs` (`backup_one`, `run_backup`, struct `BackupStatus`, fn `final_status`, teste)

**Interfaces:**
- Consumes: `run_limited` (Task 2), `Config.max_parallel` (Task 1).
- Produces:
  - evento `backup://status` com `BackupStatus { account_id: String, state: &'static str, new_messages: Option<u64>, error: Option<String> }` (campos `None` omitidos no JSON)
  - `fn final_status(id: &str, outcome: &Result<SyncReport>) -> BackupStatus`
  - `run_backup` devolve um `BackupResult` para **toda** conta pedida, na ordem; conta não iniciada por cancelamento vem com `report.cancelled = true` e `new_messages = 0`.

- [ ] **Step 1: Write the failing test** — em `mod tests` de `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn final_status_maps_outcome() {
        let ok = final_status("a", &Ok(SyncReport { new_messages: 3, ..Default::default() }));
        assert_eq!((ok.state, ok.new_messages, ok.error.as_deref()), ("done", Some(3), None));
        let c = final_status("a", &Ok(SyncReport { cancelled: true, ..Default::default() }));
        assert_eq!((c.state, c.new_messages), ("cancelled", Some(0)));
        let f = final_status("a", &Err(AppError::AuthFailed));
        assert_eq!((f.state, f.new_messages, f.error.as_deref()), ("failed", None, Some("usuário ou senha inválidos")));
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            serde_json::json!({"account_id": "a", "state": "failed", "error": "usuário ou senha inválidos"})
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib final_status`
Expected: erro de compilação — `cannot find function final_status`.

- [ ] **Step 3: Add `BackupStatus` and `final_status`** — em `src-tauri/src/commands.rs`, logo depois da struct `BackupResult`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    pub account_id: String,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_messages: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn final_status(id: &str, outcome: &Result<SyncReport>) -> BackupStatus {
    match outcome {
        Ok(r) => BackupStatus {
            account_id: id.to_string(),
            state: if r.cancelled { "cancelled" } else { "done" },
            new_messages: Some(r.new_messages),
            error: None,
        },
        Err(e) => BackupStatus { account_id: id.to_string(), state: "failed", new_messages: None, error: Some(e.to_string()) },
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib final_status`
Expected: PASS.

- [ ] **Step 5: Make `backup_one` own its data** — trocar a função inteira por:

```rust
async fn backup_one(app: AppHandle, id: String) -> Result<SyncReport> {
    let state = app.state::<AppState>();
    let acc = state.account(&id)?;
    if !acc.has_credentials {
        return Err(other("conta sem credenciais (somente leitura)"));
    }
    let pw = config::get_password(&id)?;
    let mut index = open_index(&acc)?;
    let mail_root = acc.dir.join("mail");
    let progress = |p: Progress| {
        let _ = app.emit("backup://progress", p);
    };
    let ctx = SyncCtx { account_id: &id, mail_root: &mail_root, cancel: &state.cancel, progress: &progress };
    let (host, user, pass, port) = (acc.host.as_str(), acc.username.as_str(), pw.as_str(), acc.port);
    imap_sync::sync_account(move || imap_sync::connect_tls(host, port, user, pass), &ctx, &mut index).await
}
```

e trocar o import `use tauri::{AppHandle, Emitter, State};` por `use tauri::{AppHandle, Emitter, Manager, State};`.

- [ ] **Step 6: Rewrite `run_backup`** — trocar a função inteira por:

```rust
#[tauri::command]
pub async fn run_backup(app: AppHandle, state: State<'_, AppState>, ids: Vec<String>) -> Result<Vec<BackupResult>> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(other("já existe um backup em andamento"));
    }
    state.cancel.store(false, Ordering::SeqCst);
    let limit = state.config.lock().unwrap().max_parallel as usize;
    let st: &AppState = &state;
    let results = run_limited(ids, limit, |id| {
        let app = app.clone();
        async move {
            // cancelado enquanto esperava vaga: não começa e não grava last_run
            let outcome = if st.cancel.load(Ordering::SeqCst) {
                Ok(SyncReport { cancelled: true, ..Default::default() })
            } else {
                let running = BackupStatus { account_id: id.clone(), state: "running", new_messages: None, error: None };
                let _ = app.emit("backup://status", running);
                // task própria: parse, disco e SQLite de uma conta não travam as outras
                let outcome = tauri::async_runtime::spawn(backup_one(app.clone(), id.clone())).await.unwrap_or_else(|e| Err(other(e)));
                let last = match &outcome {
                    Ok(r) => LastRun { at: now(), new_messages: r.new_messages, failed_folders: r.failed_folders.clone(), error: None },
                    Err(e) => LastRun { at: now(), new_messages: 0, failed_folders: vec![], error: Some(e.to_string()) },
                };
                let _ = st.update(|c| {
                    c.get_mut(&id)?.last_run = Some(last);
                    Ok(())
                });
                outcome
            };
            let _ = app.emit("backup://status", final_status(&id, &outcome));
            match outcome {
                Ok(r) => BackupResult { account_id: id, report: Some(r), error: None },
                Err(e) => BackupResult { account_id: id, report: None, error: Some(e) },
            }
        }
    })
    .await;
    state.running.store(false, Ordering::SeqCst);
    Ok(results)
}
```

Notas para quem implementa:
- `tauri::async_runtime::spawn(...).await` devolve `Result<Result<SyncReport>, JoinError>`; o `unwrap_or_else` achata para `Result<SyncReport>`.
- Se o compilador reclamar que o futuro de `backup_one` não é `Send`, o culpado é algo segurado através de um `.await`; o código acima só segura `State`, `Index`, `String`s e referências — todos `Send` (o comando antigo já exigia `Send`). Não troque `Mutex` por outra coisa.

- [ ] **Step 7: Build and run all tests**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: compila sem warnings novos; todos os testes passam.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(backup): contas em paralelo com evento de status por conta"
```

---

### Task 4: Tokens visuais, componentes globais e tema escuro

**Files:**
- Modify: `src/App.svelte` (estilo inteiro)
- Modify: `src/lib/api.js` (3 funções + action `modal`)
- Modify: `src/lib/Browser.svelte`, `src/lib/MessageList.svelte`, `src/lib/Reader.svelte` (só cores fixas → tokens)

**Interfaces:**
- Produces (usados pelas Tasks 5–7):
  - `api.getMaxParallel(): Promise<number>`, `api.setMaxParallel(n): Promise<number>`, `api.onStatus(cb): Promise<unlisten>`
  - `modal` — action Svelte (`use:modal`) que chama `showModal()` no `<dialog>`
  - classes globais: `button.primary`, `button.ghost`, `button.danger`, `button.icon`, `.chip` (+ `.run`, `.ok`, `.bad`), `.alert` (+ `.ok`, `.bad`), `.muted`, `.spacer`, estilos de `dialog`, `dialog h2`, `dialog footer`
  - tokens CSS: `--bg --surface --border --hover --text --muted --primary --primary-hover --primary-soft --primary-soft-text --ok-soft --ok-text --bad --bad-soft --bad-text --shadow --shadow-lg`

- [ ] **Step 1: api.js** — em `src/lib/api.js`, dentro do objeto `api`, depois de `onProgress`:

```js
  onStatus: (cb) => listen('backup://status', (e) => cb(e.payload)),
  getMaxParallel: () => invoke('get_max_parallel'),
  setMaxParallel: (n) => invoke('set_max_parallel', { n }),
```

e no fim do arquivo:

```js
// use:modal — abre o <dialog> como modal assim que entra na tela
export const modal = (node) => { node.showModal() }
```

- [ ] **Step 2: App.svelte** — substituir o bloco `<style>` inteiro (e o cabeçalho) de `src/App.svelte`. O `<script>` e o `<div hidden>` ficam como estão. Arquivo final:

```svelte
<script>
  import Accounts from './lib/Accounts.svelte'
  import Browser from './lib/Browser.svelte'

  let account = $state(null)
</script>

<header>
  <h1>Mail Backup</h1>
  {#if account}<button class="ghost" onclick={() => (account = null)}>← Contas</button> <span class="muted">{account.name}</span>{/if}
</header>

<!-- Contas fica montada (só oculta) para não perder o estado de um backup em andamento -->
<div hidden={!!account}><Accounts onbrowse={(a) => (account = a)} /></div>
{#if account}<Browser {account} />{/if}

<style>
  :global(:root) {
    --bg: #f4f4f5; --surface: #fff; --border: #e4e4e7; --hover: #f4f4f5; --text: #18181b; --muted: #71717a;
    --primary: #4f46e5; --primary-hover: #4338ca; --primary-soft: #e0e7ff; --primary-soft-text: #3730a3;
    --ok-soft: #dcfce7; --ok-text: #166534; --bad: #dc2626; --bad-soft: #fee2e2; --bad-text: #991b1b;
    --shadow: 0 1px 2px rgb(0 0 0 / .05); --shadow-lg: 0 12px 32px rgb(0 0 0 / .18);
    color-scheme: light dark;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #09090b; --surface: #18181b; --border: #27272a; --hover: #27272a; --text: #fafafa; --muted: #a1a1aa;
      --primary: #6366f1; --primary-hover: #4f46e5; --primary-soft: #1e1b4b; --primary-soft-text: #c7d2fe;
      --ok-soft: #052e16; --ok-text: #86efac; --bad-soft: #450a0a; --bad-text: #fca5a5;
      --shadow: 0 1px 2px rgb(0 0 0 / .4); --shadow-lg: 0 12px 32px rgb(0 0 0 / .6);
    }
  }
  :global(body) { margin: 0; font: 14px/1.4 system-ui, 'Segoe UI', sans-serif; color: var(--text); background: var(--bg); }
  :global(button) { font: inherit; padding: 6px 12px; cursor: pointer; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  :global(button:hover:not(:disabled)) { background: var(--hover); }
  :global(button:disabled) { opacity: .5; cursor: default; }
  :global(button.primary) { background: var(--primary); border-color: var(--primary); color: #fff; font-weight: 600; }
  :global(button.primary:hover:not(:disabled)) { background: var(--primary-hover); }
  :global(button.ghost) { background: transparent; border-color: transparent; color: var(--muted); }
  :global(button.ghost:hover:not(:disabled)) { background: var(--hover); color: var(--text); }
  :global(button.danger) { background: var(--bad); border-color: var(--bad); color: #fff; font-weight: 600; }
  :global(button.icon) { padding: 2px 8px; }
  :global(input) { font: inherit; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  :global(input[type='checkbox']) { accent-color: var(--primary); }
  :global(button:focus-visible), :global(input:focus-visible) { outline: 2px solid var(--primary); outline-offset: 1px; }
  :global(.muted) { color: var(--muted); }
  :global(.spacer) { flex: 1; }
  :global(.err) { color: var(--bad-text); }
  :global(.info) { color: var(--ok-text); }
  :global(.inline) { display: flex; gap: 8px; align-items: center; margin: 8px 0; }
  :global(.chip) { font-size: 11px; font-weight: 600; padding: 2px 8px; border-radius: 999px; white-space: nowrap; background: var(--hover); color: var(--muted); border: 1px solid var(--border); }
  :global(.chip.run) { background: var(--primary-soft); color: var(--primary-soft-text); border-color: transparent; }
  :global(.chip.ok) { background: var(--ok-soft); color: var(--ok-text); border-color: transparent; }
  :global(.chip.bad) { background: var(--bad-soft); color: var(--bad-text); border-color: transparent; }
  :global(.alert) { display: flex; gap: 8px; align-items: center; margin: 0 0 12px; padding: 8px 12px; border-radius: 10px; }
  :global(.alert.ok) { background: var(--ok-soft); color: var(--ok-text); }
  :global(.alert.bad) { background: var(--bad-soft); color: var(--bad-text); }
  :global(.alert button) { margin-left: auto; color: inherit; }
  :global(dialog) { border: 1px solid var(--border); border-radius: 14px; padding: 20px; background: var(--surface); color: var(--text); box-shadow: var(--shadow-lg); width: min(560px, calc(100vw - 32px)); box-sizing: border-box; }
  :global(dialog::backdrop) { background: rgb(0 0 0 / .45); }
  :global(dialog h2) { font-size: 16px; margin: 0 0 14px; }
  :global(dialog footer) { display: flex; gap: 8px; align-items: center; margin-top: 16px; }
  /* usados só pela tela de Contas antiga; saem na Task 7 */
  :global(.card) { display: grid; gap: 6px; max-width: 480px; padding: 12px; background: var(--surface); border: 1px solid var(--border); margin: 8px 0; }
  :global(.card label) { display: grid; grid-template-columns: 130px 1fr; gap: 8px; align-items: center; }
  :global(table) { width: 100%; border-collapse: collapse; }
  :global(td), :global(th) { padding: 6px; border-bottom: 1px solid var(--border); text-align: left; vertical-align: top; }
  :global(.actions) { white-space: nowrap; }
  :global(.accounts) { padding: 12px; }
  /* altura fixa: Browser.svelte calcula 100vh - 45px */
  header { display: flex; gap: 12px; align-items: center; height: 45px; box-sizing: border-box; padding: 0 16px; border-bottom: 1px solid var(--border); background: var(--surface); }
  header h1 { font-size: 15px; margin: 0; font-weight: 700; letter-spacing: -.01em; }
</style>
```

- [ ] **Step 3: Cores fixas → tokens nas telas não redesenhadas** (sem isso o tema escuro deixa texto claro sobre fundo branco). Edições exatas:

`src/lib/Browser.svelte`:
- `aside { overflow-y: auto; border-right: 1px solid #ddd; display: flex; flex-direction: column; background: #f3f3f3; }` → `aside { overflow-y: auto; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--bg); }`
- `aside button.sel { background: #dde7f3; }` → `aside button.sel { background: var(--primary-soft); }`
- `.reindex { margin-top: auto; color: #555; }` → `.reindex { margin-top: auto; color: var(--muted); }`
- `section { display: flex; flex-direction: column; border-right: 1px solid #ddd; min-height: 0; }` → `section { display: flex; flex-direction: column; border-right: 1px solid var(--border); min-height: 0; }`
- `.empty { padding: 16px; color: #888; }` → `.empty { padding: 16px; color: var(--muted); }`

`src/lib/MessageList.svelte`:
- na regra `.row`, trocar `border-bottom: 1px solid #eee; background: #fff;` por `border-bottom: 1px solid var(--border); border-radius: 0; background: var(--surface);`
- `.row.sel { background: #e3f2fd; }` → `.row.sel { background: var(--primary-soft); }`
- `.subj { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: #555; }` → `... color: var(--muted); }`
- `.empty { padding: 12px; color: #888; }` → `.empty { padding: 12px; color: var(--muted); }`

`src/lib/Reader.svelte`:
- `.head { padding: 10px 12px; border-bottom: 1px solid #ddd; background: #fff; }` → `.head { padding: 10px 12px; border-bottom: 1px solid var(--border); background: var(--surface); }`
- **Não mudar** `iframe { ... background: #fff; }` — o HTML do e-mail assume fundo branco.

- [ ] **Step 4: Verify no hardcoded colors left**

Run: `grep -nE "#[0-9a-fA-F]{3,6}\b" src/lib/Browser.svelte src/lib/MessageList.svelte src/lib/Reader.svelte`
Expected: só a linha do `iframe { ... background: #fff; }`.

- [ ] **Step 5: Build**

Run: `npm run build`
Expected: build OK, sem erros (avisos de a11y já existentes podem aparecer; nenhum novo de CSS não usado em `App.svelte`).

- [ ] **Step 6: Visual check** — `npm run tauri dev`; abrir uma conta no Navegador; no Windows, alternar Configurações → Personalização → Cores → Modo escuro: lista, pastas e cabeçalho do leitor continuam legíveis; o e-mail continua com fundo branco.

- [ ] **Step 7: Commit**

```bash
git add src/App.svelte src/lib/api.js src/lib/Browser.svelte src/lib/MessageList.svelte src/lib/Reader.svelte
git commit -m "feat(ui): tokens visuais, componentes globais e tema escuro"
```

---

### Task 5: `AccountCard.svelte`

**Files:**
- Create: `src/lib/AccountCard.svelte`

**Interfaces:**
- Consumes: `fmtDate` de `api.js`; classes globais da Task 4.
- Produces: componente `AccountCard` com props:
  - `account` — objeto `Account` de `list_accounts`
  - `status` — `undefined` ou `{ state: 'queued'|'running'|'done'|'failed'|'cancelled', folder?, done?, total?, new_messages?, error? }`
  - `running: boolean` (há backup em andamento)
  - `selected: boolean`
  - callbacks sem argumento: `ontoggle`, `onbrowse`, `onbackup`, `onedit`, `onremove`

- [ ] **Step 1: Create the component** — `src/lib/AccountCard.svelte`:

```svelte
<script>
  import { fmtDate } from './api.js'

  let { account, status, running, selected, ontoggle, onbrowse, onbackup, onedit, onremove } = $props()

  const CHIPS = {
    queued: ['Na fila', ''], running: ['Em andamento', 'run'], done: ['Concluído', 'ok'],
    failed: ['Falhou', 'bad'], cancelled: ['Cancelado', ''],
  }
  const n = (v) => (v ?? 0).toLocaleString('pt-BR')
  const initials = (s) => s.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join('').toUpperCase() || '?'
  const hue = (id) => [...id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 0)

  let chip = $derived(status ? CHIPS[status.state] : account.has_credentials ? null : ['Somente leitura', ''])
  let pct = $derived(status?.total ? Math.min(100, (100 * status.done) / status.total) : null)
  let menuId = $derived(`menu-${account.id}`)
  // last_run só é recarregado quando o backup inteiro termina
  let showFailed = $derived((!status || (status.state === 'done' && !running)) && account.last_run?.failed_folders.length)
</script>

<article class="card" class:selected>
  <header>
    <input type="checkbox" checked={selected} onchange={ontoggle} aria-label="Selecionar {account.name} para exportar" />
    <span class="avatar" style="background: hsl({hue(account.id)} 55% 45%)">{initials(account.name)}</span>
    <div class="who">
      <b>{account.name}</b>
      <span class="muted">{account.email}</span>
    </div>
    {#if chip}<span class="chip {chip[1]}">{chip[0]}</span>{/if}
  </header>

  <div class="body">
    {#if status?.state === 'running'}
      <div class="muted">{status.folder ? `${status.folder} · ${n(status.done)} / ${n(status.total)}` : 'Conectando…'}</div>
      <div class="bar" class:indeterminate={pct === null} role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct === null ? undefined : Math.round(pct)}>
        <i style="width: {pct ?? 30}%"></i>
      </div>
    {:else if status?.state === 'queued'}
      <div class="muted">Aguardando uma vaga…</div>
    {:else if status?.state === 'done'}
      <div>+{n(status.new_messages)} novas</div>
    {:else if status?.state === 'cancelled'}
      <div class="muted">Cancelado · +{n(status.new_messages)} novas</div>
    {:else if status?.state === 'failed'}
      <div class="err">{status.error}</div>
    {:else if account.last_run}
      <div class="muted">{fmtDate(account.last_run.at)} — {account.last_run.error ?? `+${n(account.last_run.new_messages)} novas`}</div>
    {:else}
      <div class="muted">Nenhum backup ainda</div>
    {/if}
    {#if showFailed}<small class="err">Falharam: {account.last_run.failed_folders.join(', ')}</small>{/if}
  </div>

  <footer>
    <button onclick={onbrowse}>Abrir</button>
    {#if account.has_credentials}<button onclick={onbackup} disabled={running}>Backup</button>{/if}
    <span class="spacer"></span>
    <button class="ghost icon" popovertarget={menuId} style="anchor-name: --{menuId}" aria-label="Mais ações" disabled={running}>⋯</button>
    <div id={menuId} popover class="menu" style="position-anchor: --{menuId}">
      <button class="ghost" popovertarget={menuId} popovertargetaction="hide" onclick={onedit}>Editar</button>
      <button class="ghost remove" popovertarget={menuId} popovertargetaction="hide" onclick={onremove}>Remover</button>
    </div>
  </footer>
</article>

<style>
  .card { background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 14px; display: grid; gap: 10px; box-shadow: var(--shadow); }
  .card.selected { border-color: var(--primary); box-shadow: 0 0 0 1px var(--primary); }
  header { display: flex; gap: 10px; align-items: center; }
  .avatar { width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; color: #fff; font-weight: 700; font-size: 12px; flex: none; }
  .who { display: grid; min-width: 0; flex: 1; }
  .who span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
  .body { display: grid; gap: 6px; min-height: 38px; align-content: center; font-size: 13px; }
  .bar { height: 6px; border-radius: 99px; background: var(--border); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--primary); border-radius: 99px; transition: width .3s; }
  .bar.indeterminate i { animation: slide 1.2s ease-in-out infinite; }
  @keyframes slide { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
  @media (prefers-reduced-motion: reduce) { .bar.indeterminate i { animation: none; } }
  footer { display: flex; gap: 6px; align-items: center; }
  .menu { inset: auto; margin: 4px 0 0; position-area: bottom span-left; padding: 4px; border: 1px solid var(--border); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow-lg); flex-direction: column; min-width: 140px; }
  .menu:popover-open { display: flex; }
  .menu button { text-align: left; color: var(--text); }
  .menu button.remove { color: var(--bad-text); }
</style>
```

- [ ] **Step 2: Build**

Run: `npm run build`
Expected: build OK. (O componente ainda não é usado; o Vite compila só o que é importado, então a checagem real vem na Task 7. Se quiser checar já, importe-o temporariamente em `Accounts.svelte` e remova antes do commit.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/AccountCard.svelte
git commit -m "feat(ui): card de conta com status e progresso"
```

---

### Task 6: `AccountForm.svelte` (modal de adicionar/editar)

**Files:**
- Create: `src/lib/AccountForm.svelte`

**Interfaces:**
- Consumes: `api.saveAccount`, `api.testConnection`, `api.pickFolder`, `errMsg`, `modal` (Task 4).
- Produces: componente `AccountForm` com props `initial` (objeto no formato de `blank()` de `Accounts.svelte`, opcionalmente mesclado com a conta), `onsaved()` (chamado após salvar com sucesso), `onclose()` (chamado quando o diálogo fecha por Cancelar ou Esc).

- [ ] **Step 1: Create the component** — `src/lib/AccountForm.svelte`:

```svelte
<script>
  import { api, errMsg, modal } from './api.js'

  let { initial, onsaved, onclose } = $props()
  let form = $state({ ...initial })
  let dlg
  let error = $state('')
  let tested = $state('')
  let busy = $state(false)

  const input = () => ({ ...form, port: Number(form.port), password: form.password || null })

  async function save(e) {
    e.preventDefault()
    error = ''; busy = true
    try { await api.saveAccount(input()); onsaved() } catch (err) { error = errMsg(err) } finally { busy = false }
  }

  async function test() {
    error = ''; tested = ''; busy = true
    try { await api.testConnection(input()); tested = 'Conexão OK' } catch (err) { error = errMsg(err) } finally { busy = false }
  }

  async function pickDir() {
    const d = await api.pickFolder()
    if (d) form.dir = d
  }
</script>

<dialog bind:this={dlg} use:modal onclose={onclose}>
  <form onsubmit={save}>
    <h2>{form.id ? 'Editar conta' : 'Adicionar conta'}</h2>
    <div class="fields">
      <label>Nome<input bind:value={form.name} required /></label>
      <label>E-mail<input type="email" bind:value={form.email} required /></label>
      <label>Servidor IMAP<input bind:value={form.host} required /></label>
      <label>Porta<input type="number" min="1" max="65535" bind:value={form.port} required /></label>
      <label>Usuário<input bind:value={form.username} required /></label>
      <label>Senha<input type="password" bind:value={form.password} placeholder={form.id ? '(manter a atual)' : ''} /></label>
      {#if !form.id}
        <div class="dir">
          <span>Pasta do backup</span>
          <code>{form.dir ?? '(padrão do app)'}</code>
          <button type="button" onclick={pickDir}>Escolher…</button>
        </div>
      {/if}
    </div>
    {#if error}<p class="alert bad">{error}</p>{/if}
    {#if tested}<p class="alert ok">{tested}</p>{/if}
    <footer>
      <button type="button" class="ghost" onclick={test} disabled={busy}>Testar conexão</button>
      <span class="spacer"></span>
      <button type="button" onclick={() => dlg.close()}>Cancelar</button>
      <button type="submit" class="primary" disabled={busy}>Salvar</button>
    </footer>
  </form>
</dialog>

<style>
  .fields { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; margin-bottom: 12px; }
  label { display: grid; gap: 4px; font-size: 12px; color: var(--muted); }
  label input { color: var(--text); }
  .dir { grid-column: 1 / -1; display: flex; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); }
  .dir code { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
</style>
```

Notas:
- `onsaved` desmonta o componente pelo pai (`form = null`); o `<dialog>` sai do DOM aberto, o que é válido, e `onclose` não dispara — por isso o pai faz o `refresh()` em `onsaved`.
- `form.port` vem como string de `<input type="number">` quando editado; `input()` converte, como hoje.

- [ ] **Step 2: Commit** (build verificado na Task 7, quando o componente passa a ser importado)

```bash
git add src/lib/AccountForm.svelte
git commit -m "feat(ui): modal de adicionar e editar conta"
```

---

### Task 7: Nova tela de Contas

**Files:**
- Modify (reescrita): `src/lib/Accounts.svelte`
- Modify: `src/App.svelte` (remover os estilos globais marcados "saem na Task 7")

**Interfaces:**
- Consumes: `AccountCard` (Task 5), `AccountForm` (Task 6), `api.onStatus`, `api.getMaxParallel`, `api.setMaxParallel`, `modal` (Task 4), eventos `backup://progress` e `backup://status` (Task 3).
- Produces: a mesma prop pública de hoje — `onbrowse(account)` — consumida por `App.svelte`.

- [ ] **Step 1: Rewrite `src/lib/Accounts.svelte`** — arquivo final completo:

```svelte
<script>
  import { api, errMsg, modal } from './api.js'
  import AccountCard from './AccountCard.svelte'
  import AccountForm from './AccountForm.svelte'

  let { onbrowse } = $props()
  let accounts = $state([])
  let alert = $state(null) // { kind: 'ok' | 'bad', text }
  let form = $state(null)
  let running = $state(false)
  let cancelling = $state(false)
  let status = $state({}) // account_id → { state, folder, done, total, new_messages, error }
  let selected = $state([])
  let exportPw = $state('')
  let maxParallel = $state(5)
  let removing = $state(null)
  let removeFiles = $state(false)
  let pendingImport = $state(null)
  let importPw = $state('')
  let importError = $state('')

  const blank = () => ({ id: null, name: '', email: '', host: 'imap.uhserver.com', port: 993, username: '', password: '', dir: null })
  const fail = (e) => (alert = { kind: 'bad', text: errMsg(e) })
  const closeDialog = (e) => e.currentTarget.closest('dialog').close()
  const toggle = (id) => (selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id])

  async function refresh() {
    try {
      accounts = await api.listAccounts()
      selected = selected.filter((id) => accounts.some((a) => a.id === id))
    } catch (e) { fail(e) }
  }
  refresh()
  api.getMaxParallel().then((n) => (maxParallel = n)).catch(fail)

  $effect(() => {
    const offs = [
      api.onProgress((p) => (status[p.account_id] = { ...status[p.account_id], state: 'running', folder: p.folder, done: p.done, total: p.total })),
      api.onStatus((s) => (status[s.account_id] = { ...status[s.account_id], ...s })),
    ]
    return () => offs.forEach((u) => u.then((f) => f()))
  })

  async function backup(ids) {
    running = true; cancelling = false; alert = null
    status = Object.fromEntries(ids.map((id) => [id, { state: 'queued' }]))
    try { await api.runBackup(ids) } catch (e) { fail(e) } finally { running = false; refresh() }
  }

  async function cancel() {
    cancelling = true
    try { await api.cancelBackup() } catch (e) { fail(e) }
  }

  async function changeMax(delta) {
    const n = maxParallel + delta
    if (n < 1 || n > 20) return
    try { maxParallel = await api.setMaxParallel(n) } catch (e) { fail(e) }
  }

  async function confirmRemove() {
    const a = removing
    removing = null
    try { await api.deleteAccount(a.id, removeFiles); refresh() } catch (e) { fail(e) }
  }

  async function exportSelected() {
    alert = null
    try { if (await api.exportAccounts(selected, exportPw || null)) alert = { kind: 'ok', text: 'Backup exportado.' } } catch (e) { fail(e) }
  }

  async function doImport(path, password) {
    alert = null; importError = ''
    try {
      const added = await api.importArchive(path, password)
      pendingImport = null; importPw = ''
      alert = { kind: 'ok', text: `${added.length} conta(s) importada(s).` }
      refresh()
    } catch (e) {
      if (e.kind === 'WrongPassword') { pendingImport = path; importError = password ? 'Senha incorreta.' : '' }
      else { pendingImport = null; fail(e) }
    }
  }

  async function startImport() {
    const path = await api.pickArchive()
    if (path) doImport(path, null)
  }
</script>

<section class="accounts">
  <div class="toolbar">
    {#if running}
      <button class="primary" onclick={cancel} disabled={cancelling}>{cancelling ? 'Cancelando…' : '■ Cancelar'}</button>
    {:else}
      <button class="primary" onclick={() => backup(accounts.filter((a) => a.has_credentials).map((a) => a.id))} disabled={!accounts.some((a) => a.has_credentials)}>▶ Backup de todas</button>
    {/if}
    <button onclick={() => (form = blank())} disabled={running}>＋ Adicionar conta</button>
    <button class="ghost" onclick={startImport} disabled={running}>Importar backup…</button>
    <div class="stepper" title="Quantas contas fazem backup ao mesmo tempo">
      <span>Contas simultâneas</span>
      <button class="icon" onclick={() => changeMax(-1)} disabled={running || maxParallel <= 1} aria-label="Diminuir">−</button>
      <b>{maxParallel}</b>
      <button class="icon" onclick={() => changeMax(1)} disabled={running || maxParallel >= 20} aria-label="Aumentar">＋</button>
    </div>
  </div>

  {#if alert}
    <p class="alert {alert.kind}">{alert.text}<button class="ghost icon" onclick={() => (alert = null)} aria-label="Fechar">×</button></p>
  {/if}

  {#if accounts.length}
    <div class="grid">
      {#each accounts as a (a.id)}
        <AccountCard
          account={a}
          status={status[a.id]}
          {running}
          selected={selected.includes(a.id)}
          ontoggle={() => toggle(a.id)}
          onbrowse={() => onbrowse(a)}
          onbackup={() => backup([a.id])}
          onedit={() => (form = { ...blank(), ...a, password: '' })}
          onremove={() => { removing = a; removeFiles = false }}
        />
      {/each}
    </div>
  {:else}
    <div class="empty">
      <b>Nenhuma conta ainda</b>
      <span class="muted">Adicione uma conta IMAP ou importe um backup para começar.</span>
    </div>
  {/if}

  {#if selected.length}
    <div class="exportbar">
      <span><b>{selected.length}</b> conta(s) selecionada(s)</span>
      <input type="password" placeholder="Senha do arquivo (opcional)" bind:value={exportPw} />
      <button class="primary" onclick={exportSelected} disabled={running}>Exportar…</button>
      <button class="ghost" onclick={() => (selected = [])}>Limpar</button>
    </div>
  {/if}
</section>

{#if form}
  <AccountForm initial={form} onclose={() => (form = null)} onsaved={() => { form = null; refresh() }} />
{/if}

{#if removing}
  <dialog use:modal onclose={() => (removing = null)}>
    <h2>Remover {removing.name}?</h2>
    <p class="muted">A conta {removing.email} sai do app e a senha salva é apagada.</p>
    <label class="check"><input type="checkbox" bind:checked={removeFiles} /> Apagar também os e-mails salvos no disco</label>
    <footer>
      <span class="spacer"></span>
      <button onclick={closeDialog}>Cancelar</button>
      <button class="danger" onclick={confirmRemove}>Remover</button>
    </footer>
  </dialog>
{/if}

{#if pendingImport}
  <dialog use:modal onclose={() => { pendingImport = null; importPw = ''; importError = '' }}>
    <form onsubmit={(e) => { e.preventDefault(); doImport(pendingImport, importPw) }}>
      <h2>Arquivo protegido por senha</h2>
      {#if importError}<p class="alert bad">{importError}</p>{/if}
      <input class="full" type="password" placeholder="Senha do arquivo" bind:value={importPw} required />
      <footer>
        <span class="spacer"></span>
        <button type="button" onclick={closeDialog}>Cancelar</button>
        <button type="submit" class="primary">Importar</button>
      </footer>
    </form>
  </dialog>
{/if}

<style>
  .accounts { padding: 20px 24px 96px; max-width: 1200px; margin: 0 auto; }
  .toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
  .stepper { margin-left: auto; display: flex; align-items: center; gap: 6px; color: var(--muted); font-size: 12px; }
  .stepper b { min-width: 20px; text-align: center; color: var(--text); font-size: 14px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }
  .empty { display: grid; gap: 4px; justify-items: center; padding: 48px 16px; border: 1px dashed var(--border); border-radius: 12px; }
  .exportbar { position: fixed; left: 50%; bottom: 16px; transform: translateX(-50%); display: flex; gap: 8px; align-items: center; background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 8px 12px; box-shadow: var(--shadow-lg); max-width: calc(100vw - 32px); box-sizing: border-box; }
  .check { display: flex; gap: 8px; align-items: center; }
  .full { width: 100%; box-sizing: border-box; }
</style>
```

- [ ] **Step 2: Remove obsolete globals** — em `src/App.svelte`, apagar o comentário `/* usados só pela tela de Contas antiga; saem na Task 7 */` e as 6 linhas abaixo dele (`.card`, `.card label`, `table`, `td/th`, `.actions`, `.accounts`). Conferir que nada mais usa: `grep -rnE "class=\"(card|actions)\"|<table" src` deve voltar vazio.

- [ ] **Step 3: Build**

Run: `npm run build`
Expected: build OK, sem erros.

- [ ] **Step 4: Run the app and check** — `npm run tauri dev`, com pelo menos 2 contas com senha:
  - cards em grade; avatar, nome, e-mail, último backup;
  - `−`/`＋` do limite mudam o número; fechar e reabrir o app mantém o valor; nos extremos (1 e 20) o botão fica desabilitado;
  - "Backup de todas": cards passam por Na fila → Em andamento (barra animada até o primeiro progresso, depois barra real com "Pasta · X / Y") → Concluído; com limite 1, as contas rodam uma de cada vez;
  - "■ Cancelar" vira "Cancelando…"; contas que estavam na fila vão para "Cancelado";
  - menu ⋯ abre ancorado ao botão; "Editar" abre o modal preenchido; "Remover" abre o diálogo com a caixa de apagar arquivos;
  - modal de conta: "Testar conexão" mostra "Conexão OK" ou o erro dentro do modal; Esc fecha;
  - selecionar cards mostra a barra de exportação no rodapé; "Limpar" desmarca.

- [ ] **Step 5: Commit**

```bash
git add src/lib/Accounts.svelte src/App.svelte
git commit -m "feat(ui): tela de Contas em cards com backup paralelo"
```

---

### Task 8: QA, contexto do repo e wiki

**Files:**
- Modify: `docs/qa-checklist.md` (nova seção)
- Modify: `AGENTS.md` (preencher o esqueleto)
- Commit em `CLAUDE.md` e `AGENTS.md` (hoje não versionados)
- Wiki: `C:\Users\Zatrakk\Documents\wiki\auster-wiki` (repositório separado)

- [ ] **Step 1: QA checklist** — acrescentar ao fim de `docs/qa-checklist.md`:

```markdown
## Backup paralelo e tela de Contas (2026-10-08)

- [ ] Com 6+ contas e "Contas simultâneas" = 5, "Backup de todas" mostra no máximo 5 cards "Em andamento"; os demais ficam "Na fila" e entram quando uma termina.
- [ ] Limite = 1 → contas rodam uma de cada vez. Alterar o limite, fechar e reabrir o app → valor mantido. `config.json` antigo (sem `max_parallel`) abre com 5.
- [ ] Uma conta com senha errada → card "Falhou" com "usuário ou senha inválidos"; as outras concluem normalmente.
- [ ] "Cancelar" no meio → contas em andamento param no próximo lote; contas na fila vão para "Cancelado" sem começar; rodar de novo completa sem duplicar.
- [ ] Durante o backup, clicar "Abrir" numa conta e voltar com "← Contas" → os cards continuam atualizando; o limite e os botões Backup/⋯ seguem desabilitados.
- [ ] Windows em modo escuro → Contas, Navegador, Lista e Leitor legíveis; o corpo do e-mail continua com fundo branco.
- [ ] Menu ⋯ abre junto ao botão (não no centro da tela); Esc fecha modais e menu.
- [ ] RAM durante backup de 5 contas simultâneas: _(anotar)_
```

- [ ] **Step 2: AGENTS.md** — substituir o conteúdo do esqueleto por (máximo 50 linhas; manter só o que foi apurado):

```markdown
# mail-backup

## O que e

App desktop Windows (Tauri 2 + Svelte 5) que faz backup de contas IMAP em `.eml` com indice SQLite por conta, navega os e-mails salvos e exporta/importa `.zip` (AES opcional).

## Como rodar

- dev: `npm run tauri dev` (no Git Bash: `export PATH="$HOME/.cargo/bin:$PATH"`)
- build: `npx tauri build`
- testes: `cd src-tauri && cargo test --lib`
- integracao (GreenMail, ignorados por padrao): ver o topo de `src-tauri/tests/greenmail.rs`; `cargo test --test greenmail -- --ignored --test-threads=1`
- QA manual: `docs/qa-checklist.md` (o app so conecta via TLS, entao o QA usa conta real UOL `imap.uhserver.com:993`)

## Convencoes que nao se deduzem do codigo

- Specs e planos em `docs/superpowers/{specs,plans}/`.
- `// ponytail:` marca simplificacao deliberada com teto conhecido.
- Backup de contas roda em paralelo (`run_limited`, limite `max_parallel` no `config.json`, 1 a 20); pastas de uma mesma conta continuam em serie numa unica conexao IMAP.
- UI sem biblioteca de componentes: tokens CSS e classes globais em `src/App.svelte`; modais com `<dialog>`, menus com `popover`.

## Armadilhas

- `user_version` do SQLite e o marcador de "indice completo": so `reindex()` grava.
- `AppState.running` e global: remover, reindexar e mudar o limite sao recusados durante qualquer backup.
- `Accounts.svelte` fica montada (oculta) ao navegar, para nao perder o estado do backup — nao trocar por `{#if}`.
- `Browser.svelte` usa `calc(100vh - 45px)`: o header de `App.svelte` tem altura fixa de 45px.
- Leitor: iframe `sandbox` sem `allow-same-origin`/navegacao; nao afrouxar. Fundo do iframe fica `#fff` mesmo no tema escuro.

## Tarefa, branch e commit

- Branch: `feat|fix|chore|docs/<assunto-em-kebab-case>`
- Nao commitar direto em: main, master — main e a branch publicada
- Commit: `tipo(escopo): assunto`, so assunto, sem corpo, sem `Co-Authored-By`

## Wiki

Conhecimento deste repo vive em `C:\Users\Zatrakk\Documents\wiki\auster-wiki`, dominio `projects`.
```

Antes de gravar, conferir cada afirmação no código (ex.: `grep -n "sandbox" src/lib/Reader.svelte`, `grep -n "100vh" src/lib/Browser.svelte`); apagar a linha que não se confirmar.

- [ ] **Step 3: Commit repo**

```bash
git add docs/qa-checklist.md AGENTS.md CLAUDE.md
git commit -m "docs: QA do backup paralelo e contexto do repo"
```

- [ ] **Step 4: Wiki** — ler `C:\Users\Zatrakk\Documents\wiki\auster-wiki\AGENTS.md` e `llms.txt`; criar ou atualizar a página do mail-backup no domínio `projects` com: backup paralelo de contas (limite padrão 5, 1–20, por quê do teto: conexões por IP no provedor), pastas ainda em série, decisão de UI sem biblioteca de componentes. Frontmatter com `description` em termos de busca e `atualizado: 2026-10-08`; links relativos com `<>`. Depois, dentro da wiki:

```bash
python3 gerar_indices.py
git add -A
git commit -m "wiki(projects): mail-backup backup paralelo e nova tela de Contas"
```

- [ ] **Step 5: Final check**

Run: `cd src-tauri && cargo test --lib && cd .. && npm run build && git status`
Expected: testes passam, build OK, árvore limpa.
