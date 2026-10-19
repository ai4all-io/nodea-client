import { useEffect, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { invoke } from '@tauri-apps/api/tauri'
import { useAuth } from '@/contexts/AuthContext'
import { AccountSection } from '@/components/account/AccountSection'
import { isDemoMode, mockInvoke } from '@/lib/mockTauri'
import { isTauriApp } from '@/lib/platform'
import type { NodeState } from '@/hooks/useNodePhase'
import { version as appVersion } from '../../package.json'

interface ModelCacheInfo {
  path: string
  size_bytes: number
}

function formatGB(bytes: number): string {
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`
}

export function SettingsPage() {
  const node = useOutletContext<NodeState>()
  const { logout } = useAuth()
  const [cache, setCache] = useState<ModelCacheInfo | null>(null)

  useEffect(() => {
    const demo = isDemoMode()
    ;(demo ? mockInvoke : invoke)<ModelCacheInfo>('get_model_cache_info')
      .then(setCache)
      .catch(console.error)
  }, [])

  const revealCache = () => {
    if (!cache) return
    if (isDemoMode()) {
      mockInvoke('reveal_model_cache').catch(console.error)
    } else if (isTauriApp()) {
      invoke('reveal_model_cache').catch(console.error)
    }
  }

  return (
    <div className="p-[30px]">
      <h1 className="mb-6 text-[26px] font-semibold leading-[1.2] tracking-[-0.02em] text-[var(--n-text)]">
        Settings
      </h1>

      <div className="flex flex-col gap-0.5">
        <Row first title="Share compute" description="Whenever this Mac is awake and plugged in."
             control={<Switch on={node.sharing} onChange={node.setSharing} />} />
        <Row last title="Model cache"
             description={cache ? `${formatGB(cache.size_bytes)} in ${cache.path}` : 'Calculating…'}
             control={<GhostButton onClick={revealCache} disabled={!cache}>Reveal</GhostButton>} />
      </div>

      <AccountSection />

      <div className="mt-[26px] flex items-center justify-between border-t border-[var(--n-line)] pt-[18px]">
        <span className="text-[13.5px] leading-[1.5] text-[var(--n-text-4)]">Nodea {appVersion}</span>
        <GhostButton onClick={logout}>Sign out</GhostButton>
      </div>
    </div>
  )
}

function Row({
  title, description, control, first, last,
}: {
  title: string; description: string; control: React.ReactNode; first?: boolean; last?: boolean
}) {
  const radius = first ? '10px 10px 2px 2px' : last ? '2px 2px 10px 10px' : '2px'
  return (
    <div className="flex items-center justify-between gap-5 border border-[var(--n-line)] bg-[var(--n-sidebar)] px-[18px] py-4"
         style={{ borderRadius: radius }}>
      <span>
        <span className="block text-[15px] font-semibold leading-[1.3] text-[var(--n-text)]">{title}</span>
        <span className="mt-1 block text-[14px] leading-[1.5] text-[var(--n-text-3)]">{description}</span>
      </span>
      {control}
    </div>
  )
}

function Switch({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button role="switch" aria-checked={on} onClick={() => onChange(!on)}
            className="relative inline-block h-5 w-9 flex-none rounded-[10px] transition-colors duration-[180ms]"
            style={{ background: on ? 'var(--n-accent-700)' : 'var(--n-border)' }}>
      <span className="absolute top-0.5 h-4 w-4 rounded-full transition-[left,background] duration-[180ms]"
            style={{ left: on ? 18 : 2, background: on ? 'var(--n-accent-200)' : 'var(--n-text-4)' }} />
    </button>
  )
}

function GhostButton({ children, onClick, disabled }: { children: React.ReactNode; onClick?: () => void; disabled?: boolean }) {
  return (
    <button onClick={onClick} disabled={disabled}
      className="flex-none rounded-[8px] border border-[var(--n-mute-4)] px-[13px] py-[7px] text-[14px] font-semibold leading-none text-[var(--n-text-4)] transition-colors hover:border-[var(--n-mute-2)] hover:text-[var(--n-text-2)]">
      {children}
    </button>
  )
}
