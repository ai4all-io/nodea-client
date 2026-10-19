import { test, expect, type Page } from '@playwright/test'

/**
 * Node page + sidebar status widget, driven entirely through demo mode
 * (`?demo=true&phase=<name>`, see src/lib/mockTauri.ts). No backend or login
 * required: each phase is pinned instantly instead of waiting through the
 * timed simulation, and demo mode auto-authenticates (AuthContext).
 */
async function gotoPhase(page: Page, phase: string) {
  await page.goto(`/node?demo=true&phase=${phase}`)
  await page.waitForLoadState('domcontentloaded')
}

test.describe('Node phases (demo mode)', () => {
  test('downloading: widget and hero agree', async ({ page }) => {
    await gotoPhase(page, 'downloading')

    await expect(page.getByText('Downloading', { exact: true })).toBeVisible()
    await expect(page.locator('h1')).toHaveText('Fetching the model for this Mac.')
    await expect(page.getByText('62%').first()).toBeVisible()
  })

  test('loading: widget and hero agree', async ({ page }) => {
    await gotoPhase(page, 'loading')

    await expect(page.getByText('Loading', { exact: true })).toBeVisible()
    await expect(page.locator('h1')).toHaveText('Loading the model into memory.')
  })

  test('stalled: a model failure shows retry, hero explains the interruption', async ({ page }) => {
    await gotoPhase(page, 'stalled')

    await expect(page.getByText('Download stalled', { exact: true })).toBeVisible()
    await expect(page.getByRole('button', { name: 'Retry now' })).toBeVisible()
    await expect(page.locator('h1')).toHaveText('The download stopped.')
  })

  test('offline: a connection error is not a download problem (#241)', async ({ page }) => {
    // The model is ready; only the coordinator is out of reach.
    await gotoPhase(page, 'offline')

    await expect(page.getByText('Offline', { exact: true }).first()).toBeVisible()
    await expect(page.locator('h1')).toHaveText("Can't reach the coordinator.")
    await expect(page.getByText(/download/i)).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Retry now' })).toBeVisible()
  })

  test('refused: the server refusing the account is explained, with no retry', async ({ page }) => {
    await gotoPhase(page, 'refused')

    await expect(page.getByText('Not sharing', { exact: true }).first()).toBeVisible()
    await expect(page.locator('h1')).toHaveText('The server refused this account.')
    await expect(page.getByText('Account is inactive. This Mac stopped sharing compute.')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Retry now' })).toHaveCount(0)
  })

  test('a deactivated account explains who did it and what to do', async ({ page }) => {
    await page.goto('/node?demo=true&phase=serving&account=inactive')

    await expect(page.locator('h1')).toHaveText('An admin deactivated your account in Demo Organization.')
    await expect(page.getByText(/leave the organization in Settings/)).toBeVisible()
  })

  test('serving: widget and hero agree', async ({ page }) => {
    await gotoPhase(page, 'serving')

    // "Sharing" appears twice while ready (widget title + toggle row label).
    await expect(page.getByText('Sharing', { exact: true }).first()).toBeVisible()
    await expect(page.locator('h1')).toHaveText('This Mac is serving the network.')
  })

  test('paused: widget and hero agree', async ({ page }) => {
    await gotoPhase(page, 'paused')

    await expect(page.getByText('Paused', { exact: true })).toBeVisible()
    await expect(page.locator('h1')).toHaveText('Paused. This Mac is idle.')
  })

  test('sharing toggle flips widget, hero and ring together', async ({ page }) => {
    await gotoPhase(page, 'serving')

    await expect(page.locator('h1')).toHaveText('This Mac is serving the network.')

    // The whole status widget is the sharing toggle when the node is ready.
    await page.locator('button[aria-pressed]').click()

    await expect(page.getByText('Paused', { exact: true })).toBeVisible()
    await expect(page.locator('h1')).toHaveText('Paused. This Mac is idle.')

    // Toggling back should restore the serving copy.
    await page.locator('button[aria-pressed]').click()
    await expect(page.locator('h1')).toHaveText('This Mac is serving the network.')
  })
})
