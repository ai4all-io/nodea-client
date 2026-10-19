// Mock API client for demo/development without backend
import type {
  Me,
  ApiKeyResponse,
  DeviceResponse,
  DeviceDetailResponse,
  TokenStatsResponse,
  Organization,
  OrganizationMember,
  UserInvitation,
  InvitationCheckResponse,
  SignupResponse,
  ConfirmEmailResponse,
} from './api'

// Mock data
const mockApiKeys: ApiKeyResponse[] = [
  {
    id: 1,
    user_id: 'demo-user',
    name: 'Production Server',
    value: 'ai4a_prod_1234567890abcdef1234567890abcdef',
    created_at: new Date(Date.now() - 7 * 24 * 60 * 60 * 1000).toISOString(),
  },
  {
    id: 2,
    user_id: 'demo-user',
    name: 'Mobile App',
    value: 'ai4a_mobile_abcdef1234567890abcdef1234567890',
    created_at: new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString(),
  },
  {
    id: 3,
    user_id: 'demo-user',
    name: 'Development Testing',
    value: 'ai4a_dev_test_9876543210fedcba9876543210fedcba',
    created_at: new Date(Date.now() - 1 * 60 * 60 * 1000).toISOString(),
  },
]

const mockDevices: DeviceResponse[] = [
  {
    id: 'device-001',
    user_id: 'demo-user',
    name: 'MacBook Pro',
    type: 'laptop',
    model: 'MacBook Pro M3',
    connected: true,
  },
  {
    id: 'device-002',
    user_id: 'demo-user',
    name: 'iPhone 15',
    type: 'smartphone',
    model: 'iPhone 15 Pro',
    connected: true,
  },
  {
    id: 'device-003',
    user_id: 'demo-user',
    name: 'iPad Air',
    type: 'tablet',
    model: 'iPad Air 5th Gen',
    connected: false,
  },
]

const mockDeviceDetails: Record<string, DeviceDetailResponse> = {
  'device-001': {
    ...mockDevices[0],
    tokens_last_day: 125400,
    tokens_last_week: 892350,
  },
  'device-002': {
    ...mockDevices[1],
    tokens_last_day: 45200,
    tokens_last_week: 312450,
  },
  'device-003': {
    ...mockDevices[2],
    tokens_last_day: 0,
    tokens_last_week: 15800,
  },
}

// Generate mock time-series data
const generateTimeSeriesData = (hours: number, baseValue: number): TokenStatsResponse[] => {
  const data: TokenStatsResponse[] = []
  const now = Date.now()

  for (let i = hours; i >= 0; i--) {
    const date = new Date(now - i * 60 * 60 * 1000)
    const variance = Math.random() * 0.4 + 0.8 // 0.8 to 1.2
    const value = Math.floor(baseValue * variance)

    data.push({
      date: date.toISOString(),
      value,
    })
  }

  return data
}

const mockConsumedTokens = generateTimeSeriesData(24, 5000)
const mockGeneratedTokens = generateTimeSeriesData(24, 3000)

const mockModels = [
  { id: 'llama-3-8b', name: 'Llama 3 8B' },
  { id: 'llama-3-70b', name: 'Llama 3 70B' },
  { id: 'mistral-7b', name: 'Mistral 7B' },
  { id: 'phi-3-mini', name: 'Phi-3 Mini' },
]

const mockOrganization: Organization = {
  id: 1,
  name: 'Demo Organization',
  slug: 'demo-org',
  member_count: 5,
  created_at: new Date(Date.now() - 90 * 24 * 60 * 60 * 1000).toISOString(),
}

const mockMembers: OrganizationMember[] = [
  {
    id: '1',
    uuid: 'demo-user-uuid',
    email: 'demo@getnodea.com',
    name: 'Demo User',
    role: 'admin',
    is_active: true,
    created_at: new Date(Date.now() - 90 * 24 * 60 * 60 * 1000).toISOString(),
    connected: true,
    models: ['llama-3-8b'],
  },
  {
    id: '2',
    uuid: 'user-2-uuid',
    email: 'alice@getnodea.com',
    name: 'Alice Johnson',
    role: 'admin',
    is_active: true,
    created_at: new Date(Date.now() - 60 * 24 * 60 * 60 * 1000).toISOString(),
    connected: true,
    models: ['mistral-7b'],
  },
  {
    id: '3',
    uuid: 'user-3-uuid',
    email: 'bob@getnodea.com',
    name: 'Bob Smith',
    role: 'standard',
    is_active: true,
    created_at: new Date(Date.now() - 45 * 24 * 60 * 60 * 1000).toISOString(),
    connected: false,
    models: [],
  },
  {
    id: '4',
    uuid: 'user-4-uuid',
    email: 'charlie@getnodea.com',
    name: 'Charlie Davis',
    role: 'standard',
    is_active: true,
    created_at: new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString(),
    connected: false,
    models: [],
  },
  {
    id: '5',
    uuid: 'user-5-uuid',
    email: 'diana@getnodea.com',
    name: 'Diana Prince',
    role: 'standard',
    is_active: false,
    created_at: new Date(Date.now() - 15 * 24 * 60 * 60 * 1000).toISOString(),
    connected: false,
    models: [],
  },
]

const mockInvitations: UserInvitation[] = [
  {
    id: 1,
    uuid: 'mock-invitation-uuid-1',
    organization_id: 1,
    email: 'emily@example.com',
    role: 'standard',
    token: 'mock-invitation-token-1',
    status: 'pending',
    expires_at: new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString(),
    created_at: new Date(Date.now() - 2 * 24 * 60 * 60 * 1000).toISOString(),
    created_by: 'demo-user-uuid',
    accepted_at: null,
    accepted_by: null,
  },
  {
    id: 2,
    uuid: 'mock-invitation-uuid-2',
    organization_id: 1,
    email: 'frank@example.com',
    role: 'admin',
    token: 'mock-invitation-token-2',
    status: 'pending',
    expires_at: new Date(Date.now() + 6 * 24 * 60 * 60 * 1000).toISOString(),
    created_at: new Date(Date.now() - 1 * 24 * 60 * 60 * 1000).toISOString(),
    created_by: 'demo-user-uuid',
    accepted_at: null,
    accepted_by: null,
  },
]

// Simulate API delay
const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

/**
 * The demo account's state, pinned with ?account=<state> (with ?demo=true) so
 * tests can reach each one directly:
 *   orgless      - no organization (left it, or never joined)
 *   inactive     - an admin deactivated the account
 *   deleted      - the account was deleted (every call refused)
 *   only-admin   - the only admin, so leaving is blocked
 *   last-member  - the last member, so leaving deletes the organization
 */
type DemoAccountState = 'member' | 'orgless' | 'inactive' | 'deleted' | 'only-admin' | 'last-member'

const demoAccountState = (): DemoAccountState => {
  const value = new URLSearchParams(window.location.search).get('account')
  return (['orgless', 'inactive', 'deleted', 'only-admin', 'last-member'] as const).find((s) => s === value) ?? 'member'
}

// Shaped like the real ApiError (status + code) without importing api.ts,
// which imports this module.
const demoApiError = (message: string, status: number, code: string | null = null) =>
  Object.assign(new Error(message), { name: 'ApiError', status, code })

let mockApiKeyIdCounter = 4
let mockInvitationIdCounter = 3

export class MockApiClient {
  private token: string | null = null
  private accountDeletedHandler: (() => void) | null = null
  // What leave/delete did during this page's life, on top of ?account=.
  private left = false
  private deleted = false

  setAccountDeletedHandler(handler: (() => void) | null) {
    this.accountDeletedHandler = handler
  }

  private accountState(): DemoAccountState {
    if (this.deleted) return 'deleted'
    if (this.left) return 'orgless'
    return demoAccountState()
  }

  async getMe(): Promise<Me> {
    await delay(100)
    const state = this.accountState()
    if (state === 'deleted') {
      this.accountDeletedHandler?.()
      throw demoApiError('Account deleted', 401, 'account_deleted')
    }
    const inOrg = state !== 'orgless'
    return {
      uuid: 'demo-user-uuid',
      email: 'demo@getnodea.com',
      full_name: 'Demo User',
      role: state === 'only-admin' || state === 'last-member' ? 'admin' : 'standard',
      is_active: state !== 'inactive',
      organization: inOrg
        ? { uuid: 'demo-org-uuid', name: this.organization.name, member_count: state === 'last-member' ? 1 : 5 }
        : null,
      can_leave: inOrg && state !== 'only-admin',
      leave_blocked_reason: state === 'only-admin' ? 'last_admin' : null,
      leave_deletes_organization: state === 'last-member',
      leave_disables_api_keys: inOrg && state !== 'last-member' ? 2 : 0,
    }
  }

  async leaveOrganization(confirm: string): Promise<{ organization_deleted: boolean }> {
    await delay(200)
    if (confirm.trim().toLowerCase() !== this.organization.name.toLowerCase()) {
      throw demoApiError('Type the organization name to confirm', 400, 'confirmation_mismatch')
    }
    const deletesOrganization = this.accountState() === 'last-member'
    this.left = true
    return { organization_deleted: deletesOrganization }
  }

  async deleteAccount(confirm: string): Promise<{ organization_deleted: boolean; auth_user_deleted: boolean }> {
    await delay(200)
    if (confirm.trim().toLowerCase() !== 'demo@getnodea.com') {
      throw demoApiError('Type your email to confirm', 400, 'confirmation_mismatch')
    }
    this.deleted = true
    return { organization_deleted: false, auth_user_deleted: true }
  }
  private apiKeys = [...mockApiKeys]
  private members = [...mockMembers]
  private invitations = [...mockInvitations]
  private organization = { ...mockOrganization }

  setToken(token: string) {
    this.token = token
  }

  getToken() {
    return this.token
  }

  async getApiKeys(): Promise<ApiKeyResponse[]> {
    await delay(300)
    return [...this.apiKeys]
  }

  async createApiKey(name: string): Promise<ApiKeyResponse> {
    await delay(500)
    const newKey: ApiKeyResponse = {
      id: mockApiKeyIdCounter++,
      user_id: 'demo-user',
      name,
      value: `ai4a_${Date.now()}_${Math.random().toString(36).substring(2, 15)}`,
      created_at: new Date().toISOString(),
    }
    this.apiKeys.push(newKey)
    return newKey
  }

  async deleteApiKey(id: number): Promise<void> {
    await delay(400)
    const index = this.apiKeys.findIndex(k => k.id === id)
    if (index !== -1) {
      this.apiKeys.splice(index, 1)
    }
  }

  async getUserDevices(): Promise<DeviceResponse[]> {
    await delay(300)
    return [...mockDevices]
  }

  async getDevice(deviceId: string): Promise<DeviceDetailResponse> {
    await delay(300)
    const device = mockDeviceDetails[deviceId]
    if (!device) {
      throw new Error('Device not found')
    }
    return { ...device }
  }

  async getConsumedTokens(): Promise<TokenStatsResponse[]> {
    await delay(400)
    return [...mockConsumedTokens]
  }

  async getGeneratedTokens(): Promise<TokenStatsResponse[]> {
    await delay(400)
    return [...mockGeneratedTokens]
  }

  async createCompletion(_data: { text: string; model: string }): Promise<{ id: string }> {
    await delay(300)
    return {
      id: `completion-${Date.now()}`,
    }
  }

  // Organizations
  async getOrganization(): Promise<Organization> {
    await delay(300)
    return { ...this.organization }
  }

  async updateOrganization(data: { name?: string }): Promise<Organization> {
    await delay(400)
    if (data.name) {
      this.organization.name = data.name
    }
    return { ...this.organization }
  }

  // Organization Members
  async getOrganizationMembers(): Promise<{ users: OrganizationMember[], total_count: number }> {
    await delay(300)
    return {
      users: [...this.members],
      total_count: this.members.length
    }
  }

  async updateOrganizationMember(
    userId: string,
    data: { role?: 'admin' | 'standard' }
  ): Promise<OrganizationMember> {
    await delay(400)
    const member = this.members.find(m => m.uuid === userId)
    if (!member) {
      throw new Error('Member not found')
    }
    if (data.role) {
      member.role = data.role
    }
    return { ...member }
  }

  async deactivateOrganizationMember(userId: string): Promise<void> {
    await delay(400)
    const member = this.members.find(m => m.uuid === userId)
    if (!member) {
      throw new Error('Member not found')
    }
    member.is_active = false
  }

  // Invitations
  async createInvitation(data: {
    email: string
    role: 'admin' | 'standard'
  }): Promise<UserInvitation> {
    await delay(500)
    const newInvitation: UserInvitation = {
      id: mockInvitationIdCounter++,
      uuid: `mock-invitation-uuid-${mockInvitationIdCounter}`,
      organization_id: 1,
      email: data.email,
      role: data.role,
      token: `mock-invitation-token-${mockInvitationIdCounter}`,
      status: 'pending',
      expires_at: new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString(),
      created_at: new Date().toISOString(),
      created_by: 'demo-user-uuid',
      accepted_at: null,
      accepted_by: null,
    }
    this.invitations.push(newInvitation)
    return newInvitation
  }

  async getInvitations(): Promise<UserInvitation[]> {
    await delay(300)
    return [...this.invitations]
  }

  async revokeInvitation(invitationUuid: string): Promise<void> {
    await delay(400)
    const invitation = this.invitations.find(i => i.uuid === invitationUuid)
    if (invitation) {
      invitation.status = 'revoked'
    }
  }

  async acceptInvitation(_token: string): Promise<void> {
    await delay(400)
    // In mock mode, just simulate acceptance
    console.log('Mock: Invitation accepted')
  }

  async checkInvitation(token: string): Promise<InvitationCheckResponse> {
    await delay(300)
    const invitation = this.invitations.find(i => i.token === token)

    if (!invitation) {
      return {
        valid: false,
        error: 'Invitation not found',
      }
    }

    if (invitation.status !== 'pending') {
      return {
        valid: false,
        error: 'This invitation has already been used or revoked',
      }
    }

    if (new Date(invitation.expires_at) < new Date()) {
      return {
        valid: false,
        error: 'This invitation has expired',
      }
    }

    return {
      valid: true,
      organization_name: this.organization.name,
      role: invitation.role,
      email: invitation.email,
    }
  }

  // Signup / email verification
  async signup(email: string, _password: string): Promise<SignupResponse> {
    await delay(400)
    console.log('Mock: Signup confirmation email sent')
    return { message: 'Confirmation email sent', email }
  }

  async confirmEmail(_token: string): Promise<ConfirmEmailResponse> {
    await delay(300)
    console.log('Mock: Email confirmed')
    return { message: 'Email confirmed' }
  }

  async resendConfirmation(_email: string): Promise<{ message: string }> {
    await delay(300)
    console.log('Mock: Confirmation email resent')
    return { message: 'Confirmation email resent' }
  }

  // Onboarding
  async checkOrganizationName(name: string): Promise<{ available: boolean; suggested_slug: string; error?: string }> {
    await delay(300)
    // In mock mode, always return available with a slugified name
    const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')
    return {
      available: true,
      suggested_slug: slug,
    }
  }

  async createOrganizationOnboarding(name: string): Promise<{ organization: Organization }> {
    await delay(500)
    // In mock mode, just update the existing organization
    this.organization.name = name
    this.organization.slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')
    console.log('Mock: Organization created:', this.organization)
    return {
      organization: { ...this.organization }
    }
  }

  // Mock SSE streams
  createEventSource(_endpoint: string): EventSource {
    throw new Error('EventSource not supported in mock mode. Use createMockStream instead.')
  }

  createMockStream(type: 'models' | 'devices' | 'fleet' | 'completion', _data?: any): MockEventSource {
    return new MockEventSource(type)
  }

  createStreamEventSource(_requestId: string): MockEventSource {
    return this.createMockStream('completion')
  }

  createDevicesStream(): MockEventSource {
    return this.createMockStream('devices')
  }

  createFleetStream(): MockEventSource {
    return this.createMockStream('fleet')
  }

  createModelsStream(): MockEventSource {
    return this.createMockStream('models')
  }

  createEventsStream(): EventSource {
    throw new Error('Events stream not supported in mock mode')
  }
}

// Mock EventSource for SSE streams
class MockEventSource {
  onmessage: ((event: any) => void) | null = null
  onerror: ((event: any) => void) | null = null
  private closed = false
  private interval: number | null = null

  constructor(type: 'models' | 'devices' | 'fleet' | 'completion') {
    setTimeout(() => {
      if (this.closed) return

      switch (type) {
        case 'models':
          this.sendMessage(JSON.stringify(mockModels))
          break

        case 'devices':
        case 'fleet': {
          // Match the real backend envelopes: /v1/user-devices-stream sends
          // { type: 'user_devices', ... } and /v1/fleet/stream sends
          // { type: 'fleet', ... }. Same payload here - the mock has a single
          // organization, so a user's devices and its fleet coincide.
          const envelope = type === 'fleet' ? 'fleet' : 'user_devices'
          this.sendMessage(JSON.stringify({ type: envelope, value: mockDevices }))
          // Send updates every 5 seconds
          this.interval = window.setInterval(() => {
            if (!this.closed && this.onmessage) {
              // Randomly toggle connection status
              const updatedDevices = mockDevices.map(d => ({
                ...d,
                connected: Math.random() > 0.3,
              }))
              this.sendMessage(JSON.stringify({ type: envelope, value: updatedDevices }))
            }
          }, 5000)
          break
        }

        case 'completion':
          this.simulateCompletion()
          break
      }
    }, 100)
  }

  private sendMessage(data: string) {
    if (this.closed || !this.onmessage) return
    this.onmessage({ data })
  }

  private async simulateCompletion() {
    const responses = [
      "Hello! I'm an AI assistant running in demo mode. ",
      "I can help you with various tasks like answering questions, ",
      "providing information, and having conversations. ",
      "This is a simulated streaming response to demonstrate the chat interface. ",
      "In production, this would be connected to a real AI model. ",
      "Feel free to explore the other features of the application!"
    ]

    let fullText = ''

    for (const chunk of responses) {
      if (this.closed) return

      await delay(150)
      fullText += chunk

      this.sendMessage(JSON.stringify({
        type: 'content',
        text: chunk,
      }))
    }

    await delay(200)
    this.sendMessage(JSON.stringify({
      type: 'done',
      full_text: fullText,
    }))
  }

  close() {
    this.closed = true
    if (this.interval) {
      clearInterval(this.interval)
    }
  }
}

export const mockApiClient = new MockApiClient()
