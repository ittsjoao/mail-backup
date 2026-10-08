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
