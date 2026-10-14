import { execFileSync } from "node:child_process";

/**
 * `git init` in `repo` with a remote naming the GitHub repository the specs'
 * fake `gh` programs answer for (`acme/repo`), as a real checkout has one:
 * `hided` answers a checkout whose remotes name no GitHub repository "no
 * GitHub remote" without asking `gh`. The remote is called `github`, so a
 * fixture's own `origin`, a local bare repository it measures branches
 * against, stays as it was.
 */
export function initGithubRepository(repo: string): void {
  execFileSync("git", ["-c", "init.defaultBranch=main", "init"], { cwd: repo, stdio: "ignore" });
  execFileSync("git", ["remote", "add", "github", "https://github.com/acme/repo.git"], { cwd: repo, stdio: "ignore" });
}
