# Backup paralelo e nova tela de Contas — Design

Data: 2026-10-08
Status: aguardando revisão

## 1. Objetivo

O "Backup de todas" processa as contas uma por vez; com muitas contas o backup demora.
Rodar várias contas ao mesmo tempo, com limite configurável, e mostrar o progresso de
cada conta numa tela de Contas redesenhada (cards).

### Critérios de sucesso

- Com N contas selecionadas, até `max_parallel` (padrão 5) rodam ao mesmo tempo; as demais aguardam na fila.
- O usuário ajusta o limite na tela (1 a 20); o valor persiste no `config.json`.
- Cada conta mostra seu estado em tempo real: Na fila, Em andamento (pasta, contagem, barra), Concluído, Falhou, Cancelado.
- A falha de uma conta não interrompe as outras.
- Cancelar para as contas em andamento e impede o início das que estão na fila.
- Nada muda na retomada (`last_uid`), dedupe, pastas com falha e gravação do `.eml`.

### Fora de escopo

- Pastas em paralelo dentro de uma mesma conta (adicionar se uma conta isolada virar gargalo).
- Limite por servidor/provedor (o limite é global).
- Redesign das telas Navegador, Lista e Leitor (segundo ciclo).
- Biblioteca de componentes / Tailwind.

## 2. Backend

### 2.1 Limite configurável

- `Config` ganha `max_parallel: u32` com `#[serde(default = "default_max_parallel")]` (= 5); `config.json` antigos continuam válidos.
- Comando `set_max_parallel(n: u32)`: rejeita fora de `1..=20` com erro "limite deve ficar entre 1 e 20"; grava via `AppState::update`.
- Recusado enquanto houver backup em andamento (`ensure_idle`). O valor é lido no início de `run_backup`.
- `list_accounts` não muda; o valor chega à UI por um comando `get_max_parallel()`.

### 2.2 Execução concorrente

- Helper puro em `commands.rs`:
  `async fn run_limited<T, F, Fut>(ids: Vec<String>, limit: usize, f: F) -> Vec<T>`
  usando `futures::stream::iter(..).map(f).buffer_unordered(limit)` com o índice de cada id, reordenando no fim — no máximo `limit` futuros ativos, resultados na ordem de entrada. (`buffered` ordenado seguraria a fila atrás de uma conta lenta.)
- `run_backup` passa para `run_limited` uma closure que, por conta, faz `tauri::async_runtime::spawn` de `backup_one` (cada conta numa task do runtime multi-thread, para que parse/disco/SQLite de uma não travem as outras) e aguarda o `JoinHandle`.
- `backup_one(app: AppHandle, id: String)` recebe dados com dono e obtém o estado por `app.state::<AppState>()`. A lógica de sync (`imap_sync::sync_account`) não muda.
- Antes de iniciar cada conta: se `cancel` estiver ligado, a conta não roda e o resultado é `cancelled` (sem gravar `last_run`).
- `running` continua global: enquanto qualquer conta roda, remover, reindexar, alterar o limite e iniciar outro backup são recusados.

### 2.3 Evento de status

Novo evento `backup://status`, payload:

```json
{ "account_id": "…", "state": "running" | "done" | "failed" | "cancelled", "new_messages": 12, "error": "…" }
```

- `running` ao sair da fila; `done`/`failed`/`cancelled` ao terminar (`cancelled` quando `SyncReport.cancelled` ou quando a conta nem começou).
- `new_messages` só em `done`/`cancelled`; `error` (mensagem) só em `failed`.
- "Na fila" não tem evento: a UI deduz (pedida e sem status ainda).
- `backup://progress` continua igual.

### 2.4 Erros

- Erro de uma conta vira `BackupResult` com `error` e `last_run.error`, como hoje; as outras seguem.
- `last_run` é gravado ao fim de cada conta; gravações concorrentes são serializadas pelo `Mutex<Config>`.
- `JoinError` (task abortada) vira `AppError::Other`. Com `panic = "abort"` no perfil release, panic encerra o app como hoje.

## 3. Tela de Contas

### 3.1 Componentes e estilo

- Sem dependências novas. Tokens CSS em `:root` no `App.svelte` (cores, raio, sombra, espaçamento), tema escuro via `@media (prefers-color-scheme: dark)`.
- Peças: botão primário/secundário/ghost, chip de status, barra de progresso, avatar com iniciais (cor derivada do id), card, aviso dispensável.
- Modais com `<dialog>` nativo; menu ⋯ com o atributo `popover`.

### 3.2 Layout

- **Barra superior:** "▶ Backup de todas" (primário; vira "Cancelar" durante o backup), "＋ Adicionar conta", "Importar…" (ghost) e, à direita, "Contas simultâneas − N ＋" (desabilitado durante o backup).
- **Grade de cards** (`repeat(auto-fill, minmax(300px, 1fr))`). Card:
  - checkbox de seleção para exportação;
  - avatar, nome, e-mail;
  - chip: Na fila / Em andamento / Concluído / Falhou / Cancelado / Somente leitura;
  - durante o backup: "Pasta · 3.420 / 12.000" e barra de progresso (indeterminada até o primeiro evento de progresso);
  - fora do backup: último backup ("+214 novas · 08/10 14:32" ou o erro) e pastas que falharam;
  - ações "Abrir", "Backup" (só com credenciais) e menu ⋯ com "Editar" e "Remover".
- **Exportação:** com seleção, barra fixa no rodapé: "N contas selecionadas", senha opcional, "Exportar…".
- **Aviso no topo** para erro/sucesso, com botão de fechar.

### 3.3 Modais

- **Adicionar/Editar conta:** mesmos campos de hoje em duas colunas; "Testar conexão" mostra o resultado dentro do modal.
- **Remover:** um diálogo com a caixa "Apagar também os e-mails salvos no disco" (substitui os dois `confirm()`).
- **Senha do arquivo** na importação.

### 3.4 Estado e arquivos

- `Accounts.svelte`: estado e orquestração. Progresso vira um mapa `account_id → { state, folder, done, total, new_messages, error }`, alimentado por `onProgress` e `onStatus`; limpo ao iniciar um novo backup, mantido ao terminar até o próximo.
- `AccountCard.svelte` (novo): um card.
- `AccountForm.svelte` (novo): modal de adicionar/editar.
- `api.js`: `getMaxParallel`, `setMaxParallel`, `onStatus`.
- `App.svelte`: tokens e estilos globais; o cabeçalho ganha o mesmo visual.

## 4. Testes

- Unidade (Rust): `config.json` sem `max_parallel` carrega com 5; `set_max_parallel` rejeita 0 e 21.
- Unidade (Rust): `run_limited` com closure falsa — o pico de execuções simultâneas nunca passa do limite e os resultados saem na ordem de entrada.
- `tests/greenmail.rs` continua cobrindo o sync de uma conta (inalterado).
- `vite build` sem erros.
- `docs/qa-checklist.md` ganha: limite respeitado com 6+ contas; cancelar no meio (fila não inicia); uma conta com senha errada falha enquanto as outras concluem; alterar o limite e reabrir o app; tema escuro.
