import { Outlet, useLocation } from 'react-router-dom'
import { Sidebar } from './Sidebar'
import { useNodePhase } from '@/hooks/useNodePhase'
import { DeviceLimitBlockedScreen } from '@/components/DeviceLimitBlockedScreen'

const TITLES: Record<string, string> = {
  '/node': 'Node',
  '/fleet': 'Fleet',
  '/settings': 'Settings',
  '/playground': 'Playground (dev)',
  '/dev-logs': 'Dev Logs (dev)',
}

export function AppLayout() {
  const node = useNodePhase()
  const { pathname } = useLocation()

  if (node.phase === 'stalled' && node.message && /device limit/i.test(node.message)) {
    return <DeviceLimitBlockedScreen message={node.message} />
  }

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--n-bg)]">
      <Sidebar node={node} />
      <main className="flex min-w-0 flex-1 flex-col">
        {/* Titlebar strip — screen name only, and the window drag region */}
        <div className="n-drag flex h-[46px] flex-none items-center border-b border-[var(--n-line)] px-6"
             data-tauri-drag-region>
          <span className="text-[14px] font-semibold leading-none text-[var(--n-text-4)]">
            {TITLES[pathname] ?? ''}
          </span>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          <Outlet context={node} />
        </div>
      </main>
    </div>
  )
}
