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
