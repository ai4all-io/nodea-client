import { useMemo, useState } from 'react'
import type { TokenStatsResponse } from '@/lib/api'

export type ChartRange = '1h' | '1d' | '1w'

const AXES: Record<ChartRange, string[]> = {
  '1h': ['−60m', '−45m', '−30m', '−15m', 'now'],
  '1d': ['00:00', '06:00', '12:00', '18:00', 'now'],
  '1w': ['Mon', 'Wed', 'Fri', 'Sun', 'now'],
}

const WINDOW_MS: Record<ChartRange, number> = {
  '1h': 60 * 60 * 1000,
  '1d': 24 * 60 * 60 * 1000,
  '1w': 7 * 24 * 60 * 60 * 1000,
}

const BARS = 18

/** value 0-100 → ramp step */
function barColor(pct: number) {
  if (pct > 88) return 'var(--n-bar-6)'
  if (pct > 70) return 'var(--n-bar-5)'
  if (pct > 55) return 'var(--n-bar-4)'
  if (pct > 40) return 'var(--n-bar-3)'
  if (pct > 25) return 'var(--n-bar-2)'
  return 'var(--n-bar-1)'
}

/** Buckets raw token stats into 18 bars for the selected window. */
function bucket(stats: TokenStatsResponse[], range: ChartRange): number[] {
  const now = Date.now()
  const from = now - WINDOW_MS[range]
  const width = WINDOW_MS[range] / BARS
  const sums = new Array(BARS).fill(0)

  for (const s of stats) {
    const t = new Date(s.date).getTime()
    if (t < from || t > now) continue
    const i = Math.min(BARS - 1, Math.floor((t - from) / width))
    sums[i] += s.value
  }

  const max = Math.max(...sums, 1)
  return sums.map((v) => Math.round((v / max) * 100))
}

export function ThroughputChart({ stats }: { stats: TokenStatsResponse[] }) {
  const [range, setRange] = useState<ChartRange>('1d')
  const bars = useMemo(() => bucket(stats, range), [stats, range])

  return (
    <div className="mt-[26px]">
      <div className="flex items-center justify-between gap-3">
        <span className="text-[14px] font-semibold leading-none text-[var(--n-text-2)]">
          Throughput <span className="font-normal text-[var(--n-text-3)]">· tokens / min</span>
        </span>
        <span className="flex gap-0.5 rounded-[7px] bg-[var(--n-sidebar)] p-0.5">
          {(['1h', '1d', '1w'] as ChartRange[]).map((r) => (
            <button key={r} onClick={() => setRange(r)}
              className="rounded-[5px] px-2.5 py-1 text-[13px] font-semibold leading-none transition-colors"
              style={
                range === r
                  ? { background: 'var(--n-tint-selected)', color: 'var(--n-accent-200)' }
                  : { background: 'transparent', color: 'var(--n-text-4)' }
              }>
              {r}
            </button>
          ))}
        </span>
      </div>

      <div className="mt-3.5 flex h-24 items-end gap-[3px]">
        {bars.map((pct, i) => (
          <div key={i}
               className="flex-1 rounded-t-[2px] transition-[height] duration-150 ease-out"
               style={{ height: `${Math.max(pct, 2)}%`, background: barColor(pct) }} />
        ))}
      </div>

      <div className="mt-2 flex justify-between text-[12px] leading-none text-[var(--n-text-3)]">
        {AXES[range].map((l) => <span key={l}>{l}</span>)}
      </div>
    </div>
  )
}
