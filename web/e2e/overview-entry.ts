// Content scenarios enter the shared Overview page through its current scope.
// Modal lifetime, focus and terminal preservation belong to overview-modal.spec.
import { expect, type Page } from "@playwright/test";
import { showProjectRow } from "./wire";

export async function openProjectOverview(page: Page, label: string) {
  await (await showProjectRow(page, page.locator("[data-project-row]", { hasText: new RegExp(`^${label}`) }))).click();
  await openCurrentProjectOverview(page, label);
}

/** #350 opens a Workspace first; #349 exposes its scope through shared Overview. */
export async function openCurrentProjectOverview(page: Page, label: string) {
  await expect(page.locator("[data-workspace-screen]")).toBeVisible();
  await expect(page.locator("[data-overview-screen]")).toHaveCount(0);
  await expect(page.locator("[data-project-overview]")).toHaveCount(0);
  await openProjectPage(page, label);
  await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
}

/** All projects from a Workspace as the operator opens it: the Overview row's layer, on its All projects tab (sidebar-titlebar-refresh D-07). */
export async function openAllProjects(page: Page) {
  await page.locator("[data-sidebar-overview]").click();
  await page.getByRole("tab", { name: "All projects", exact: true }).click();
  await expect(page.locator("[data-main-screen]")).toBeVisible();
}

/**
 * All projects as a page, with no Workspace in front: from a Workspace the
 * Overview row opens it as a layer, so the way to the page goes through the
 * Factory page, where the Overview row opens the page.
 */
export async function openAllProjectsPage(page: Page) {
  await page.locator("[data-sidebar-factory]").click();
  await expect(page.locator("[data-factory-screen]")).toBeVisible();
  await page.locator("[data-sidebar-overview]").click();
  await expect(page.locator("[data-overview-page]")).toBeVisible();
  await expect(page.locator("[data-overview-modal]")).toHaveCount(0);
  // The page keeps the scope it last showed, so All projects is chosen.
  await page.getByRole("tab", { name: "All projects", exact: true }).click();
  await expect(page.locator("[data-main-screen]")).toBeVisible();
}

/** The Overview page on the project in front's tab, which is the page the Workspace's toolbar opened before the top row. */
export async function openProjectPage(page: Page, label: string) {
  await openAllProjectsPage(page);
  await page.getByRole("tab", { name: label, exact: true }).click();
  await expect(page.locator("[data-overview-screen]")).toBeVisible();
}
