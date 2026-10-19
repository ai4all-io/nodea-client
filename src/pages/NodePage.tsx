import { useEffect, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { CircleNotch, DownloadSimple, CheckCircle, CircleDashed, Broadcast, WifiSlash, Prohibit } from '@phosphor-icons/react'
import { useAuth } from '@/contexts/AuthContext'
import { apiClient, type TokenStatsResponse } from '@/lib/api'
import { ThroughputChart } from '@/components/ThroughputChart'
import type { NodeState } from '@/hooks/useNodePhase'

export function NodePage() {
  const node = useOutletContext<NodeState>()
  const { organization, accountInactive } = useAuth()
  const [stats, setStats] = useState<TokenStatsResponse[]>([])

  useEffect(() => {
    const refresh = () => apiClient.getConsumedTokens().then(setStats).catch(console.error)
    refresh()
    // The stat strip and chart reflect real backend usage (Playground
    // requests, or actual served traffic) - poll instead of fetching once,
    // or the chart just freezes at whatever existed when the page mounted.
    const interval = setInterval(refresh, 15000)
    return () => clearInterval(interval)
  }, [])

  const gettingReady =
    node.phase === 'downloading' || node.phase === 'loading' || node.phase === 'connecting' || node.phase === 'stalled'

  const hero = {
    downloading: {
      kicker: 'Stage 1 of 2 · Downloading',
      KickerIcon: DownloadSimple,
      spin: false,
      title: 'Fetching the model for this Mac.',
      body: `${node.modelName}, chosen for this hardware. It downloads once and stays cached.`,
      ringPct: node.downloadProgress,
      ringLabel: `${node.downloadProgress}%`,
      ringCaption: 'downloaded',
      ringColor: 'var(--n-accent)',
    },
    loading: {
      kicker: 'Stage 2 of 2 · Loading',
      KickerIcon: CircleNotch,
      spin: true,
      title: 'Loading the model into memory.',
      body: 'Weights are on disk. Nodea is mapping them onto the GPU — this takes about twenty seconds and happens once per launch.',
      ringPct: 100,
      ringLabel: '—',
      ringCaption: 'warming up',
      ringColor: 'var(--n-accent-700)',
    },
    connecting: {
      kicker: 'Connecting',
      KickerIcon: CircleNotch,
      spin: true,
      title: 'Reaching the coordinator.',
      body: 'Nodea is opening a connection for this Mac.',
      ringPct: 0,
      ringLabel: '—',
      ringCaption: 'connecting',
      ringColor: 'var(--n-mute-2)',
    },
    stalled: {
      kicker: 'Interrupted',
      KickerIcon: DownloadSimple,
      spin: false,
      title: 'The download stopped.',
      body: node.message ?? 'Nodea picks up where it left off when you retry.',
      ringPct: node.downloadProgress,
      ringLabel: `${node.downloadProgress}%`,
      ringCaption: 'downloaded',
      ringColor: 'var(--n-mute-2)',
    },
    offline: {
      kicker: 'Offline',
      KickerIcon: WifiSlash,
      spin: false,
      title: "Can't reach the coordinator.",
      body: 'Nodea keeps retrying on its own. The model stays on this Mac, so sharing resumes as soon as the connection does.',
      ringPct: 0,
      ringLabel: '—',
      ringCaption: 'offline',
      ringColor: 'var(--n-mute-2)',
    },
    refused: accountInactive
      ? {
          kicker: 'Account deactivated',
          KickerIcon: Prohibit,
          spin: false,
          title: `An admin deactivated your account${organization?.name ? ` in ${organization.name}` : ''}.`,
          body: 'This Mac stopped sharing compute. Ask an admin to reactivate you, or leave the organization in Settings.',
          ringPct: 0,
          ringLabel: '—',
          ringCaption: 'not sharing',
          ringColor: 'var(--n-mute-2)',
        }
      : {
          kicker: 'Not sharing',
          KickerIcon: Prohibit,
          spin: false,
          title: 'The server refused this account.',
          body: node.message ? `${node.message}. This Mac stopped sharing compute.` : 'This Mac stopped sharing compute.',
          ringPct: 0,
          ringLabel: '—',
          ringCaption: 'not sharing',
          ringColor: 'var(--n-mute-2)',
        },
    serving: {
      kicker: null,
      KickerIcon: Broadcast,
      spin: false,
      title: 'This Mac is serving the network.',
      body: '',
      ringPct: 71,
      ringLabel: '71%',
      ringCaption: 'capacity used',
      ringColor: 'var(--n-accent)',
    },
    paused: {
      kicker: null,
      KickerIcon: Broadcast,
      spin: false,
      title: 'Paused. This Mac is idle.',
      body: `${node.modelName} is still in memory. Turn sharing back on and it picks up within seconds.`,
      ringPct: 0,
      ringLabel: '0%',
      ringCaption: 'capacity used',
      ringColor: 'var(--n-mute-2)',
    },
  }[node.phase]

  return (
    <div className="px-[30px] pt-[30px]">
      <div className="flex items-start justify-between gap-6">
        <div>
          {hero.kicker && (
            <div className="flex items-center gap-2 text-[13px] font-semibold uppercase leading-none tracking-[0.09em] text-[var(--n-accent-400)]">
              <hero.KickerIcon size={14} className={hero.spin ? 'n-spin' : undefined} />
              {node.phase === 'serving' ? `Serving · ${organization?.name}` : hero.kicker}
            </div>
          )}
          <h1 className="mt-3.5 max-w-[440px] text-[32px] font-semibold leading-[1.15] tracking-[-0.02em] text-[var(--n-text)] [text-wrap:pretty]">
            {hero.title}
          </h1>
          {hero.body && (
            <p className="mt-2.5 max-w-[400px] text-[15px] leading-[1.6] text-[var(--n-text-4)]">{hero.body}</p>
          )}
        </div>
        <CapacityRing pct={hero.ringPct} label={hero.ringLabel} caption={hero.ringCaption} color={hero.ringColor} />
      </div>

      {gettingReady ? <ReadyChecklist node={node} /> : (
        <>
          <StatStrip stats={stats} />
          <ThroughputChart stats={stats} />
        </>
      )}
    </div>
  )
}

function CapacityRing({ pct, label, caption, color }: { pct: number; label: string; caption: string; color: string }) {
  const CIRC = 440
  return (
    <div className="relative h-[168px] w-[168px] flex-none">
      <svg width="168" height="168" viewBox="0 0 168 168">
        <circle cx="84" cy="84" r="70" fill="none" stroke="var(--n-line-strong)" strokeWidth="10" />
        <circle cx="84" cy="84" r="70" fill="none" stroke={color} strokeWidth="10" strokeLinecap="round"
                strokeDasharray={CIRC} strokeDashoffset={CIRC - (CIRC * pct) / 100}
                transform="rotate(-90 84 84)"
                style={{ transition: 'stroke-dashoffset .4s ease, stroke .3s ease' }} />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-[3px]">
        <span className="text-[28px] font-semibold leading-none tracking-[-0.02em] text-[var(--n-text)]">{label}</span>
        <span className="text-[12.5px] font-medium uppercase leading-none tracking-[0.06em] text-[var(--n-text-3)]">{caption}</span>
      </div>
    </div>
  )
}

/**
 * Only "Tokens today" is real (apiClient.getConsumedTokens()). The design's
 * Requests / Median latency / Uptime cells have no backend field yet, so
 * they're dropped rather than shown as invented numbers.
 */
function StatStrip({ stats }: { stats: TokenStatsResponse[] }) {
  const today = stats.reduce((s, x) => s + x.value, 0)
  return (
    <div className="mt-[26px] w-fit overflow-hidden rounded-[12px] border border-[var(--n-line)] bg-[var(--n-sidebar)]">
      <div className="px-4 py-3.5">
        <div className="text-[12.5px] font-medium uppercase leading-none tracking-[0.06em] text-[var(--n-text-3)]">Tokens today</div>
        <div className="mt-[7px] text-[23px] font-semibold leading-none text-[var(--n-text)]">
          {new Intl.NumberFormat('en-US').format(today)}
        </div>
      </div>
    </div>
  )
}

function ReadyChecklist({ node }: { node: NodeState }) {
  const gb = ((node.downloadProgress / 100) * 4.9).toFixed(1)
  const dl = node.phase === 'downloading' || node.phase === 'stalled'
  const ld = node.phase === 'loading'

  const rows = [
    {
      title: 'Fetching weights',
      sub: dl ? `${gb} of 4.9 GB at 24 MB/s` : '4.9 GB cached locally',
      value: dl ? 'In progress' : 'Done',
      state: dl ? 'busy' : 'done',
    },
    {
      title: 'Loading into memory',
      sub: ld ? 'Metal · unified memory' : dl ? 'Waiting for the download' : 'Metal · unified memory',
      value: ld ? '~20s' : dl ? '—' : '18s',
      state: ld ? 'busy' : dl ? 'idle' : 'done',
    },
    {
      title: 'Accepting requests',
      sub: dl || ld ? 'Starts on its own when the model is ready' : 'First request answered moments ago',
      value: dl || ld ? '—' : 'Live',
      state: dl || ld ? 'idle' : 'live',
    },
  ] as const

  return (
    <div className="mt-[30px] overflow-hidden rounded-[12px] border border-[var(--n-line)]">
      {rows.map((r, i) => (
        <div key={r.title}
             className={`flex items-center gap-3 bg-[var(--n-sidebar)] px-[18px] py-3.5 ${i ? 'border-t border-[var(--n-line)]' : ''}`}>
          {r.state === 'done' && <CheckCircle size={17} color="var(--n-accent-400)" />}
          {r.state === 'busy' && <CircleNotch size={17} color="var(--n-accent-400)" className="n-spin" />}
          {r.state === 'idle' && <CircleDashed size={17} color="var(--n-mute)" />}
          {r.state === 'live' && <Broadcast size={17} color="var(--n-accent-400)" />}
          <span className="min-w-0 flex-1">
            <span className="block text-[15px] font-semibold leading-[1.3]"
                  style={{ color: r.state === 'idle' ? 'var(--n-text-4)' : r.state === 'done' ? 'var(--n-text-2)' : 'var(--n-text)' }}>
              {r.title}
            </span>
            <span className="mt-[3px] block text-[13.5px] leading-[1.3] text-[var(--n-text-3)]">{r.sub}</span>
          </span>
          <span className="font-mono text-[13.5px] font-medium leading-none text-[var(--n-text-2)]">{r.value}</span>
        </div>
      ))}
    </div>
  )
}
