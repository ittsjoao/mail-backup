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
    try {
      accounts = await api.listAccounts()
      selected = selected.filter((id) => accounts.some((a) => a.id === id))
    } catch (e) { error = errMsg(e) }
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
