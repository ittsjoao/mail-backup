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
  .row { position: absolute; left: 0; right: 0; display: grid; gap: 2px; text-align: left; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; background: var(--surface); padding: 6px 10px; }
  .row.sel { background: var(--primary-soft); }
  .top { display: flex; justify-content: space-between; gap: 8px; overflow: hidden; white-space: nowrap; }
  .subj { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--muted); }
  .empty { padding: 12px; color: var(--muted); }
</style>
