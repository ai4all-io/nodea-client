import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { Toaster } from './components/ui/sonner'
import { AuthProvider } from './contexts/AuthContext'
import { ProtectedRoute } from './components/ProtectedRoute'
import { DeepLinkListener } from './components/DeepLinkListener'
import { AppLayout } from './components/layout/AppLayout'
import { LoginPage } from './pages/LoginPage'
import { SignupPage } from './pages/SignupPage'
import { ConfirmEmailPage } from './pages/ConfirmEmailPage'
import { AcceptInvitationPage } from './pages/AcceptInvitationPage'
import { OnboardingPage } from './pages/OnboardingPage'
import { NodePage } from './pages/NodePage'
import { FleetPage } from './pages/FleetPage'
import { SettingsPage } from './pages/SettingsPage'
import { DevLogsPage } from './pages/DevLogsPage'
import { PlaygroundPage } from './pages/PlaygroundPage'

/* Removed routes: /rewards, /api-keys, /usage, /team, /devices/:deviceId.
 * Those live in the web console now. Playground and Dev Logs survive as
 * DEV-only debug tools (see Sidebar.tsx) - not part of the shipped app. */

function App() {
  return (
    <AuthProvider>
      <BrowserRouter>
        <DeepLinkListener />
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/signup" element={<SignupPage />} />
          <Route path="/confirm-email/:token" element={<ConfirmEmailPage />} />
          <Route path="/invitation/:token" element={<AcceptInvitationPage />} />
          <Route
            path="/onboarding"
            element={
              <ProtectedRoute>
                <OnboardingPage />
              </ProtectedRoute>
            }
          />
          <Route
            path="/"
            element={
              <ProtectedRoute>
                <AppLayout />
              </ProtectedRoute>
            }
          >
            <Route index element={<Navigate to="/node" replace />} />
            <Route path="node" element={<NodePage />} />
            <Route path="fleet" element={<FleetPage />} />
            <Route path="settings" element={<SettingsPage />} />
            {import.meta.env.DEV && <Route path="playground" element={<PlaygroundPage />} />}
            {import.meta.env.DEV && <Route path="dev-logs" element={<DevLogsPage />} />}
          </Route>

          <Route path="*" element={<Navigate to="/node" replace />} />
        </Routes>
        <Toaster />
      </BrowserRouter>
    </AuthProvider>
  )
}

export default App
