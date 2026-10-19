import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/api/shell'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useAuth } from '@/contexts/AuthContext'
import { apiClient, type Me } from '@/lib/api'
import { isTauriApp } from '@/lib/platform'
import { WEB_CONSOLE_URL } from '@/lib/webConsole'

const ACCOUNT_URL = `${WEB_CONSOLE_URL}account`

// The same comparison the backend makes, so the button only enables for a
// confirmation the server will accept.
function confirms(typed: string, expected: string): boolean {
  return typed.trim().toLocaleLowerCase() === expected.trim().toLocaleLowerCase()
}

function openAccountInBrowser() {
  if (isTauriApp()) open(ACCOUNT_URL).catch(console.error)
  else window.open(ACCOUNT_URL, '_blank')
}

/**
 * Settings → Account. Only leave and delete live here; everything else about
 * the account is managed in the web console (spec D-8).
 */
export function AccountSection() {
  const { organizationLost, accountDeleted } = useAuth()
  const [me, setMe] = useState<Me | null>(null)
  const [dialog, setDialog] = useState<'leave' | 'delete' | null>(null)

  useEffect(() => {
    apiClient.getMe().then(setMe).catch(console.error)
  }, [])

  if (!me) return null

  const blocked = me.organization !== null && !me.can_leave
  const leaveText = me.leave_deletes_organization
    ? 'You are the last member, so leaving deletes the organization, its API keys and its devices.'
    : 'This Mac stops sharing compute with it.' +
      (me.leave_disables_api_keys > 0
        ? ` ${me.leave_disables_api_keys === 1 ? 'The API key' : `The ${me.leave_disables_api_keys} API keys`} you created stop working.`
        : '')

  return (
    <section aria-labelledby="account-heading" className="mt-[26px]">
      <h2 id="account-heading" className="mb-2.5 text-[15px] font-semibold leading-none text-[var(--n-text)]">
        Account
      </h2>
      <div className="flex flex-col gap-0.5">
        <Row first title={me.email ?? 'Signed in'}
             description={me.organization ? `Member of ${me.organization.name}` : 'Not in an organization'}
             control={<GhostButton onClick={openAccountInBrowser}>Manage account ↗</GhostButton>} />
        {me.organization && (
          <Row title="Leave organization"
               description={blocked ? "You're the only admin. Promote another member in the web console first." : leaveText}
               control={<GhostButton onClick={() => setDialog('leave')} disabled={blocked}>Leave</GhostButton>} />
        )}
        <Row last title="Delete account"
             description={blocked
               ? "You're the only admin. Promote another member in the web console first."
               : 'Erases your name and email and removes your devices. This cannot be undone.'}
             control={<GhostButton onClick={() => setDialog('delete')} disabled={blocked || !me.email}>Delete</GhostButton>} />
      </div>

      {me.organization && (
        <ConfirmDialog
          open={dialog === 'leave'}
          onOpenChange={(o) => setDialog(o ? 'leave' : null)}
          title={`${me.leave_deletes_organization ? 'Leave and delete' : 'Leave'} ${me.organization.name}?`}
          description={leaveText}
          expected={me.organization.name}
          inputLabel={`Type ${me.organization.name} to confirm`}
          action="Leave organization"
          run={async (typed) => {
            await apiClient.leaveOrganization(typed)
            await organizationLost(me.organization!.name)
          }}
        />
      )}
      {me.email && (
        <ConfirmDialog
          open={dialog === 'delete'}
          onOpenChange={(o) => setDialog(o ? 'delete' : null)}
          title="Delete your account?"
          description={
            'You are signed out everywhere and cannot sign in to this account again. Your devices are removed. ' +
            (me.organization
              ? me.leave_deletes_organization
                ? `You are the last member of ${me.organization.name}, so it is deleted too. `
                : `You leave ${me.organization.name}, and API keys you created there stop working. `
              : '') +
            'Downloaded models stay on this Mac.'
          }
          expected={me.email}
          inputLabel={`Type ${me.email} to confirm`}
          action="Delete account"
          run={async (typed) => {
            await apiClient.deleteAccount(typed)
            await accountDeleted()
          }}
        />
      )}
    </section>
  )
}

function ConfirmDialog({
  open, onOpenChange, title, description, expected, inputLabel, action, run,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description: string
  expected: string
  inputLabel: string
  action: string
  run: (typed: string) => Promise<void>
}) {
  const [typed, setTyped] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleOpenChange = (next: boolean) => {
    if (busy) return
    if (!next) {
      setTyped('')
      setError(null)
    }
    onOpenChange(next)
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await run(typed)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Something went wrong')
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <form onSubmit={submit} className="grid gap-4">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>{description}</DialogDescription>
          </DialogHeader>
          <label className="grid gap-2 text-[14px] text-[var(--n-text-2)]">
            {inputLabel}
            <Input value={typed} autoFocus autoComplete="off" disabled={busy} onChange={(e) => setTyped(e.target.value)} />
          </label>
          {error && <p role="alert" className="text-[13.5px] text-[var(--n-text-2)]">{error}</p>}
          <DialogFooter>
            <GhostButton onClick={() => handleOpenChange(false)}>Cancel</GhostButton>
            <button type="submit" disabled={busy || !confirms(typed, expected)}
                    className="rounded-[8px] bg-[var(--n-danger,#b42318)] px-[13px] py-[7px] text-[14px] font-semibold leading-none text-white disabled:opacity-40">
              {busy ? 'Working…' : action}
            </button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
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

function GhostButton({ children, onClick, disabled }: { children: React.ReactNode; onClick?: () => void; disabled?: boolean }) {
  return (
    <button type="button" onClick={onClick} disabled={disabled}
      className="flex-none rounded-[8px] border border-[var(--n-mute-4)] px-[13px] py-[7px] text-[14px] font-semibold leading-none text-[var(--n-text-4)] transition-colors hover:border-[var(--n-mute-2)] hover:text-[var(--n-text-2)] disabled:opacity-40">
      {children}
    </button>
  )
}
