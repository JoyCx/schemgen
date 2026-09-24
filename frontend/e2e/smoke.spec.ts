// Upload a fixture model, watch its preview build, convert it, and check the
// result is downloadable — the whole path a user takes, against the real
// server.

import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'

const fixture = (name: string) =>
  fileURLToPath(new URL(`../../backend/fixtures/${name}`, import.meta.url))

test('a model converts end to end', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))

  await page.goto('/')
  await expect(page.locator('.health--ok')).toBeVisible()
  await expect(page.getByRole('form', { name: 'Conversion settings' })).toBeVisible()

  await page.locator('input[type=file]').setInputFiles(fixture('textured.glb'))
  await expect(page.getByRole('button', { name: 'textured.glb', exact: true })).toBeVisible()

  // The live preview comes back with its material list.
  await page.getByRole('tab', { name: 'Blocks' }).click()
  const materials = page.getByRole('region', { name: 'Material list' })
  await expect(materials).toBeVisible({ timeout: 60_000 })
  await expect(materials.getByText('preview', { exact: true })).toBeVisible()

  // A setting the schema drew changes what is sent.
  await page.getByRole('combobox', { name: 'Minecraft version' }).selectOption('1.20.4')

  await page.getByRole('button', { name: /^Convert/ }).click()
  await expect(page.locator('.queue-item--done')).toBeVisible({ timeout: 90_000 })
  await expect(materials.getByText('final', { exact: true })).toBeVisible()
  await expect(page.locator('.queue-detail')).toContainText('Minecraft 1.20.4')

  const download = page.getByRole('link', { name: /Download textured/ })
  await expect(download).toHaveAttribute('href', /\/api\/jobs\/[^/]+\/download/)
  const response = await page.request.get((await download.getAttribute('href'))!)
  expect(response.ok()).toBeTruthy()
  expect((await response.body()).length).toBeGreaterThan(100)

  // Split view and the keyboard shortcuts.
  await page.keyboard.press('3')
  await expect(page.getByRole('separator', { name: 'Model and blocks divider' })).toBeVisible()

  expect(errors).toEqual([])
})
