import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export const api = {
  listAccounts: () => invoke('list_accounts'),
  saveAccount: (input) => invoke('save_account', { input }),
  deleteAccount: (id, deleteFiles) => invoke('delete_account', { id, deleteFiles }),
  testConnection: (input) => invoke('test_connection', { input }),
  pickFolder: () => invoke('pick_folder'),
  runBackup: (ids) => invoke('run_backup', { ids }),
  cancelBackup: () => invoke('cancel_backup'),
  listFolders: (accountId) => invoke('list_folders', { accountId }),
  listMessages: (accountId, query) => invoke('list_messages', { accountId, query }),
  getMessage: (accountId, messageId, allowRemoteImages) => invoke('get_message', { accountId, messageId, allowRemoteImages }),
  openEml: (accountId, messageId) => invoke('open_eml', { accountId, messageId }),
  saveAttachment: (accountId, messageId, index) => invoke('save_attachment', { accountId, messageId, index }),
  openAttachment: (accountId, messageId, index) => invoke('open_attachment', { accountId, messageId, index }),
  reindex: (accountId) => invoke('reindex_account', { accountId }),
  exportAccounts: (ids, password) => invoke('export_accounts', { ids, password }),
  pickArchive: () => invoke('pick_archive'),
  importArchive: (path, password) => invoke('import_archive', { path, password }),
  onProgress: (cb) => listen('backup://progress', (e) => cb(e.payload)),
  onStatus: (cb) => listen('backup://status', (e) => cb(e.payload)),
  getMaxParallel: () => invoke('get_max_parallel'),
  setMaxParallel: (n) => invoke('set_max_parallel', { n }),
}

export const errMsg = (e) => (e && e.message) || String(e)
export const fmtDate = (ts) => (ts ? new Date(ts * 1000).toLocaleString('pt-BR') : '')

// use:modal — abre o <dialog> como modal assim que entra na tela
export const modal = (node) => { node.showModal() }
