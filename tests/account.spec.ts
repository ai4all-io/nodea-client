import { test, expect, type Page } from '@playwright/test'

/**
 * Settings → Account and the app's reactions to membership changes (#212,
 * #241), driven through demo mode: `?demo=true&account=<state>` pins the
 * account (see src/lib/mockApi.ts), with no backend or login.
 */
async function gotoSettings(page: Page, account?: string) {
  await page.goto(`/settings?demo=true${account ? `&account=${account}` : ''}`)
  await expect(page.getByRole('heading', { name: 'Account' })).toBeVisible()
}

test.describe('Settings → Account (demo mode)', () => {
  test('shows who is signed in, the organization and the actions', async ({ page }) => {
    await gotoSettings(page)

    const account = page.getByRole('region', { name: 'Account' })
    await expect(account.getByText('demo@getnodea.com')).toBeVisible()
    await expect(account.getByText('Member of Demo Organization')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Manage account ↗' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Leave' })).toBeEnabled()
    await expect(page.getByRole('button', { name: 'Delete' })).toBeEnabled()
    await expect(page.getByText('The 2 API keys you created stop working.')).toBeVisible()
  })

  test('the only admin cannot leave or delete, and is told why', async ({ page }) => {
    await gotoSettings(page, 'only-admin')

    await expect(page.getByRole('button', { name: 'Leave' })).toBeDisabled()
    await expect(page.getByRole('button', { name: 'Delete' })).toBeDisabled()
    await expect(page.getByText(/only admin/).first()).toBeVisible()
  })

  test('leaving needs the organization name, then lands on onboarding', async ({ page }) => {
    await gotoSettings(page)
    await page.getByRole('button', { name: 'Leave' }).click()

    const dialog = page.getByRole('dialog')
    const confirm = dialog.getByRole('button', { name: 'Leave organization' })
    await expect(confirm).toBeDisabled()
    await dialog.getByLabel(/to confirm/).fill('demo organization')
    await confirm.click()

    await expect(page).toHaveURL(/\/onboarding/)
    await expect(page.getByText("You're no longer in Demo Organization")).toBeVisible()
  })

  test('the last member is warned the organization goes too', async ({ page }) => {
    await gotoSettings(page, 'last-member')
    await page.getByRole('button', { name: 'Leave' }).click()

    await expect(page.getByRole('dialog')).toContainText('Leave and delete Demo Organization?')
  })

  test('deleting needs the email, then signs out with a notice', async ({ page }) => {
    await gotoSettings(page)
    await page.getByRole('button', { name: 'Delete' }).click()

    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText('Downloaded models stay on this Mac.')
    const confirm = dialog.getByRole('button', { name: 'Delete account' })
    await expect(confirm).toBeDisabled()
    await dialog.getByLabel(/to confirm/).fill('DEMO@getnodea.com')
    await confirm.click()

    await expect(page).toHaveURL(/\/login\?account=deleted/)
    await expect(page.getByText('This account was deleted.')).toBeVisible()
  })
})

test.describe('Membership changes made elsewhere (demo mode)', () => {
  test('leaving in the web console sends the app to onboarding', async ({ page }) => {
    await page.goto('/node?demo=true&phase=serving&account=orgless')

    await expect(page).toHaveURL(/\/onboarding/)
    await expect(page.getByText("You're no longer in Demo Organization")).toBeVisible()
  })

  test('a deleted account is signed out with a notice', async ({ page }) => {
    await page.goto('/node?demo=true&phase=serving&account=deleted')

    await expect(page).toHaveURL(/\/login\?account=deleted/)
    await expect(page.getByText('This account was deleted.')).toBeVisible()
  })

  test('joining an organization elsewhere takes onboarding back to the node page', async ({ page }) => {
    // Demo mode has an organization, which is the state the membership check
    // reaches after an invitation is accepted in the browser.
    await page.goto('/onboarding?demo=true')

    await expect(page).toHaveURL(/\/node/)
  })
})
