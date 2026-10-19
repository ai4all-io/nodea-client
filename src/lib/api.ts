// API client for backend REST API
import { mockApiClient } from './mockApi'
import { invoke } from '@tauri-apps/api/tauri'

const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || 'http://localhost:8080/site'

interface ApiKeyResponse {
  id: number
  user_id: string
  name: string
  value: string
  created_at: string
}

interface DeviceResponse {
  id: string
  user_id: string
  name: string
  type: string
  model: string
  connected: boolean
}

interface DeviceDetailResponse extends DeviceResponse {
  tokens_last_day: number
  tokens_last_week: number
}

interface TokenStatsResponse {
  date: string
  value: number
}

interface CompletionRequest {
  text: string
  model: string
}

interface CompletionResponse {
  id: string
}

interface Organization {
  id: number
  uuid?: string
  name: string
  slug: string
  member_count?: number
  created_at: string
}

interface OrganizationMember {
  id: string
  uuid: string
  email: string
  name: string | null
  role: 'admin' | 'standard'
  is_active: boolean
  created_at: string
  connected: boolean
  models: string[]
}

interface UserInvitation {
  id: number
  uuid: string
  organization_id: number
  email: string
  role: 'admin' | 'standard'
  token: string
  status: 'pending' | 'accepted' | 'revoked'
  expires_at: string
  created_at: string
  created_by: string
  accepted_at: string | null
  accepted_by: string | null
}

interface InvitationCheckResponse {
  valid: boolean
  organization_name?: string
  role?: 'admin' | 'standard'
  email?: string
  error?: string
}

interface CheckNameAvailabilityResponse {
  available: boolean
  suggested_slug: string
  error?: string
}

interface SignupResponse {
  message: string
  email: string
}

interface ConfirmEmailResponse {
  message: string
}

// The signed-in user's own account (GET /v1/me).
interface Me {
  uuid: string
  email: string | null
  full_name: string | null
  role: 'admin' | 'standard'
  is_active: boolean
  organization: { uuid: string; name: string; member_count: number } | null
  can_leave: boolean
  leave_blocked_reason: 'last_admin' | null
  leave_deletes_organization: boolean
  leave_disables_api_keys: number
}

class ApiError extends Error {
  status: number
  // Machine-readable reason the backend attaches when the client must act
  // on it, e.g. "last_admin" or "account_deleted".
  code: string | null

  constructor(message: string, status: number, code: string | null = null) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.code = code
  }
}

class ApiClient {
  private token: string | null = null
  private accountDeletedHandler: (() => void) | null = null

  setToken(token: string) {
    this.token = token
  }

  // Called when any request is refused because the account was deleted - a
  // token issued before the deletion stays valid until it expires, so the
  // session is ended here rather than on the next sign-in.
  setAccountDeletedHandler(handler: (() => void) | null) {
    this.accountDeletedHandler = handler
  }

  private async fetch<T>(
    endpoint: string,
    options: RequestInit = {}
  ): Promise<T> {
    const startTime = performance.now()
    const method = options.method || 'GET'
    const requestBody = options.body as string | undefined

    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }

    if (this.token) {
      headers['Authorization'] = `Bearer ${this.token}`
    }

    // Merge with options headers if provided
    if (options.headers) {
      Object.assign(headers, options.headers)
    }

    let responseBody: string | undefined
    let statusCode: number
    let error: Error | undefined

    try {
      const response = await fetch(`${API_BASE_URL}${endpoint}`, {
        ...options,
        headers,
      })

      statusCode = response.status

      if (!response.ok) {
        responseBody = await response.text().catch(() => undefined)
        let data: { error?: string; message?: string; code?: string } | undefined
        try {
          data = responseBody ? JSON.parse(responseBody) : undefined
        } catch {
          data = undefined
        }
        const code = typeof data?.code === 'string' ? data.code : null
        error = new ApiError(
          data?.error || data?.message || `API Error: ${response.statusText}`,
          response.status,
          code
        )
        if (response.status === 401 && code === 'account_deleted') this.accountDeletedHandler?.()
      } else {
        const responseText = await response.text()
        responseBody = responseText

        const endTime = performance.now()
        const duration = Math.round(endTime - startTime)

        // Log the successful API call (only in dev mode)
        if (import.meta.env.DEV) {
          this.logApiCall(method, endpoint, statusCode, requestBody, responseBody, duration)
        }

        return JSON.parse(responseText)
      }
    } catch (e) {
      statusCode = 0
      error = e as Error
      responseBody = error.message
    }

    const endTime = performance.now()
    const duration = Math.round(endTime - startTime)

    // Log the failed API call (only in dev mode)
    if (import.meta.env.DEV) {
      this.logApiCall(method, endpoint, statusCode, requestBody, responseBody, duration)
    }

    throw error!
  }

  private logApiCall(
    method: string,
    url: string,
    statusCode: number,
    requestBody: string | undefined,
    responseBody: string | undefined,
    durationMs: number
  ): void {
    // Fire and forget - don't await or block the API call
    invoke('log_rest_api_call', {
      method,
      url,
      statusCode,
      requestBody,
      responseBody,
      durationMs,
    }).catch(err => {
      console.error('Failed to log API call:', err)
    })
  }

  // API Keys
  async getApiKeys(): Promise<ApiKeyResponse[]> {
    return this.fetch<ApiKeyResponse[]>('/v1/api-keys')
  }

  async createApiKey(name: string): Promise<ApiKeyResponse> {
    return this.fetch<ApiKeyResponse>('/v1/api-keys', {
      method: 'POST',
      body: JSON.stringify({ name }),
    })
  }

  async deleteApiKey(id: number): Promise<void> {
    return this.fetch<void>(`/v1/api-keys/${id}`, {
      method: 'DELETE',
    })
  }

  // Devices
  async getUserDevices(): Promise<DeviceResponse[]> {
    return this.fetch<DeviceResponse[]>('/v1/user-devices')
  }

  async getDevice(deviceId: string): Promise<DeviceDetailResponse> {
    return this.fetch<DeviceDetailResponse>(`/v1/devices/${deviceId}`)
  }

  // Token Stats
  async getConsumedTokens(): Promise<TokenStatsResponse[]> {
    return this.fetch<TokenStatsResponse[]>('/v1/token-stats')
  }

  async getGeneratedTokens(): Promise<TokenStatsResponse[]> {
    return this.fetch<TokenStatsResponse[]>('/v1/generated-tokens-ts')
  }

  // Completions
  async createCompletion(data: CompletionRequest): Promise<CompletionResponse> {
    return this.fetch<CompletionResponse>('/v1/completions/streams', {
      method: 'POST',
      body: JSON.stringify(data),
    })
  }

  // SSE streams
  createEventSource(endpoint: string): EventSource {
    const url = `${API_BASE_URL}${endpoint}`

    // Append token as query parameter if available (EventSource doesn't support custom headers)
    if (this.token) {
      const separator = endpoint.includes('?') ? '&' : '?'
      const authenticatedUrl = `${url}${separator}token=${encodeURIComponent(this.token)}`
      return new EventSource(authenticatedUrl)
    }

    return new EventSource(url)
  }

  createStreamEventSource(requestId: string): EventSource {
    return this.createEventSource(`/v1/completions/streams?request_id=${requestId}`)
  }

  // The caller's own devices. The organization-wide roster is the fleet
  // stream below - the two are different resources.
  createDevicesStream(): EventSource {
    return this.createEventSource('/v1/user-devices-stream')
  }

  createFleetStream(): EventSource {
    return this.createEventSource('/v1/fleet/stream')
  }

  createModelsStream(): EventSource {
    return this.createEventSource('/v1/available-models-stream')
  }

  createEventsStream(): EventSource {
    return this.createEventSource('/v1/events')
  }

  // The caller's own account
  async getMe(): Promise<Me> {
    return this.fetch<Me>('/v1/me')
  }

  // `confirm` is the organization's name, typed by the user.
  async leaveOrganization(confirm: string): Promise<{ organization_deleted: boolean }> {
    return this.fetch<{ organization_deleted: boolean }>('/v1/me/leave-organization', {
      method: 'POST',
      body: JSON.stringify({ confirm }),
    })
  }

  // `confirm` is the account's email, typed by the user.
  async deleteAccount(confirm: string): Promise<{ organization_deleted: boolean; auth_user_deleted: boolean }> {
    return this.fetch<{ organization_deleted: boolean; auth_user_deleted: boolean }>('/v1/me/delete', {
      method: 'POST',
      body: JSON.stringify({ confirm }),
    })
  }

  // Organizations
  async getOrganization(): Promise<Organization> {
    return this.fetch<Organization>('/v1/organization')
  }

  async updateOrganization(data: { name?: string }): Promise<Organization> {
    return this.fetch<Organization>('/v1/organization', {
      method: 'PUT',
      body: JSON.stringify(data),
    })
  }

  // Onboarding
  async checkOrganizationName(name: string): Promise<CheckNameAvailabilityResponse> {
    return this.fetch<CheckNameAvailabilityResponse>(
      `/v1/onboarding/check-name?name=${encodeURIComponent(name)}`
    )
  }

  async createOrganizationOnboarding(name: string): Promise<{ organization: Organization }> {
    return this.fetch<{ organization: Organization }>('/v1/onboarding/organization', {
      method: 'POST',
      body: JSON.stringify({ name }),
    })
  }

  // Organization Members
  async getOrganizationMembers(): Promise<{ users: OrganizationMember[], total_count: number }> {
    return this.fetch<{ users: OrganizationMember[], total_count: number }>('/v1/users')
  }

  async updateOrganizationMember(
    userId: string,
    data: { role?: 'admin' | 'standard' }
  ): Promise<OrganizationMember> {
    return this.fetch<OrganizationMember>(`/v1/users/${userId}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    })
  }

  async deactivateOrganizationMember(userId: string): Promise<void> {
    return this.fetch<void>(`/v1/users/${userId}`, {
      method: 'DELETE',
    })
  }

  // Invitations
  async createInvitation(data: {
    email: string
    role: 'admin' | 'standard'
  }): Promise<UserInvitation> {
    return this.fetch<UserInvitation>('/v1/invitations', {
      method: 'POST',
      body: JSON.stringify(data),
    })
  }

  async getInvitations(): Promise<UserInvitation[]> {
    const response = await this.fetch<{
      invitations: UserInvitation[]
      page: number
      per_page: number
      total_count: number
    }>('/v1/invitations')
    return response.invitations
  }

  async revokeInvitation(invitationUuid: string): Promise<void> {
    return this.fetch<void>(`/v1/invitations/${invitationUuid}`, {
      method: 'DELETE',
    })
  }

  async acceptInvitation(token: string): Promise<void> {
    return this.fetch<void>(`/v1/invitations/${token}/accept`, {
      method: 'POST',
    })
  }

  async signupWithInvitation(
    token: string,
    password: string
  ): Promise<{ email: string; organization_id: number; role: 'admin' | 'standard' }> {
    return this.fetch(`/v1/invitations/${token}/signup`, {
      method: 'POST',
      body: JSON.stringify({ password }),
    })
  }

  async checkInvitation(token: string): Promise<InvitationCheckResponse> {
    return this.fetch<InvitationCheckResponse>(`/v1/invitations/check/${token}`)
  }

  // Signup / email verification
  async signup(email: string, password: string): Promise<SignupResponse> {
    return this.fetch<SignupResponse>('/v1/signup', {
      method: 'POST',
      body: JSON.stringify({ email, password }),
    })
  }

  async confirmEmail(token: string): Promise<ConfirmEmailResponse> {
    return this.fetch<ConfirmEmailResponse>(`/v1/confirm-email/${token}`)
  }

  async resendConfirmation(email: string): Promise<{ message: string }> {
    return this.fetch<{ message: string }>('/v1/confirm-email/resend', {
      method: 'POST',
      body: JSON.stringify({ email }),
    })
  }
}

// Check if demo mode is enabled (set via URL param or localStorage)
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

// Create API client - use mock in demo mode
const createApiClient = (): ApiClient => {
  if (isDemoMode()) {
    console.log('🎭 Running in DEMO MODE - using mock data')
    return mockApiClient as any
  }
  return new ApiClient()
}

// Export a singleton instance
const apiClient = createApiClient()

export { apiClient, ApiError }
export type {
  Me,
  ApiKeyResponse,
  DeviceResponse,
  DeviceDetailResponse,
  TokenStatsResponse,
  Organization,
  OrganizationMember,
  UserInvitation,
  InvitationCheckResponse,
  CheckNameAvailabilityResponse,
  SignupResponse,
  ConfirmEmailResponse,
}
