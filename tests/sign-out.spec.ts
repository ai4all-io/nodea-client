import { test, expect } from '@playwright/test'

// Demo mode (src/lib/mockTauri.ts): the mock's `logout` stands in for the Rust
// command, which also tears down the node's connection. Before, sign-out left
// the node serving the previous account's organization from a signed-out Mac.
test.describe('Signing out of the desktop app (demo mode)', () => {
  test('sign-out goes through the logout command, which stops sharing', async ({ page }) => {
    const invoked: string[] = []
    page.on('console', (msg) => {
      const match = msg.text().match(/^\[Mock Tauri\] Invoking command: (\S+)/)
      if (match) invoked.push(match[1])
    })

    await page.goto('/settings?demo=true&phase=serving')
    await page.getByRole('button', { name: 'Sign out' }).click()

    await expect(page).toHaveURL(/\/login/)
    expect(invoked).toContain('logout')
  })

  test('sign-out keeps the Share compute preference for the next sign-in', async ({ page }) => {
    await page.goto('/settings?demo=true&phase=serving')
    await page.getByRole('button', { name: 'Sign out' }).click()

    await expect(page).toHaveURL(/\/login/)
    expect(await page.evaluate(() => localStorage.getItem('nodea.sharing'))).toBe('true')
  })
})
