import { Navigate } from 'react-router-dom'
import { useAuth } from '@/contexts/AuthContext'

export function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, isLoading, organization, organizationLoadError, refreshOrganization, wasAccountDeleted } = useAuth()

  if (isLoading) {
    return (
      <div className="flex h-screen items-center justify-center">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-primary mx-auto"></div>
          <p className="mt-4 text-muted-foreground">Loading...</p>
        </div>
      </div>
    )
  }

  if (!isAuthenticated) {
    return <Navigate to={wasAccountDeleted ? '/login?account=deleted' : '/login'} replace />
  }

  // Check if there was an error loading organization
  if (organizationLoadError && window.location.pathname !== '/onboarding') {
    return (
      <div className="flex h-screen items-center justify-center">
        <div className="text-center max-w-md">
          <div className="text-destructive mb-4">
            <svg className="h-12 w-12 mx-auto" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
            </svg>
          </div>
          <h2 className="text-xl font-bold mb-2">Failed to Load Organization</h2>
          <p className="text-muted-foreground mb-4">{organizationLoadError}</p>
          <button
            onClick={() => refreshOrganization()}
            className="px-4 py-2 bg-primary text-primary-foreground rounded hover:bg-primary/90"
          >
            Retry
          </button>
        </div>
      </div>
    )
  }

  // Check if user needs onboarding
  // Redirect to onboarding if:
  // 1. User just signed up (even if they somehow have an org), OR
  // 2. User doesn't have an organization (incomplete onboarding)
  // But NOT if there's an organization load error (that's handled above)
  const justSignedUp = sessionStorage.getItem('just_signed_up') === 'true'
  const pendingInvitation = sessionStorage.getItem('pending_invitation_token')

  // If user has pending invitation, redirect there instead of onboarding
  if (pendingInvitation && window.location.pathname !== `/invitation/${pendingInvitation}`) {
    return <Navigate to={`/invitation/${pendingInvitation}`} replace />
  }

  // Normal onboarding redirect (skip if user has pending invitation)
  if ((justSignedUp || !organization) && !organizationLoadError &&
      window.location.pathname !== '/onboarding' && !pendingInvitation) {
    return <Navigate to="/onboarding" replace />
  }

  return <>{children}</>
}
