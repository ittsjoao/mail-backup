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
- Backup de contas roda em paralelo (`run_limited`, limite `max_parallel` no `config.json`, 1 a 20); pastas de uma mesma conta continuam em serie numa sessao IMAP.
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
