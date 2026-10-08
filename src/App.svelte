<script>
  import Accounts from './lib/Accounts.svelte'
  import Browser from './lib/Browser.svelte'

  let account = $state(null)
</script>

<header>
  <h1>Mail Backup</h1>
  {#if account}<button class="ghost" onclick={() => (account = null)}>← Contas</button> <span class="muted">{account.name}</span>{/if}
</header>

<!-- Contas fica montada (só oculta) para não perder o estado de um backup em andamento -->
<div hidden={!!account}><Accounts onbrowse={(a) => (account = a)} /></div>
{#if account}<Browser {account} />{/if}

<style>
  :global(:root) {
    --bg: #f4f4f5; --surface: #fff; --border: #e4e4e7; --hover: #f4f4f5; --text: #18181b; --muted: #71717a;
    --primary: #4f46e5; --primary-hover: #4338ca; --primary-soft: #e0e7ff; --primary-soft-text: #3730a3;
    --ok-soft: #dcfce7; --ok-text: #166534; --bad: #dc2626; --bad-soft: #fee2e2; --bad-text: #991b1b;
    --shadow: 0 1px 2px rgb(0 0 0 / .05); --shadow-lg: 0 12px 32px rgb(0 0 0 / .18);
    color-scheme: light dark;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --bg: #09090b; --surface: #18181b; --border: #27272a; --hover: #27272a; --text: #fafafa; --muted: #a1a1aa;
      --primary: #6366f1; --primary-hover: #4f46e5; --primary-soft: #1e1b4b; --primary-soft-text: #c7d2fe;
      --ok-soft: #052e16; --ok-text: #86efac; --bad-soft: #450a0a; --bad-text: #fca5a5;
      --shadow: 0 1px 2px rgb(0 0 0 / .4); --shadow-lg: 0 12px 32px rgb(0 0 0 / .6);
    }
  }
  :global(body) { margin: 0; font: 14px/1.4 system-ui, 'Segoe UI', sans-serif; color: var(--text); background: var(--bg); }
  :global(button) { font: inherit; padding: 6px 12px; cursor: pointer; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  :global(button:hover:not(:disabled)) { background: var(--hover); }
  :global(button:disabled) { opacity: .5; cursor: default; }
  :global(button.primary) { background: var(--primary); border-color: var(--primary); color: #fff; font-weight: 600; }
  :global(button.primary:hover:not(:disabled)) { background: var(--primary-hover); }
  :global(button.ghost) { background: transparent; border-color: transparent; color: var(--muted); }
  :global(button.ghost:hover:not(:disabled)) { background: var(--hover); color: var(--text); }
  :global(button.danger) { background: var(--bad); border-color: var(--bad); color: #fff; font-weight: 600; }
  :global(button.danger:hover:not(:disabled)) { background: var(--bad); filter: brightness(.9); }
  :global(button.icon) { padding: 2px 8px; }
  :global(input) { font: inherit; padding: 6px 8px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--text); }
  :global(input[type='checkbox']) { accent-color: var(--primary); }
  :global(button:focus-visible), :global(input:focus-visible) { outline: 2px solid var(--primary); outline-offset: 1px; }
  :global(.muted) { color: var(--muted); }
  :global(.spacer) { flex: 1; }
  :global(.err) { color: var(--bad-text); }
  :global(.info) { color: var(--ok-text); }
  :global(.inline) { display: flex; gap: 8px; align-items: center; margin: 8px 0; }
  :global(.chip) { font-size: 11px; font-weight: 600; padding: 2px 8px; border-radius: 999px; white-space: nowrap; background: var(--hover); color: var(--muted); border: 1px solid var(--border); }
  :global(.chip.run) { background: var(--primary-soft); color: var(--primary-soft-text); border-color: transparent; }
  :global(.chip.ok) { background: var(--ok-soft); color: var(--ok-text); border-color: transparent; }
  :global(.chip.bad) { background: var(--bad-soft); color: var(--bad-text); border-color: transparent; }
  :global(.alert) { display: flex; gap: 8px; align-items: center; margin: 0 0 12px; padding: 8px 12px; border-radius: 10px; }
  :global(.alert.ok) { background: var(--ok-soft); color: var(--ok-text); }
  :global(.alert.bad) { background: var(--bad-soft); color: var(--bad-text); }
  :global(.alert button) { margin-left: auto; color: inherit; }
  :global(dialog) { border: 1px solid var(--border); border-radius: 14px; padding: 20px; background: var(--surface); color: var(--text); box-shadow: var(--shadow-lg); width: min(560px, calc(100vw - 32px)); box-sizing: border-box; }
  :global(dialog::backdrop) { background: rgb(0 0 0 / .45); }
  :global(dialog h2) { font-size: 16px; margin: 0 0 14px; }
  :global(dialog footer) { display: flex; gap: 8px; align-items: center; margin-top: 16px; }
  /* altura fixa: Browser.svelte calcula 100vh - 45px */
  header { display: flex; gap: 12px; align-items: center; height: 45px; box-sizing: border-box; padding: 0 16px; border-bottom: 1px solid var(--border); background: var(--surface); }
  header h1 { font-size: 15px; margin: 0; font-weight: 700; letter-spacing: -.01em; }
</style>
