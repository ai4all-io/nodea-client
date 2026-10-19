import { test, expect, type Page } from '@playwright/test'

/**
 * Fleet page (formerly "Devices"), driven entirely through demo mode
 * (`?demo=true`, see src/lib/mockTauri.ts and src/lib/mockApi.ts). No backend
 * or login required.
 *
 * `?role=standard` demotes the auto-authenticated demo user from admin to an
 * ordinary member. The roster is the *organization's*, not the caller's, so
 * every active member is allowed to see it - the page used to sit behind an
 * AdminRoute that redirected standard users to /node.
 */
async function gotoFleet(page: Page, role?: 'standard') {
  await page.goto(`/fleet?demo=true${role ? `&role=${role}` : ''}`)
  await page.waitForLoadState('domcontentloaded')
}

test.describe('Fleet (demo mode)', () => {
  for (const role of ['admin', 'standard'] as const) {
    test(`a ${role} member can list the fleet`, async ({ page }) => {
      await gotoFleet(page, role === 'standard' ? 'standard' : undefined)

      // Still on /fleet: no route guard bounced an ordinary member to /node.
      await expect(page).toHaveURL(/\/fleet/)

      // The serving count drifts (the mock stream re-rolls it every 5s), but
      // the organization's device total does not.
      await expect(page.locator('h1')).toContainText(/Fleet\s*·\s*\d+ of 3 serving/)

      // "MacBook Pro" is the machine the app reports as its own, so its name
      // cell carries the badge too - assert on the badge instead of an exact
      // text match that the badge breaks.
      await expect(page.getByText('this Mac')).toBeVisible()
      await expect(page.getByText('iPhone 15', { exact: true })).toBeVisible()
      await expect(page.getByText('iPad Air', { exact: true })).toBeVisible()
    })
  }

  test('the sidebar offers Fleet to a standard member', async ({ page }) => {
    await gotoFleet(page, 'standard')

    const fleet = page.getByRole('navigation').getByRole('link', { name: 'Fleet' })
    await expect(fleet).toBeVisible()

    await fleet.click()
    await expect(page).toHaveURL(/\/fleet/)
  })

  // Both hand-offs to the web app must land inside the console. The bare
  // app.getnodea.com origin is the marketing/auth surface, not the console.
  const CONSOLE_URL = 'https://app.getnodea.com/console/'

  test('the "Open console" link points at the console', async ({ page }) => {
    await gotoFleet(page)

    await expect(page.getByRole('link', { name: /Open console/ }))
      .toHaveAttribute('href', CONSOLE_URL)
  })

  test('the sidebar account menu opens the console', async ({ page }) => {
    // window.open is a no-op in the Tauri webview under test, so record the
    // URL it was called with rather than waiting for a popup that never comes.
    await page.addInitScript(() => {
      ;(window as unknown as { __openedUrls: string[] }).__openedUrls = []
      window.open = ((url?: string | URL) => {
        ;(window as unknown as { __openedUrls: string[] }).__openedUrls.push(String(url))
        return null
      }) as typeof window.open
    })
    await gotoFleet(page)

    await page.getByRole('button', { name: /Demo User/ }).click()
    await page.getByRole('button', { name: /Web console/ }).click()

    const opened = await page.evaluate(
      () => (window as unknown as { __openedUrls: string[] }).__openedUrls,
    )
    expect(opened).toEqual([CONSOLE_URL])
  })
})
