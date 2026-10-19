import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../contexts/AuthContext'
import { apiClient } from '../lib/api'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Card } from '../components/ui/card'
import { toast } from 'sonner'

export function OnboardingPage() {
  const [organizationName, setOrganizationName] = useState('')
  const [isChecking, setIsChecking] = useState(false)
  const [isCreating, setIsCreating] = useState(false)
  const [suggestedSlug, setSuggestedSlug] = useState('')
  const [validationError, setValidationError] = useState('')
  const { refreshOrganization, leftOrganizationName, organization } = useAuth()
  const navigate = useNavigate()

  // Joined an organization elsewhere (an invitation accepted in the browser,
  // or re-added) while this page was open: the membership check picks it up,
  // and the node page is where sharing resumes. Not for a fresh signup, whom
  // ProtectedRoute keeps here on purpose until they finish.
  useEffect(() => {
    if (organization && !isCreating && sessionStorage.getItem('just_signed_up') !== 'true') {
      navigate('/node', { replace: true })
    }
  }, [organization, isCreating, navigate])

  // Debounced name availability check
  useEffect(() => {
    if (!organizationName.trim()) {
      setSuggestedSlug('')
      setValidationError('')
      return
    }

    const timer = setTimeout(async () => {
      setIsChecking(true)
      try {
        const result = await apiClient.checkOrganizationName(organizationName)
        setSuggestedSlug(result.suggested_slug)

        if (!result.available) {
          setValidationError(result.error || 'Name is not available')
        } else {
          setValidationError('')
        }
      } catch (error) {
        console.error('Name check failed:', error)
        setValidationError('Failed to check name availability')
      } finally {
        setIsChecking(false)
      }
    }, 500)

    return () => clearTimeout(timer)
  }, [organizationName])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()

    if (validationError) {
      toast.error('Please fix validation errors')
      return
    }

    if (!organizationName.trim()) {
      toast.error('Please enter an organization name')
      return
    }

    setIsCreating(true)

    try {
      await apiClient.createOrganizationOnboarding(organizationName)

      // Refresh organization in auth context
      await refreshOrganization()

      // Clear the "just signed up" flag
      sessionStorage.removeItem('just_signed_up')

      toast.success('Organization created successfully!')
      navigate('/node')
    } catch (error: any) {
      console.error('Failed to create organization:', error)
      toast.error(error.message || 'Failed to create organization')
    } finally {
      setIsCreating(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <Card className="w-full max-w-md p-6">
        <div className="text-center mb-6">
          {leftOrganizationName ? (
            <>
              <h1 className="text-2xl font-bold mb-2">You're no longer in {leftOrganizationName}</h1>
              <p className="text-muted-foreground">
                This Mac stopped sharing compute. Create an organization below, or open an
                invitation link to join one.
              </p>
            </>
          ) : (
            <>
              <h1 className="text-2xl font-bold mb-2">Welcome to Nodea!</h1>
              <p className="text-muted-foreground">
                Let's set up your organization to get started
              </p>
            </>
          )}
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="orgName" className="block text-sm font-medium mb-1">
              Organization Name
            </label>
            <Input
              id="orgName"
              type="text"
              value={organizationName}
              onChange={(e) => setOrganizationName(e.target.value)}
              placeholder="e.g., Acme Corporation"
              disabled={isCreating}
              className={validationError ? 'border-destructive' : ''}
              maxLength={255}
            />

            {isChecking && (
              <p className="text-sm text-muted-foreground mt-1">
                Checking availability...
              </p>
            )}

            {validationError && (
              <p className="text-sm text-destructive mt-1">
                {validationError}
              </p>
            )}

            {suggestedSlug && !validationError && (
              <p className="text-sm text-muted-foreground mt-1">
                URL: <span className="font-mono">{suggestedSlug}</span>
              </p>
            )}
          </div>

          <Button
            type="submit"
            className="w-full"
            disabled={isCreating || isChecking || !!validationError || !organizationName.trim()}
          >
            {isCreating ? 'Creating...' : 'Continue'}
          </Button>
        </form>
      </Card>
    </div>
  )
}
