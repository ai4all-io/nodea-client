import { CircleNotch, DownloadSimple, WarningCircle, Cube } from '@phosphor-icons/react'
import type { NodeState } from '@/hooks/useNodePhase'

/* Sidebar status widget — the single source of truth for "what is the node doing".
 * One card, two-segment progress track: 1 · Download, 2 · Load.
 * Clickable as the sharing toggle once the node is ready.
 */
export function NodeStatusWidget({ node }: { node: NodeState }) {
  const { phase, modelName, downloadProgress, sharing, setSharing, retry } = node
  const ready = phase === 'serving' || phase === 'paused'

  const header = {
    downloading: { icon: <DownloadSimple size={14} color="var(--n-accent-400)" />, title: 'Downloading', value: `${downloadProgress}%`, valueColor: 'var(--n-accent-300)' },
    loading: { icon: <CircleNotch size={14} color="var(--n-accent-400)" className="n-spin" />, title: 'Loading', value: '—', valueColor: 'var(--n-text-3)' },
    connecting: { icon: <CircleNotch size={14} color="var(--n-accent-400)" className="n-spin" />, title: 'Connecting', value: '—', valueColor: 'var(--n-text-3)' },
    stalled: { icon: <WarningCircle size={14} color="var(--n-text-2)" />, title: 'Download stalled', value: `${downloadProgress}%`, valueColor: 'var(--n-text-3)' },
    offline: { icon: <WarningCircle size={14} color="var(--n-text-2)" />, title: 'Offline', value: '—', valueColor: 'var(--n-text-3)' },
    refused: { icon: <WarningCircle size={14} color="var(--n-text-2)" />, title: 'Not sharing', value: '—', valueColor: 'var(--n-text-3)' },
    serving: { icon: <Dot on />, title: 'Sharing', value: '100%', valueColor: 'var(--n-accent-300)' },
    paused: { icon: <Dot />, title: 'Paused', value: '100%', valueColor: 'var(--n-text-3)' },
  }[phase]

  const caption = {
    downloading: `${((downloadProgress / 100) * 4.9).toFixed(1)} of 4.9 GB · 24 MB/s`,
    loading: 'Mapping 4.9 GB onto Metal · about 20s',
    connecting: 'Reaching the coordinator…',
    stalled: 'The model failed. Retry to pick up where it left off.',
    offline: "Can't reach the coordinator. Retrying on its own.",
    refused: 'The server refused this account. See the Node page.',
    serving: null,
    paused: 'Not taking new work. The model stays loaded, so resuming is instant.',
  }[phase]

  const bar1 = phase === 'downloading' || phase === 'stalled' ? `${downloadProgress}%` : '100%'
  const bar1Color = phase === 'stalled' ? 'var(--n-mute)' : phase === 'downloading' ? 'var(--n-accent)' : 'var(--n-accent-700)'
  const onStep1 = phase === 'downloading' || phase === 'stalled'
  const onStep2 = phase === 'loading'

  const body = (
    <>
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-[7px]">
          {header.icon}
          <span className="text-[14px] font-semibold leading-none text-[var(--n-text)]">{header.title}</span>
        </div>
        <span className="font-mono text-[13px] font-semibold leading-none" style={{ color: header.valueColor }}>
          {header.value}
        </span>
      </div>

      <div className="mt-2 flex items-center gap-1.5">
        <Cube size={13} color="var(--n-text-4)" />
        <span className="truncate text-[13px] leading-[1.3] text-[var(--n-text-2)]">{modelName}</span>
      </div>

      {/* two-segment track */}
      <div className="mt-[9px] flex gap-1">
        <span className="block h-1 flex-1 overflow-hidden rounded-[2px] bg-[var(--n-border)]">
          <span className="block h-full rounded-[2px] transition-[width] duration-[400ms] ease-out"
                style={{ width: bar1, background: bar1Color }} />
        </span>
        <span className="relative block h-1 flex-1 overflow-hidden rounded-[2px] bg-[var(--n-border)]">
          {phase === 'loading' ? (
            <span className="n-slide absolute left-0 top-0 h-full w-[30%] rounded-[2px] bg-[var(--n-accent)]" />
          ) : ready ? (
            <span className="block h-full w-full rounded-[2px] bg-[var(--n-accent-700)]" />
          ) : null}
        </span>
      </div>

      {!ready && (
        <div className="mt-1.5 flex justify-between text-[11px] font-semibold uppercase leading-none tracking-[0.05em]">
          <span style={{ color: onStep1 ? 'var(--n-accent-300)' : 'var(--n-text-4)' }}>
            {onStep1 ? '1 · Download' : '1 · Done'}
          </span>
          <span style={{ color: onStep2 ? 'var(--n-accent-300)' : 'var(--n-text-4)' }}>2 · Load</span>
        </div>
      )}

      {caption && <div className="mt-[9px] text-[13px] leading-[1.4] text-[var(--n-text-3)]">{caption}</div>}

      {(phase === 'stalled' || phase === 'offline') && (
        <button onClick={retry}
                className="mt-2.5 w-full rounded-[7px] border border-[var(--n-accent-700)] py-[7px] text-[13.5px] font-semibold leading-none text-[var(--n-accent-300)] transition-colors hover:bg-[var(--n-tint-hover-strong)]">
          Retry now
        </button>
      )}

      {ready && (
        <div className="mt-2.5 flex items-center justify-between gap-2 border-t border-[var(--n-border)] pt-[9px]">
          <span className="text-[13px] font-medium leading-none text-[var(--n-text-2)]">Sharing</span>
          <Switch on={sharing} />
        </div>
      )}
    </>
  )

  const shell = 'mx-3 mb-3 rounded-[10px] bg-[var(--n-raised)] p-3 text-left'

  return ready ? (
    <button className={`${shell} block w-[calc(100%-24px)] border transition-colors`}
            style={{ borderColor: sharing ? 'var(--n-accent-800)' : 'var(--n-border)' }}
            onClick={() => setSharing(!sharing)}
            aria-pressed={sharing}>
      {body}
    </button>
  ) : (
    <div className={`${shell} border border-[var(--n-border)]`}>{body}</div>
  )
}

function Dot({ on = false }: { on?: boolean }) {
  return (
    <span className={`h-[7px] w-[7px] rounded-full ${on ? 'n-pulse' : ''}`}
          style={{ background: on ? 'var(--n-accent-400)' : 'var(--n-mute-2)' }} />
  )
}

function Switch({ on }: { on: boolean }) {
  return (
    <span className="relative inline-block h-[18px] w-8 rounded-[9px] transition-colors duration-[180ms]"
          style={{ background: on ? 'var(--n-accent-700)' : 'var(--n-border)' }}>
      <span className="absolute top-0.5 h-3.5 w-3.5 rounded-full transition-[left,background] duration-[180ms]"
            style={{ left: on ? 16 : 2, background: on ? 'var(--n-accent-200)' : 'var(--n-text-4)' }} />
    </span>
  )
}
