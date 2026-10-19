import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { AlertCircle, CheckCircle } from 'lucide-react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { apiClient } from '@/lib/api'

type ConfirmState = 'loading' | 'confirmed' | 'expired' | 'error'

export function ConfirmEmailPage() {
  const { token } = useParams<{ token: string }>()
  const navigate = useNavigate()

  const [state, setState] = useState<ConfirmState>('loading')
  const [error, setError] = useState<string | null>(null)
  const [email, setEmail] = useState('')
  const [resending, setResending] = useState(false)
  const [resent, setResent] = useState(false)

  useEffect(() => {
    const storedEmail = sessionStorage.getItem('pending_confirmation_email')
    if (storedEmail) {
      setEmail(storedEmail)
    }

    if (!token) {
      setState('error')
      setError('Invalid confirmation link')
      return
    }

    confirmEmail(token)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [token])

  const confirmEmail = async (confirmToken: string) => {
    try {
      setState('loading')
      setError(null)
      await apiClient.confirmEmail(confirmToken)
      setState('confirmed')
      sessionStorage.removeItem('pending_confirmation_email')
    } catch (err) {
      setState('expired')
      setError(err instanceof Error ? err.message : 'This link has expired or was already used')
    }
  }

  const handleResend = async () => {
    if (!email) return

    try {
      setResending(true)
      setError(null)
      await apiClient.resendConfirmation(email)
      setResent(true)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to resend confirmation email')
    } finally {
      setResending(false)
    }
  }

  const renderContent = () => {
    switch (state) {
      case 'loading':
        return (
          <div className="text-center py-12">
            <div className="text-muted-foreground">Confirming your email...</div>
          </div>
        )

      case 'confirmed':
        return (
          <div className="text-center space-y-4 py-8">
            <CheckCircle className="h-16 w-16 mx-auto text-green-500" />
            <div className="space-y-2">
              <h2 className="text-2xl font-bold">Email confirmed!</h2>
              <p className="text-muted-foreground">
                Your account is now active. You can log in.
              </p>
            </div>
            <Button onClick={() => navigate('/login')} size="lg" className="w-full">
              Go to Login
            </Button>
          </div>
        )

      case 'expired':
      case 'error':
        return (
          <div className="space-y-6">
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertTitle>Link expired or invalid</AlertTitle>
              <AlertDescription>
                {error || 'This confirmation link has expired or was already used.'}
              </AlertDescription>
            </Alert>

            {resent ? (
              <p className="text-center text-sm text-muted-foreground">
                A new confirmation email is on its way. Check your inbox.
              </p>
            ) : (
              <div className="space-y-3">
                {!sessionStorage.getItem('pending_confirmation_email') && (
                  <div className="space-y-2">
                    <label htmlFor="resend-email" className="text-sm font-medium">
                      Email
                    </label>
                    <Input
                      id="resend-email"
                      type="email"
                      placeholder="you@example.com"
                      value={email}
                      onChange={(e) => setEmail(e.target.value)}
                    />
                  </div>
                )}
                <Button
                  onClick={handleResend}
                  disabled={resending || !email}
                  className="w-full"
                >
                  {resending ? 'Resending...' : 'Resend confirmation email'}
                </Button>
              </div>
            )}

            <Button variant="outline" onClick={() => navigate('/login')} className="w-full">
              Back to Login
            </Button>
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
          <CardDescription className="text-center">Email Confirmation</CardDescription>
        </CardHeader>
        <CardContent>{renderContent()}</CardContent>
      </Card>
    </div>
  )
}
