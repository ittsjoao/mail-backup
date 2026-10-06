<script>
  import Accounts from './lib/Accounts.svelte'
  import Browser from './lib/Browser.svelte'

  let account = $state(null)
</script>

<header>
  <h1>Mail Backup</h1>
  {#if account}<button onclick={() => (account = null)}>← Contas</button> <span>{account.name}</span>{/if}
</header>

<!-- Contas fica montada (só oculta) para não perder o estado de um backup em andamento -->
<div hidden={!!account}><Accounts onbrowse={(a) => (account = a)} /></div>
{#if account}<Browser {account} />{/if}

<style>
  :global(body) { margin: 0; font: 14px system-ui, sans-serif; color: #1d1d1f; background: #fafafa; }
  :global(button) { padding: 4px 10px; cursor: pointer; }
  :global(input) { padding: 4px 6px; }
  :global(.err) { color: #b00020; }
  :global(.info) { color: #1b5e20; }
  :global(.inline) { display: flex; gap: 8px; align-items: center; margin: 8px 0; }
  :global(.card) { display: grid; gap: 6px; max-width: 480px; padding: 12px; background: #fff; border: 1px solid #ddd; margin: 8px 0; }
  :global(.card label) { display: grid; grid-template-columns: 130px 1fr; gap: 8px; align-items: center; }
  :global(table) { width: 100%; border-collapse: collapse; }
  :global(td), :global(th) { padding: 6px; border-bottom: 1px solid #eee; text-align: left; vertical-align: top; }
  :global(.actions) { white-space: nowrap; }
  header { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid #ddd; background: #fff; }
  header h1 { font-size: 16px; margin: 0; }
  :global(.accounts) { padding: 12px; }
</style>
