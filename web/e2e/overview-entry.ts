// Content scenarios enter the shared Overview through its current scope. From
// a Workspace the Overview row opens it as a layer over the Workspace
// (sidebar-titlebar-refresh D-07), so that is how these helpers reach it; with
// no Workspace in front the row leaves the page as it is.
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
  await openOverviewOn(page, label);
  await expect(page.locator("[data-sidebar-overview]")).toHaveAttribute("aria-current", "page");
}

/** All projects from a Workspace as the operator opens it: the Overview row's layer, on its All projects tab (sidebar-titlebar-refresh D-07). */
export async function openAllProjects(page: Page) {
  await page.locator("[data-sidebar-overview]").click();
  await page.getByRole("tab", { name: "All projects", exact: true }).click();
  await expect(page.locator("[data-main-screen]")).toBeVisible();
}

/** The Overview on the tab of the project in front, which is the Overview the Workspace's toolbar opened before the top row. */
export async function openOverviewOn(page: Page, label: string) {
  await openAllProjects(page);
  await page.getByRole("tab", { name: label, exact: true }).click();
  await expect(page.locator("[data-overview-screen]")).toBeVisible();
}
