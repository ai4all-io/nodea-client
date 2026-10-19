import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { AlertCircle, CheckCircle, Mail } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { useAuth } from '@/contexts/AuthContext'
import { apiClient, InvitationCheckResponse } from '@/lib/api'

type InvitationState = 'loading' | 'valid' | 'invalid' | 'accepted' | 'error'

export function AcceptInvitationPage() {
  const { token } = useParams<{ token: string }>()
  const { isAuthenticated, refreshOrganization } = useAuth()
  const navigate = useNavigate()

  const [state, setState] = useState<InvitationState>('loading')
  const [invitationData, setInvitationData] = useState<InvitationCheckResponse | null>(null)
  const [accepting, setAccepting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (token) {
      checkInvitation()
    } else {
      setState('invalid')
      setError('Invalid invitation link')
    }
  }, [token])

  const checkInvitation = async () => {
    if (!token) return

    try {
      setState('loading')
      setError(null)
      const data = await apiClient.checkInvitation(token)
      setInvitationData(data)

      if (data.valid) {
        setState('valid')
      } else {
        setState('invalid')
        setError(data.error || 'This invitation is invalid or has expired')
      }
    } catch (err) {
      setState('error')
      setError(err instanceof Error ? err.message : 'Failed to check invitation')
    }
  }

  const handleAccept = async () => {
    if (!token) return

    if (!isAuthenticated) {
      // Store invitation token for signup flow
      sessionStorage.setItem('pending_invitation_token', token)

      // Redirect to login with return URL
      navigate(`/login?redirect=/invitation/${token}`)
      return
    }

    try {
      setAccepting(true)
      setError(null)
      await apiClient.acceptInvitation(token)
      await refreshOrganization()
      setState('accepted')

      // Clear invitation token and signup flags
      sessionStorage.removeItem('pending_invitation_token')
      sessionStorage.removeItem('just_signed_up')

      // Redirect to the node page after a short delay
      setTimeout(() => {
        navigate('/node')
      }, 2000)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to accept invitation')
      setState('error')
    } finally {
      setAccepting(false)
    }
  }

  const renderContent = () => {
    switch (state) {
      case 'loading':
        return (
          <div className="text-center py-12">
            <div className="text-muted-foreground">Checking invitation...</div>
          </div>
        )

      case 'valid':
        return (
          <div className="space-y-6">
            <div className="text-center space-y-2">
              <Mail className="h-12 w-12 mx-auto text-primary" />
              <h2 className="text-2xl font-bold">You've been invited!</h2>
              <p className="text-muted-foreground">
                {invitationData?.organization_name && (
                  <>Join <span className="font-semibold">{invitationData.organization_name}</span> on Nodea</>
                )}
              </p>
            </div>

            {invitationData && (
              <div className="space-y-4">
                <div className="flex items-center justify-between p-4 border rounded-lg">
                  <div className="space-y-1">
                    <div className="text-sm font-medium">Email</div>
                    <div className="text-sm text-muted-foreground">{invitationData.email}</div>
                  </div>
                  <div className="space-y-1 text-right">
                    <div className="text-sm font-medium">Role</div>
                    <Badge variant={invitationData.role === 'admin' ? 'default' : 'secondary'}>
                      {invitationData.role}
                    </Badge>
                  </div>
                </div>

                {!isAuthenticated && (
                  <Alert>
                    <AlertCircle className="h-4 w-4" />
                    <AlertTitle>Login Required</AlertTitle>
                    <AlertDescription>
                      You need to be logged in to accept this invitation. You'll be redirected to the login page.
                    </AlertDescription>
                  </Alert>
                )}

                {error && (
                  <Alert variant="destructive">
                    <AlertCircle className="h-4 w-4" />
                    <AlertDescription>{error}</AlertDescription>
                  </Alert>
                )}

                <div className="flex flex-col gap-2">
                  <Button
                    onClick={handleAccept}
                    disabled={accepting}
                    size="lg"
                    className="w-full"
                  >
                    {accepting ? 'Accepting...' : isAuthenticated ? 'Accept Invitation' : 'Login to Accept'}
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => navigate('/')}
                    size="lg"
                    className="w-full"
                  >
                    Decline
                  </Button>
                </div>
              </div>
            )}
          </div>
        )

      case 'accepted':
        return (
          <div className="text-center space-y-4 py-12">
            <CheckCircle className="h-16 w-16 mx-auto text-green-500" />
            <div className="space-y-2">
              <h2 className="text-2xl font-bold">Welcome aboard!</h2>
              <p className="text-muted-foreground">
                You've successfully joined {invitationData?.organization_name}
              </p>
              <p className="text-sm text-muted-foreground">
                Redirecting...
              </p>
            </div>
          </div>
        )

      case 'invalid':
      case 'error':
        return (
          <div className="space-y-6">
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertTitle>Invalid Invitation</AlertTitle>
              <AlertDescription>
                {error || 'This invitation link is invalid or has expired.'}
              </AlertDescription>
            </Alert>

            <div className="text-center space-y-4">
              <p className="text-sm text-muted-foreground">
                The invitation link may have expired or been revoked. Please contact your organization administrator for a new invitation.
              </p>
              <Button onClick={() => navigate('/')} variant="outline">
                Go to Home
              </Button>
            </div>
          </div>
        )

      default:
        return null
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle className="text-2xl text-center">Nodea</CardTitle>
          <CardDescription className="text-center">
            Organization Invitation
          </CardDescription>
        </CardHeader>
        <CardContent>
          {renderContent()}
        </CardContent>
      </Card>
    </div>
  )
}
