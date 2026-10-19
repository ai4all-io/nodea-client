import { test, expect } from '@playwright/test'

/**
 * Login page, with no backend and no Supabase running.
 *
 * Three of these tests only render the form, so they need nothing. The other
 * two need an authentication *outcome*, and each gets one locally:
 *
 *  - the failure path stubs the single GoTrue endpoint supabase-js calls
 *    (`/auth/v1/token`). Demo mode cannot cover this case: AuthContext's
 *    `login()` always resolves in demo mode, so there is no error to assert on.
 *  - the success path uses demo mode (`?demo=true`), where `login()` resolves
 *    locally and the page navigates to `redirectTo` - see
 *    src/contexts/AuthContext.tsx.
 */

// supabase-js posts here for a password grant. The glob covers the
// `?grant_type=password` query string.
const TOKEN_ENDPOINT = '**/auth/v1/token**'

test.describe('Login Page', () => {
  test('should display login form', async ({ page }) => {
    await page.goto('/login')

    await expect(page.getByText('Welcome back')).toBeVisible()
    await expect(page.locator('#email')).toBeVisible()
    await expect(page.locator('#password')).toBeVisible()
    await expect(page.locator('button[type="submit"]')).toBeVisible()
  })

  test('should show error for invalid credentials', async ({ page }) => {
    // The shape GoTrue actually returns for a bad password. Several spellings
    // of the message are included because supabase-js picks whichever of
    // `msg`/`error_description`/`message` the deployed version understands.
    await page.route(TOKEN_ENDPOINT, (route) =>
      route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({
          code: 400,
          error_code: 'invalid_credentials',
          msg: 'Invalid login credentials',
          error: 'invalid_grant',
          error_description: 'Invalid login credentials',
          message: 'Invalid login credentials',
        }),
      })
    )

    await page.goto('/login')
    await page.fill('#email', 'wrong@example.com')
    await page.fill('#password', 'wrongpassword')
    await page.click('button[type="submit"]')

    await expect(page.getByRole('alert')).toBeVisible()
  })

  test('should successfully login with valid credentials', async ({ page }) => {
    // Demo mode authenticates locally, so this exercises the form, the submit
    // handler and the redirect without a Supabase project or a backend.
    await page.goto('/login?demo=true')

    await page.fill('#email', 'demo@getnodea.com')
    await page.fill('#password', 'whatever')
    await page.click('button[type="submit"]')

    await page.waitForURL((url) => !url.pathname.includes('/login'), { timeout: 10000 })
    expect(page.url()).not.toContain('/login')
    // Proves the redirect landed in the authenticated shell rather than
    // bouncing straight back out of ProtectedRoute.
    await expect(page.locator('nav a:has-text("Node")')).toBeVisible()
  })

  test('should link to the dedicated signup page', async ({ page }) => {
    await page.goto('/login')

    await expect(page.locator('button[type="submit"]')).toContainText('Sign in')

    await page.getByRole('link', { name: 'Sign up' }).click()

    await page.waitForURL((url) => url.pathname === '/signup', { timeout: 10000 })
    expect(page.url()).toContain('/signup')
  })

  test('should show Google login button', async ({ page }) => {
    await page.goto('/login')

    await expect(page.locator('text=/continue with Google/i')).toBeVisible()
  })
})
