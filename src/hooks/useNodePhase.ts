import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { listen } from '@tauri-apps/api/event'
import { isDemoMode, mockInvoke, mockListen, initMockTauri } from '@/lib/mockTauri'
import { useAuth } from '@/contexts/AuthContext'

/* Existing backend shapes — see src-tauri/src/status.rs */
export interface ConnectionStatus {
  type: 'Disconnected' | 'Connecting' | 'Connected' | 'Error' | 'DeviceLimitReached' | 'Refused'
  message?: string
}
export interface ModelStatus {
  type: 'NoModel' | 'Downloading' | 'Loading' | 'Ready' | 'Error'
  model_name?: string
  progress?: number
  message?: string
}
interface StatusSnapshot {
  connection: ConnectionStatus
  model: ModelStatus
}

/* The phases the UI renders. Derived, never stored.
 *
 * `stalled` is the model failing (download or load). Connection problems are
 * their own phases: `offline` (can't reach the coordinator; retries on its
 * own) and `refused` (the server refused this account - deleted or
 * deactivated). Mapping every connection Error to `stalled` is how a leave in
 * the web console read as "The download stopped" (#241). */
export type NodePhase =
  | 'connecting'
  | 'downloading'
  | 'loading'
  | 'serving'
  | 'paused'
  | 'stalled'
  | 'offline'
  | 'refused'

export interface NodeState {
  phase: NodePhase
  modelName: string
  /** 0-100, only meaningful while downloading */
  downloadProgress: number
  message?: string
  sharing: boolean
  setSharing: (on: boolean) => void
  retry: () => void
}

const SHARING_KEY = 'nodea.sharing'

export function derivePhase(
  connection: ConnectionStatus | undefined,
  model: ModelStatus | undefined,
  sharing: boolean,
  accountInactive: boolean
): NodePhase {
  if (accountInactive || connection?.type === 'Refused') return 'refused'
  if (model?.type === 'Error') return 'stalled'
  if (connection?.type === 'Error') return 'offline'
  if (model?.type === 'Downloading') return 'downloading'
  if (model?.type === 'Loading') return 'loading'
  if (model?.type === 'Ready' && connection?.type === 'Connected') return sharing ? 'serving' : 'paused'
  if (!sharing) return 'paused'
  return 'connecting'
}

export function useNodePhase(): NodeState {
  const demo = isDemoMode()
  const { accountInactive } = useAuth()
  const [snapshot, setSnapshot] = useState<StatusSnapshot | null>(null)
  const [sharing, setSharingState] = useState(() => {
    if (demo) {
      // ?phase=paused|serving (see mockTauri.ts) pins sharing before the
      // mock lifecycle even starts - reading localStorage here would be too
      // late, since the mock only updates it from an effect after mount.
      const forcedPhase = new URLSearchParams(window.location.search).get('phase')
      if (forcedPhase === 'paused') return false
      if (forcedPhase === 'serving') return true
    }
    return localStorage.getItem(SHARING_KEY) !== 'false'
  })

  useEffect(() => {
    if (demo) initMockTauri()
    const inv = demo ? mockInvoke : invoke
    const lis = demo ? mockListen : listen
    let cancelled = false

    inv<StatusSnapshot>('get_all_status')
      .then((s) => {
        if (cancelled) return
        setSnapshot(s)
        if (s.connection.type === 'Disconnected' && sharing) {
          inv('connect_grpc').catch(console.error)
        }
      })
      .catch(console.error)

    const unlisten = [
      lis<ConnectionStatus>('connection-status-changed', (e) =>
        setSnapshot((p) => (p ? { ...p, connection: e.payload } : p))
      ),
      lis<ModelStatus>('model-status-changed', (e) =>
        setSnapshot((p) => (p ? { ...p, model: e.payload } : p))
      ),
      // The tray's "Pause/Resume sharing" item (src-tauri/src/main.rs) calls
      // connect_grpc/disconnect_grpc directly and emits this so the app's own
      // Sharing toggle - a local preference this hook doesn't otherwise
      // re-derive from connection state - doesn't go stale.
      lis<boolean>('sharing-changed', (e) => {
        setSharingState(e.payload)
        localStorage.setItem(SHARING_KEY, String(e.payload))
      }),
    ]

    return () => {
      cancelled = true
      unlisten.forEach((p) => p.then((off) => off()))
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [demo])

  const setSharing = (on: boolean) => {
    setSharingState(on)
    localStorage.setItem(SHARING_KEY, String(on))
    const inv = demo ? mockInvoke : invoke
    inv(on ? 'connect_grpc' : 'disconnect_grpc').catch(console.error)
  }

  const retry = () => {
    const inv = demo ? mockInvoke : invoke
    inv('connect_grpc').catch(console.error)
  }

  const connection = snapshot?.connection
  const model = snapshot?.model

  const phase = derivePhase(connection, model, sharing, accountInactive)

  return {
    phase,
    modelName: model?.model_name ?? 'Llama 3.1 8B Instruct',
    downloadProgress: Math.round(model?.progress ?? 0),
    // The message that explains the phase: the model's for a model failure,
    // the connection's for an offline or refused node.
    message: phase === 'stalled' ? model?.message : connection?.message ?? model?.message,
    sharing,
    setSharing,
    retry,
  }
}
