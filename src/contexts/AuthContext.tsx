import { createContext, useContext, useState, useEffect, useCallback, useRef, ReactNode } from 'react'
import { invoke } from '@tauri-apps/api/tauri'
import { createClient } from '@supabase/supabase-js'
import { apiClient, Organization } from '../lib/api'
import { isTauriApp } from '../lib/platform'
import { mockInvoke } from '../lib/mockTauri'

// Initialize Supabase client for web mode
const supabaseUrl = import.meta.env.VITE_SUPABASE_URL || ''
const supabaseAnonKey = import.meta.env.VITE_SUPABASE_ANON_KEY || ''
const supabase = createClient(supabaseUrl, supabaseAnonKey)

interface User {
  uuid: string
  email: string
  name?: string
  picture?: string
  role: 'admin' | 'standard'
  organization_id: number
}

interface AuthContextType {
  user: User | null
  organization: Organization | null
  organizationLoadError: string | null
  isAuthenticated: boolean
  isAdmin: boolean
  isLoading: boolean
  login: (email: string, password: string) => Promise<void>
  signup: (email: string, password: string) => Promise<void>
  signupWithInvitation: (token: string, email: string, password: string) => Promise<void>
  loginWithGoogle: () => Promise<void>
  logout: () => Promise<void>
  refreshOrganization: () => Promise<void>
  /** An admin deactivated this account: it may not share, but may leave. */
  accountInactive: boolean
  /** Name of the organization the user just stopped being part of, for the onboarding notice. */
  leftOrganizationName: string | null
  /** The user left (here or elsewhere): stop sharing and drop the organization. */
  organizationLost: (name: string) => Promise<void>
  /** The account was deleted: stop sharing and sign out. */
  accountDeleted: () => Promise<void>
  /** Set once the account was deleted, so the login page can say so. */
  wasAccountDeleted: boolean
}

// How often to re-check the account while the app is open. Nothing else the
// desktop does fails after a leave elsewhere (the device stream is accepted
// without an organization), so the app has to ask (#241).
const MEMBERSHIP_CHECK_MS = 60_000

const AuthContext = createContext<AuthContextType | undefined>(undefined)

// Check if demo mode is enabled
const isDemoMode = () => {
  const urlParams = new URLSearchParams(window.location.search)
  const demoParam = urlParams.get('demo')
  const storedDemo = localStorage.getItem('demoMode')

  if (demoParam !== null) {
    const enabled = demoParam === 'true' || demoParam === '1'
    localStorage.setItem('demoMode', enabled.toString())
    return enabled
  }

  return storedDemo === 'true'
}

// Role of the demo-mode user, overridable with ?role=standard (see
// mockTauri's ?phase=). Demo mode is admin by default; the override exists so
// tests can walk the app as an ordinary member.
const demoRole = (): 'admin' | 'standard' =>
  new URLSearchParams(window.location.search).get('role') === 'standard' ? 'standard' : 'admin'

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [organization, setOrganization] = useState<Organization | null>(null)
  const [organizationLoadError, setOrganizationLoadError] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [accountInactive, setAccountInactive] = useState(false)
  const [leftOrganizationName, setLeftOrganizationName] = useState<string | null>(null)
  const [wasAccountDeleted, setWasAccountDeleted] = useState(false)
  const organizationRef = useRef<Organization | null>(null)
  const accountDeletedHandled = useRef(false)
  const demoMode = isDemoMode()

  useEffect(() => {
    organizationRef.current = organization
  }, [organization])

  // The device must not share for an organization the user is no longer
  // part of, nor for a deactivated account. Sharing stays on as a preference
  // and resumes when the user joins or creates an organization.
  const stopSharing = useCallback(async () => {
    if (isTauriApp()) await invoke('disconnect_grpc').catch(console.error)
  }, [])

  useEffect(() => {
    checkAuth()

    // Native sign-in resolves inside the Rust command, and demo mode has no
    // Supabase at all - only web mode comes back from Google as a redirect
    // whose code the SDK exchanges after checkAuth has already run.
    if (isTauriApp() || demoMode) {
      return
    }

    const { data } = supabase.auth.onAuthStateChange((event, session) => {
      if (event !== 'SIGNED_IN' || !session?.access_token) {
        return
      }

      apiClient.setToken(session.access_token)
      loadUserFromBackend(session.access_token).catch((error) => {
        console.error('[Auth] Failed to load user after OAuth redirect:', error)
      })
    })

    return () => data.subscription.unsubscribe()
  }, [])

  const loadOrganization = useCallback(async () => {
    // Always attempt the fetch and let a 404 mean "no organization yet" -
    // user.organization_id can't gate this: the backend's /v1/organization
    // response has no numeric id and /v1/users member objects don't carry
    // organization_id, so it's derived as 0 whenever that fallback chain
    // runs (see loadUserFromBackend), which isn't a reliable signal here.
    try {
      console.log('[Auth] Loading organization...')
      setOrganizationLoadError(null)

      const org = await apiClient.getOrganization()
      console.log('[Auth] Organization loaded successfully:', org)
      setOrganization(org)
    } catch (error: any) {
      console.error('[Auth] Failed to load organization:', error)

      if (error.status === 404) {
        // User has no organization yet - not an error, needs onboarding
        setOrganization(null)
        setOrganizationLoadError(null)
      } else {
        // Actual error (network, server, etc.)
        setOrganizationLoadError(error.message || 'Failed to load organization')
      }
    }
  }, [])

  // Load organization when user logs in
  useEffect(() => {
    // Skip while checkAuth() is still resolving: this effect's closure sees
    // the pre-checkAuth `user` (null) and would otherwise queue
    // setOrganization(null) after (and clobber) the organization checkAuth
    // itself just set - e.g. demo mode's synchronous mock org, which briefly
    // bounced every demo-mode page to /onboarding before this guard.
    if (isLoading) return

    if (user) {
      console.log('[Auth] User changed, loading organization for user:', user.email)
      loadOrganization()
    } else {
      console.log('[Auth] No user, clearing organization')
      setOrganization(null)
    }
  }, [user?.uuid, isLoading, loadOrganization])

  const refreshOrganization = async () => {
    await loadOrganization()
  }

  const organizationLost = async (name: string) => {
    await stopSharing()
    setLeftOrganizationName(name)
    setOrganization(null)
  }

  const accountDeleted = async () => {
    if (accountDeletedHandled.current) return
    accountDeletedHandled.current = true
    await stopSharing()
    setWasAccountDeleted(true)
    await logout()
  }

  // Any request refused with account_deleted ends the session (a token
  // issued before the deletion stays valid until it expires).
  useEffect(() => {
    apiClient.setAccountDeletedHandler(() => {
      void accountDeleted()
    })
    return () => apiClient.setAccountDeletedHandler(null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Membership changes made elsewhere: leaving or being removed in the web
  // console, an admin deactivating the account, deleting it. Checked on load,
  // whenever the window regains focus, and every minute.
  useEffect(() => {
    if (isLoading || !user) return
    let stopped = false

    const check = async () => {
      try {
        const me = await apiClient.getMe()
        if (stopped) return

        setAccountInactive(!me.is_active)
        if (!me.is_active) await stopSharing()

        const current = organizationRef.current
        if (current && !me.organization) {
          await organizationLost(current.name)
        } else if (!current && me.organization) {
          await loadOrganization()
        }
      } catch (error) {
        // account_deleted is handled by the client's handler; anything else
        // (offline, a deploy) is retried on the next check.
        console.warn('[Auth] Membership check failed:', error)
      }
    }

    void check()
    const interval = setInterval(() => void check(), MEMBERSHIP_CHECK_MS)
    const onFocus = () => void check()
    window.addEventListener('focus', onFocus)
    return () => {
      stopped = true
      clearInterval(interval)
      window.removeEventListener('focus', onFocus)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [user?.uuid, isLoading])

  const checkAuth = async () => {
    console.log('[checkAuth] Demo mode:', demoMode)
    if (demoMode) {
      console.log('[checkAuth] Setting up demo mode user and organization')
      // Auto-login for demo mode with mock organization data
      setUser({
        uuid: 'demo-user-uuid',
        email: 'demo@getnodea.com',
        name: 'Demo User',
        role: demoRole(),
        organization_id: 1,
      })
      setOrganization({
        id: 1,
        name: 'Demo Organization',
        slug: 'demo-org',
        member_count: 5,
        created_at: new Date().toISOString(),
      })
      setIsLoading(false)
      console.log('[checkAuth] Demo mode setup complete')
      return
    }

    try {
      if (isTauriApp()) {
        // Tauri mode: use invoke command
        const token = await invoke<string | null>('auto_login')
        if (token) {
          apiClient.setToken(token)
          await loadUserFromBackend(token)
        }
      } else {
        // Web mode: check Supabase session
        const { data: { session } } = await supabase.auth.getSession()
        if (session?.access_token) {
          apiClient.setToken(session.access_token)
          await loadUserFromBackend(session.access_token)
        }
      }
    } catch (error) {
      console.error('Auto-login failed:', error)
    } finally {
      setIsLoading(false)
    }
  }

  const loadUserFromBackend = async (token: string) => {
    try {
      console.log('[Auth] Loading user from backend...')
      setOrganizationLoadError(null)

      // Decode JWT first to get user info
      const payload = JSON.parse(atob(token.split('.')[1]))

      // Try to load organization to get user details
      let org: Organization | null = null
      let currentUser
      const isOnboardingPage = window.location.pathname === '/onboarding'
      const isInvitationPage = window.location.pathname.startsWith('/invitation/')

      if (!isOnboardingPage && !isInvitationPage) {
        try {
          org = await apiClient.getOrganization()
          console.log('[Auth] Organization loaded successfully:', org)

          // If org exists, get members to find current user
          const membersResponse = await apiClient.getOrganizationMembers()
          console.log('[Auth] Members loaded:', membersResponse)
          currentUser = membersResponse.users.find(
            m => m.uuid === payload.sub || m.email === payload.email
          )
        } catch (error: any) {
          if (error.status === 404) {
            console.log('[Auth] No organization found (404) - user needs onboarding')
            // This is expected for new users - not an error
          } else {
            console.error('[Auth] Failed to load organization:', error)
            setOrganizationLoadError(error.message || 'Failed to load organization')
          }
        }
      }

      // Organization membership doesn't carry organization_id (it's implicit
      // in which org's member list it came from) - the org's own id is the
      // source of truth here.
      const orgId = org?.id || 0

      setUser({
        uuid: currentUser?.uuid || payload.sub,
        email: currentUser?.email || payload.email,
        name: currentUser?.name || undefined,
        role: currentUser?.role || 'standard',
        organization_id: orgId,  // 0 if no org
      })

      setOrganization(org)
      console.log('[Auth] User and org state set successfully')
    } catch (err) {
      console.error('[Auth] Failed to load user/org from backend:', err)
      // Re-throw the error so login knows it failed
      throw err
    }
  }

  const login = async (email: string, password: string) => {
    if (demoMode) {
      // Auto-succeed in demo mode
      setUser({
        uuid: 'demo-user-uuid',
        email,
        name: 'Demo User',
        role: 'admin',
        organization_id: 1,
      })
      return Promise.resolve()
    }

    try {
      let token: string

      if (isTauriApp()) {
        // Tauri mode: use invoke command
        token = await invoke<string>('login_email_password', { email, password })
      } else {
        // Web mode: use Supabase client
        const { data, error } = await supabase.auth.signInWithPassword({
          email,
          password,
        })

        if (error) throw new Error(error.message)
        if (!data.session) throw new Error('No session returned')

        token = data.session.access_token
      }

      // Set token in API client
      apiClient.setToken(token)

      // Load user and organization data from backend
      await loadUserFromBackend(token)

      return Promise.resolve()
    } catch (error) {
      throw new Error(error as string)
    }
  }

  const signup = async (email: string, password: string) => {
    if (demoMode) {
      // Auto-succeed in demo mode
      setUser({
        uuid: 'demo-user-uuid',
        email,
        name: 'Demo User',
        role: 'admin',
        organization_id: 1,
      })
      return Promise.resolve()
    }

    try {
      // The backend creates an unconfirmed Supabase account and emails our
      // own verification link - there is no session to store here. Login
      // stays gated until that link is clicked (see LoginPage's handling
      // of Supabase's "Email not confirmed" error).
      if (isTauriApp()) {
        await invoke<string>('sign_up', { email, password })
      } else {
        await apiClient.signup(email, password)
      }

      return Promise.resolve()
    } catch (error) {
      throw new Error(error as string)
    }
  }

  const signupWithInvitation = async (token: string, email: string, password: string) => {
    if (demoMode) {
      // Auto-succeed in demo mode
      setUser({
        uuid: 'demo-user-uuid',
        email,
        name: 'Demo User',
        role: 'admin',
        organization_id: 1,
      })
      return Promise.resolve()
    }

    try {
      // Backend creates the (pre-confirmed) Supabase account for the
      // invitation's own email and links the organization, so this never
      // depends on the project's global email-confirmation setting.
      await apiClient.signupWithInvitation(token, password)

      // Now sign in with the credentials just created, reusing the normal
      // login path (already handles the Tauri vs. web split).
      await login(email, password)

      return Promise.resolve()
    } catch (error) {
      throw new Error(error as string)
    }
  }

  const loginWithGoogle = async () => {
    if (demoMode) {
      // Auto-succeed in demo mode
      setUser({
        uuid: 'demo-user-uuid',
        email: 'demo@getnodea.com',
        name: 'Demo User',
        picture: 'https://ui-avatars.com/api/?name=Demo+User&background=0D8ABC&color=fff',
        role: 'admin',
        organization_id: 1,
      })
      return Promise.resolve()
    }

    try {
      if (isTauriApp()) {
        // Tauri mode: use invoke command
        const token = await invoke<string>('login_oauth', { provider: 'google' })
        apiClient.setToken(token)
        await loadUserFromBackend(token)
      } else {
        // Web mode: use Supabase OAuth
        const { error } = await supabase.auth.signInWithOAuth({
          provider: 'google',
          options: {
            redirectTo: window.location.origin,
          },
        })

        if (error) throw new Error(error.message)

        // OAuth redirect will happen, and checkAuth will be called on return
      }

      return Promise.resolve()
    } catch (error) {
      throw new Error(error as string)
    }
  }

  const logout = async () => {
    if (demoMode) {
      // The mock stands in for the Rust command, which also stops sharing.
      await mockInvoke('logout').catch(console.error)
      setUser(null)
      setOrganization(null)
      return
    }

    try {
      if (isTauriApp()) {
        // Also stops sharing (see `logout` in src-tauri/src/main.rs), so a
        // signed-out Mac stops serving the previous account's organization.
        await invoke('logout')
      } else {
        // Web mode: use Supabase client
        await supabase.auth.signOut()
        // Nothing writes this any more; clears a copy older builds left behind.
        localStorage.removeItem('access_token')
      }

      setUser(null)
      setOrganization(null)
    } catch (error) {
      console.error('Logout failed:', error)
    }
  }

  return (
    <AuthContext.Provider
      value={{
        user,
        organization,
        organizationLoadError,
        isAuthenticated: !!user,
        isAdmin: user?.role === 'admin',
        isLoading,
        login,
        signup,
        signupWithInvitation,
        loginWithGoogle,
        logout,
        refreshOrganization,
        accountInactive,
        leftOrganizationName,
        organizationLost,
        accountDeleted,
        wasAccountDeleted,
      }}
    >
      {children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider')
  }
  return context
}
