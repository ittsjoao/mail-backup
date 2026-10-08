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
