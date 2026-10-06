# Checklist de QA — mail-backup

## Pré-requisitos

- GreenMail não é necessário: o app só conecta via TLS, então todo o QA usa uma conta real em `imap.uhserver.com:993` (usuário = e-mail completo).
- `npm run tauri dev` (com `export PATH="$HOME/.cargo/bin:$PATH"` no Git Bash).
- Conta com mensagens (várias pastas, algumas não lidas) e acesso ao webmail para conferir.

## Resultados do build

Medidos em 2026-10-06, Windows 10, build de release.

- `npx tauri build --no-bundle`: OK. `mail-backup.exe` = 7.585.792 bytes (~7,2 MiB).
- RAM em repouso (8 s após abrir, Working Set): `mail-backup.exe` 27,0 MB + 6 processos `msedgewebview2.exe` filhos (94,5 + 8,3 + 51,6 + 31,2 + 18,6 + 56,1 = 260,3 MB) = **~287 MB total**.
- RAM durante um backup: _(a preencher pelo usuário)_
- `npx tauri build`: OK, 2 instaladores em `src-tauri/target/release/bundle/`:
  - `msi/mail-backup_0.1.0_x64_en-US.msi` — 3.948.544 bytes (3,77 MiB)
  - `nsis/mail-backup_0.1.0_x64-setup.exe` — 2.959.741 bytes (2,82 MiB)
- Instalação do instalador e repetição do item 1 de "Backup real UOL": _(a preencher pelo usuário)_

## Contas (Task 8, Step 4)

- [ ] Lista vazia; "Adicionar conta" abre o formulário com `imap.uhserver.com`/993.
- [ ] Salvar uma conta com senha → aparece na tabela sem "somente leitura"; `%APPDATA%\mail-backup\config.json` não contém a senha.
- [ ] "Testar conexão" com senha errada → mensagem "usuário ou senha inválidos".
- [ ] "Editar" → salvar sem senha mantém `has_credentials`.

## Navegador de pastas e leitor (Task 9, Step 5)

- [ ] "Abrir" numa conta mostra a árvore de pastas com contagens.
- [ ] Rolar uma pasta com milhares de mensagens é fluido; só ~30 linhas existem no DOM (DevTools → Elements).
- [ ] Busca por parte do assunto/remetente filtra após ~300 ms; `50%` busca literalmente. Os campos "De/Até" restringem por data (o dia do "Até" entra inteiro).
- [ ] E-mail HTML aparece no iframe; imagens remotas só após "Carregar imagens".
- [ ] **Spike dos links:** clicar num link dentro do e-mail abre o navegador padrão e não navega o app. Se falhar, avisar o desenvolvedor (fallback da spec: em `message.rs::wrap`, acrescentar ao `<style>` `a{pointer-events:none;color:inherit}`; links aparecem como texto). Não trocar o `sandbox` para liberar navegação.
- [ ] Anexo `.pdf` → "Salvar como…" grava o arquivo; "Abrir" abre no leitor padrão. Anexo `.exe` não tem botão "Abrir".
- [ ] "Abrir no programa de e-mail" abre o `.eml` no cliente padrão (ou pergunta qual app usar).
- [ ] E-mail com `<a target="_self" href="https://...">` e com `<meta http-equiv="refresh" content="0;url=https://...">` não navega dentro do leitor (o iframe continua mostrando o e-mail).
- [ ] Anexo `.docx` → "Abrir" abre no Word em Modo Protegido (arquivo marcado como vindo da internet).

## Backup real UOL (Task 10, Step 1)

- [ ] "Testar conexão" OK; senha errada → "usuário ou senha inválidos".
- [ ] Backup completo: todas as pastas do webmail aparecem em `<dir>/mail/` com nomes em português corretos (Enviados, Lixeira, Rascunhos, acentos).
- [ ] No webmail, uma mensagem não lida continua não lida após o backup.
- [ ] Segundo backup imediato → "0 novas" e rápido.
- [ ] Enviar um e-mail para a conta, rodar backup → "1 novas".
- [ ] Durante um backup grande, desligar o Wi-Fi por ~10 s e religar → o backup continua ou a pasta aparece em "Falharam"; rodar de novo completa sem duplicar (conferir contagem no navegador vs. webmail).
- [ ] "Cancelar" durante o backup → para entre lotes; rodar de novo completa.
- [ ] Iniciar um backup, clicar "Abrir" numa conta e voltar com "← Contas" → "Cancelar" continua visível, o progresso segue e os botões Remover/Editar/Backup continuam desabilitados; o resultado aparece ao terminar.
- [ ] Navegar a conta enquanto o backup roda → sem erro "database is locked".

## Exportar/importar real (Task 10, Step 2)

- [ ] Selecionar a conta, senha `teste123`, exportar → abrir o `.zip` no 7-Zip: pede senha; com a senha, os `.eml` abrem.
- [ ] "Importar backup…" do mesmo arquivo → pede senha; senha errada → "Senha incorreta."; correta → conta "`<email>` (importado)", somente leitura, mesma contagem de mensagens.

## Instalador e RAM (Task 10, Step 3)

- [ ] Instalar o NSIS ou MSI e repetir o primeiro item de "Backup real UOL".
- [ ] Anotar a RAM (`mail-backup.exe` + `msedgewebview2.exe` filhos) durante um backup na seção "Resultados do build".
