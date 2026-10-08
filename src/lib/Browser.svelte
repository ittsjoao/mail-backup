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
  let refresh = $state(0)
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
    try { await api.reindex(account.id); await loadFolders(); folderId = null; selectedId = null; refresh++ } catch (e) { error = errMsg(e) }
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
      {#key refresh}<MessageList accountId={account.id} {folderId} {text} dateFrom={ts(from)} dateTo={ts(to, 1)} {selectedId} onselect={(id) => (selectedId = id)} />{/key}
    </div>
  </section>
  <main>
    {#if selectedId}<Reader accountId={account.id} messageId={selectedId} />{:else}<p class="empty">Selecione uma mensagem</p>{/if}
  </main>
</div>

<style>
  .browser { display: grid; grid-template-columns: 220px 380px 1fr; height: calc(100vh - 45px); }
  aside { overflow-y: auto; border-right: 1px solid var(--border); display: flex; flex-direction: column; background: var(--bg); }
  aside button { text-align: left; border: 0; background: none; padding: 6px 10px; display: flex; justify-content: space-between; }
  aside button.sel { background: var(--primary-soft); }
  .reindex { margin-top: auto; color: var(--muted); }
  section { display: flex; flex-direction: column; border-right: 1px solid var(--border); min-height: 0; }
  .search { margin: 8px 8px 4px; }
  .dates { display: flex; gap: 8px; margin: 0 8px 8px; font-size: 12px; }
  .listwrap { flex: 1; min-height: 0; }
  main { min-width: 0; min-height: 0; }
  .empty { padding: 16px; color: var(--muted); }
</style>
