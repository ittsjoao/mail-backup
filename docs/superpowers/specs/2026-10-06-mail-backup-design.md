# Mail Backup — Design

Data: 2026-10-06
Status: aguardando revisão

## 1. Objetivo

App desktop Windows, leve, que faz backup completo de contas de e-mail via IMAP,
preservando a estrutura de pastas, e permite navegar/buscar os e-mails salvos.
Backups podem ser exportados e importados como um arquivo `.zip` (senha AES-256 opcional).

### Critérios de sucesso

- Cadastrar conta UOL Host (`imap.uhserver.com:993`, SSL, usuário/senha) e fazer backup de todas as pastas.
- Backups seguintes baixam apenas mensagens novas.
- Nenhuma mensagem é marcada como lida no servidor.
- Queda de rede/app no meio do backup não perde nem duplica mensagens; rodar de novo retoma.
- Navegar pastas, listar e buscar (remetente/assunto/data) em caixas com 100k mensagens sem travar.
- Ler e-mails HTML com segurança (sem scripts, imagens remotas bloqueadas por padrão).
- Exportar uma conta para `.zip` e importar em outra máquina com os `.eml` byte a byte idênticos.
- "Leve": binário de poucos MB e baixo uso de RAM/CPU durante o backup.

### Fora de escopo

- Restaurar e-mails para um servidor (IMAP APPEND) e qualquer uso de SMTP.
- Backup agendado / app residente na bandeja (backup é disparado manualmente).
- OAuth2 (Gmail / Microsoft 365).
- Sincronizar flags após o download, espelhar exclusões do servidor.
- Busca no corpo (full-text), threads/conversas, marcar como lido localmente.
- Importar `.eml`/`.mbox` de outros clientes.
- Outras plataformas além de Windows (não bloqueadas, mas não testadas).

## 2. Stack

| Camada | Escolha |
|---|---|
| Shell desktop | Tauri v2 (WebView2) |
| Frontend | Svelte + Vite |
| IMAP | `async-imap` (`runtime-tokio`) + `tokio-rustls` |
| Nomes de pasta IMAP | `utf7-imap` (UTF-7 modificado) |
| Parsing de e-mail | `mail-parser` |
| Índice | `rusqlite` (feature `bundled`) |
| Credenciais | `keyring` (Gerenciador de Credenciais do Windows) |
| Export/Import | `zip` (feature `aes-crypto`, deflate) |
| Erros | `thiserror` |
| Plugins Tauri | `dialog` (abrir/salvar arquivo), `opener` (abrir no app padrão) |

## 3. Modelo de dados

Uma **conta** no app = uma pasta de dados; credenciais IMAP são opcionais.
- Com credenciais: pode fazer backup e navegar.
- Sem credenciais (importada): só navegar. Adicionar credenciais depois continua o incremental do ponto salvo.

### Configuração

`%APPDATA%/mail-backup/config.json`:

```json
{ "accounts": [ { "id": "uuid", "name": "Joao", "email": "joao@empresa.com.br",
  "host": "imap.uhserver.com", "port": 993, "username": "joao@empresa.com.br",
  "dir": "C:/.../joao@empresa.com.br", "has_credentials": true,
  "last_run": { "at": "2026-10-06T10:00:00Z", "new_messages": 120, "failed_folders": [] } } ] }
```

Pasta da conta: padrão `%APPDATA%/mail-backup/accounts/<id>/`, alterável no cadastro.

Senha: `keyring` com service `mail-backup` e user `<account id>`. Nunca em arquivo, log ou mensagem de erro.

### Layout em disco (por conta)

```
<dir>/
  index.sqlite
  mail/
    INBOX/4521-00001.eml
    Itens Enviados/…
    Clientes/Projeto X/…
```

Nome do arquivo: `{uidvalidity}-{uid:05}.eml`, conteúdo RFC822 bruto, exatamente como veio do servidor.
Os `.eml` são a fonte da verdade; `index.sqlite` é cache reconstruível.

### Esquema SQLite (`PRAGMA user_version = 1`)

```sql
CREATE TABLE folders (
  id INTEGER PRIMARY KEY,
  imap_name TEXT UNIQUE,     -- nome bruto no servidor (NULL se veio de reindexação)
  path TEXT UNIQUE NOT NULL, -- relativo a mail/, definido uma vez
  uidvalidity INTEGER,
  last_uid INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE messages (
  id INTEGER PRIMARY KEY,
  folder_id INTEGER NOT NULL REFERENCES folders(id),
  uid INTEGER,
  file TEXT NOT NULL,        -- relativo a mail/
  date INTEGER,              -- unix; INTERNALDATE ou Date:
  from_addr TEXT, from_name TEXT, subject TEXT,
  message_id TEXT,
  flags TEXT, size INTEGER, has_attach INTEGER,
  UNIQUE(folder_id, file)
);
CREATE INDEX messages_folder_date ON messages(folder_id, date DESC);
```

Esquema com `user_version` menor que o atual → reindexação completa.

## 4. Componentes

Backend `src-tauri/src/`:

| Módulo | Responsabilidade |
|---|---|
| `config.rs` | Ler/gravar `config.json`; ler/gravar senha no `keyring` |
| `imap_sync.rs` | Conectar/login, listar pastas, fetch incremental em lotes; entrega `(uid, flags, internaldate, bytes)` |
| `store.rs` | Nome IMAP → caminho seguro; gravação atômica de `.eml`; varredura para reindexar; resolução segura de caminhos |
| `index.rs` | Esquema, inserts em lote, consultas de lista/busca, estado de sync por pasta |
| `archive.rs` | Exportar/importar `.zip` em streaming, com AES opcional |
| `commands.rs` | Comandos Tauri e eventos de progresso |

Frontend Svelte, telas:
1. **Contas**: adicionar/editar (com "Testar conexão"), remover, "Fazer backup" (conta ou todas) com progresso e cancelar, "Exportar…", "Importar…", resumo do último backup.
2. **Navegador**: árvore de pastas | lista com scroll virtual + busca | leitura.
3. **Leitura**: cabeçalhos, corpo em iframe isolado, anexos.

Fronteira de confiança: o front só envia IDs (`account_id`, `folder_id`, `message_id`, índice de anexo), nunca caminhos. O conteúdo dos e-mails e dos arquivos importados é tratado como hostil.

## 5. Fluxo de backup

1. Ler a senha do `keyring`, conexão TLS na porta 993, `LOGIN`. Uma conexão por conta; contas processadas em sequência.
2. `LIST "" "*"`; ignorar `\Noselect`. Para cada pasta nova:
   - decodificar UTF-7 modificado;
   - dividir pelo delimitador do servidor → subpastas;
   - substituir `<>:"/\|?*` e caracteres de controle por `_`, remover espaços/pontos no fim, prefixar `_` em nomes reservados (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`);
   - colisão de caminho → sufixo `~2`, `~3`…;
   - gravar em `folders`. O caminho nunca muda depois.
3. `EXAMINE` (somente leitura) → `UIDVALIDITY`, `UIDNEXT`.
   - Mesmo `UIDVALIDITY` e `UIDNEXT - 1 <= last_uid`: pular.
   - `UIDVALIDITY` diferente: zerar `last_uid`, gravar o novo `uidvalidity` e baixar tudo, pulando mensagens cujo `(message_id, date, size)` já exista na pasta.
4. `UID FETCH {last_uid+1}:* (UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])` em lotes de 100 UIDs. Descartar respostas com `uid <= last_uid` (o servidor devolve a última mensagem quando não há nada novo). Para cada mensagem:
   - gravar `<nome>.eml.tmp` e renomear para `<nome>.eml`;
   - extrair cabeçalhos com `mail-parser` e inserir em `messages` (`INSERT OR REPLACE`).
   Fim do lote: uma transação inclui os inserts e o novo `last_uid`.
5. Emitir `backup://progress { account_id, folder, done, total }`.
6. No fim: gravar `last_run` na config e emitir `backup://done` com o resumo.

Cancelar: verificado entre lotes; o que foi confirmado permanece.
Memória: uma mensagem por vez na RAM; nada acumulado entre lotes.

## 6. Navegação

- `list_folders(account_id)` → árvore com contagem de mensagens.
- `list_messages(account_id, folder_id?, query?, date_from?, date_to?, offset, limit)` → linhas ordenadas por `date DESC` + total. Busca: `LIKE '%q%'` em `subject`, `from_name` e `from_addr`. Se ficar lento em volumes reais, trocar por FTS5 só nos cabeçalhos.
- `get_message(account_id, message_id)` → cabeçalhos (de, para, cc, data, assunto), corpo e lista de anexos `{index, name, mime, size}`.
  - Corpo: parte HTML se existir; senão texto escapado em `<pre>`.
  - Imagens `cid:` são trocadas por `data:` URIs.
  - HTML exibido em `<iframe sandbox srcdoc>` (sem `allow-scripts`, sem `allow-same-origin`) com CSP injetada: `default-src 'none'; img-src data:; style-src 'unsafe-inline'`. "Carregar imagens" acrescenta `https:` ao `img-src` naquela mensagem.
  - Links abrem no navegador padrão, nunca dentro do app. O mecanismo no Tauri v2 (interceptar a nova janela e chamar o `opener`) é validado num spike no início da implementação.
- "Abrir no programa de e-mail": abre o `.eml` com o app padrão.
- Anexos:
  - "Salvar como…" via dialog;
  - "Abrir" grava em pasta temporária e usa o `opener`. Extensões `.exe .bat .cmd .com .js .jse .vbs .vbe .wsf .ps1 .msi .scr .lnk .hta .cpl .jar` só podem ser salvas.
- Resolução de arquivo: `canonicalize(dir/mail/file)` precisa estar dentro de `canonicalize(dir/mail)`; caso contrário, `UnsafePath`.

## 7. Exportar / Importar

**Exportar** (uma ou mais contas → um `.zip`):
- O usuário escolhe o destino e, opcionalmente, uma senha.
- Entradas: `<email>/mail/**.eml` e `<email>/account.json` (nome, email, host, porta, usuário e o estado de `folders`: `imap_name`, `path`, `uidvalidity`, `last_uid`). Senha da conta nunca vai junto. `index.sqlite` não é exportado.
- Compressão deflate; com senha, AES-256 por entrada. Escrita em streaming, arquivo por arquivo.
- Grava em `<destino>.tmp` e renomeia no fim.
- Os nomes das entradas ficam visíveis no ZIP mesmo com senha (limitação aceita do formato).

**Importar**:
- O usuário escolhe o `.zip`; se as entradas estiverem cifradas, o app pede a senha.
- Cada `<email>/` vira uma conta sem credenciais em `%APPDATA%/mail-backup/accounts/<id>/`.
- Toda entrada é validada: caminho relativo, sem `..`, sem prefixo de drive. Entrada inválida → `UnsafePath`, ignorada.
- Depois de extrair: recria `folders` a partir de `account.json` (ou da estrutura de pastas, se faltar) e reindexa os `.eml`.
- Senha errada ou arquivo corrompido: a importação é abortada e a pasta parcial removida.
- Conta importada com o mesmo e-mail de uma conta existente: importar como nova conta com nome `"<email> (importado)"`. Não há mesclagem.

## 8. Erros

`enum AppError` (com `thiserror`), serializado para o front como `{ kind, message }`:

| kind | Tratamento |
|---|---|
| `AuthFailed` | Aborta a conta, mensagem "usuário ou senha inválidos" |
| `Network`, `Tls` | Até 3 reconexões com espera de 1s, 4s e 16s; depois a pasta vai para `failed_folders` e as outras continuam |
| `Io` | Aborta o backup/importação da conta, remove os `.tmp`, mantém o que já foi confirmado |
| `WrongPassword`, `CorruptArchive` | Aborta a importação e remove a extração parcial |
| `UnsafePath` | Rejeita a entrada/arquivo e continua |
| `Parse` | Mensagem salva mesmo assim; o índice fica com campos vazios (o `.eml` bruto é preservado) |

## 9. Testes

- **Unitários**:
  - `store`: decodificação UTF-7 modificado, caracteres proibidos, nomes reservados, colisões, resolução fora da pasta → erro;
  - `imap_sync`: filtro `uid > last_uid`;
  - `archive`: rejeição de zip-slip;
  - `commands`: lista de extensões bloqueadas.
- **Integração IMAP** contra GreenMail (`greenmail/standalone` em Docker, IMAPS na 3993), com `#[ignore]`:
  - backup completo; mensagens novas → incremental baixa só as novas;
  - interromper no meio de um lote e rodar de novo → contagem exata, sem duplicatas;
  - apagar e recriar a pasta (`UIDVALIDITY` muda) → sem duplicatas;
  - nenhuma mensagem com `\Seen` acrescentado no servidor.
- **Ida e volta**: exportar com senha → importar → hashes dos `.eml` idênticos e contagem no índice igual; senha errada → `WrongPassword`.
- **Manual**: conta UOL real (`imap.uhserver.com:993`); checklist da UI (cadastro, backup/cancelar, navegação, busca, leitura HTML, imagens, links, anexos, exportar/importar).

## 10. Riscos e pontos a validar

- Interceptação de links no Tauri v2 a partir de iframe sandbox: spike no começo; se não funcionar, os links aparecem como texto com botão "copiar".
- Escrita AES no crate `zip` (`aes-crypto`): confirmar a versão que suporta cifrar (não só decifrar); se faltar, reavaliar antes de implementar `archive.rs`.
- `async-imap` teve o último release em jan/2024; se surgir bug bloqueante, a alternativa é o cliente std do `io-imap` (Pimalaya) em `spawn_blocking`.
