<script>
  import { api, errMsg, fmtDate } from './api.js'

  let { accountId, messageId } = $props()
  let view = $state(null)
  let remote = $state(false)
  let error = $state('')

  async function load(a, m, allowRemote) {
    error = ''
    try { view = await api.getMessage(a, m, allowRemote) } catch (e) { error = errMsg(e); view = null }
  }

  $effect(() => {
    const a = accountId, m = messageId
    remote = false
    load(a, m, false)
  })

  const kb = (n) => (n < 1024 ? `${n} B` : `${Math.round(n / 1024)} KB`)
  const act = (p) => p.catch((e) => (error = errMsg(e)))
</script>

{#if error}<p class="err">{error}</p>{/if}
{#if view}
  <div class="reader">
    <div class="head">
      <h2>{view.subject || '(sem assunto)'}</h2>
      <div>De: {view.from}</div>
      <div>Para: {view.to}</div>
      {#if view.cc}<div>Cc: {view.cc}</div>{/if}
      <div><small>{fmtDate(view.date)}</small></div>
      <div class="inline">
        {#if view.has_remote_images && !remote}
          <button onclick={() => { remote = true; load(accountId, messageId, true) }}>Carregar imagens</button>
        {/if}
        <button onclick={() => act(api.openEml(accountId, messageId))}>Abrir no programa de e-mail</button>
      </div>
      {#if view.attachments.length}
        <ul class="att">
          {#each view.attachments as a (a.index)}
            <li>
              {a.name} <small>({kb(a.size)})</small>
              <button onclick={() => act(api.saveAttachment(accountId, messageId, a.index))}>Salvar como…</button>
              {#if !a.blocked}<button onclick={() => act(api.openAttachment(accountId, messageId, a.index))}>Abrir</button>{/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <iframe title="Conteúdo do e-mail" sandbox="allow-popups allow-popups-to-escape-sandbox" srcdoc={view.html}></iframe>
  </div>
{/if}

<style>
  .reader { display: flex; flex-direction: column; height: 100%; }
  .head { padding: 10px 12px; border-bottom: 1px solid #ddd; background: #fff; }
  .head h2 { font-size: 16px; margin: 0 0 6px; }
  .att { margin: 6px 0 0; padding-left: 18px; }
  iframe { flex: 1; border: 0; width: 100%; background: #fff; }
</style>
