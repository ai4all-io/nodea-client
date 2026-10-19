import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { listen } from '@tauri-apps/api/event'

/**
 * Bridges nodea:// deep links (opened from invitation/confirm-email
 * emails) into the React Router. The Rust side (src-tauri/src/main.rs)
 * registers the OS-level scheme handler and forwards incoming links here
 * via the "scheme-request-received" event.
 *
 * FRONTEND_BASE_URL is "nodea://" in production, so a link built as
 * `${FRONTEND_BASE_URL}/confirm-email/${token}` becomes
 * "nodea:///confirm-email/<token>" - an empty host with the app route as
 * the URL's pathname, which is what we navigate to below.
 */
export function DeepLinkListener() {
  const navigate = useNavigate()

  useEffect(() => {
    const unlistenPromise = listen<string>('scheme-request-received', (event) => {
      try {
        const path = new URL(event.payload).pathname
        navigate(path)
      } catch (err) {
        console.error('Failed to parse deep link URL:', event.payload, err)
      }
    })

    return () => {
      unlistenPromise.then((unlisten) => unlisten())
    }
  }, [navigate])

  return null
}
