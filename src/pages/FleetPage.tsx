import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { apiClient, type DeviceResponse } from '@/lib/api'
import { isDemoMode, mockInvoke } from '@/lib/mockTauri'
import { WEB_CONSOLE_URL } from '@/lib/webConsole'

/* Open to every member of the organization: the roster it shows is the
 * organization's, not the signed-in user's, so there is nothing here a
 * standard member should not see. */
export function FleetPage() {
  const [devices, setDevices] = useState<DeviceResponse[]>([])
  const [thisDeviceId, setThisDeviceId] = useState<string | null>(null)

  useEffect(() => {
    const es = apiClient.createFleetStream()
    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        if (data.type === 'fleet') setDevices(data.value)
      } catch (err) {
        console.error('fleet stream', err)
      }
    }
    return () => es.close()
  }, [])

  useEffect(() => {
    const demo = isDemoMode()
    ;(demo ? mockInvoke : invoke)<string>('get_device_id')
      .then(setThisDeviceId)
      .catch(console.error)
  }, [])

  const serving = devices.filter((d) => d.connected).length

  return (
    <div className="p-[30px]">
      <h1 className="mb-[22px] text-[26px] font-semibold leading-[1.2] tracking-[-0.02em] text-[var(--n-text)]">
        Fleet{' '}
        <span className="text-[17px] font-normal leading-none text-[var(--n-text-4)]">
          · {serving} of {devices.length} serving
        </span>
      </h1>

      <div className="overflow-hidden rounded-[12px] border border-[var(--n-line)]">
        <div className="grid grid-cols-[2fr_1.4fr_1fr] gap-3 bg-[var(--n-sidebar)] px-[18px] py-2.5 text-[12.5px] font-semibold uppercase leading-none tracking-[0.06em] text-[var(--n-text-3)]">
          <span>Device</span>
          <span>Model</span>
          <span>Status</span>
        </div>

        {devices.map((d) => {
          const isThis = d.id === thisDeviceId
          return (
            <div key={d.id}
                 className="grid grid-cols-[2fr_1.4fr_1fr] items-center gap-3 border-t border-[var(--n-line)] px-[18px] py-3.5"
                 style={{ background: isThis ? 'var(--n-tint-row)' : undefined }}>
              <span>
                <span className="block text-[15px] font-semibold leading-[1.3]"
                      style={{ color: d.connected ? 'var(--n-text)' : 'var(--n-text-2)' }}>
                  {d.name}
                  {isThis && (
                    <span className="ml-1.5 rounded-[4px] border border-[var(--n-accent-800)] px-[5px] py-0.5 text-[12px] font-medium leading-none text-[var(--n-accent)]">
                      this Mac
                    </span>
                  )}
                </span>
                <span className="mt-[3px] block text-[13px] leading-[1.3] text-[var(--n-text-3)]">{d.type}</span>
              </span>
              <span className="text-[14px] leading-[1.3]"
                    style={{ color: d.connected ? 'var(--n-text-2)' : 'var(--n-text-4)' }}>
                {d.model || '—'}
              </span>
              <span className="flex items-center gap-[7px] text-[14px] font-medium leading-none"
                    style={{ color: d.connected ? 'var(--n-accent-300)' : 'var(--n-text-4)' }}>
                <span className="h-1.5 w-1.5 rounded-full"
                      style={{ background: d.connected ? 'var(--n-accent-400)' : 'var(--n-mute-4)' }} />
                {d.connected ? 'Serving' : 'Offline'}
              </span>
            </div>
          )
        })}
      </div>

      <div className="mt-5 flex items-center justify-between rounded-[12px] border border-[var(--n-line)] bg-[var(--n-sidebar)] px-[18px] py-3.5">
        <span className="text-[14.5px] leading-[1.5] text-[var(--n-text-4)]">
          Keys, usage history and team live on the web.
        </span>
        <a href={WEB_CONSOLE_URL} target="_blank" rel="noreferrer"
           className="text-[14px] font-semibold leading-none text-[var(--n-accent-400)]">
          Open console ↗
        </a>
      </div>
    </div>
  )
}
