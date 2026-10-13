/** GitHub repository spelling is case-insensitive; the host, path kind and number stay exact. */
export function githubPullRequestIdentity(url: string | null | undefined): string | null {
  const parts = url?.match(/^https:\/\/github\.com\/([A-Za-z0-9_.-]+)\/([A-Za-z0-9_.-]+)\/pull\/([1-9][0-9]*)$/);
  return parts ? `${parts[1].toLowerCase()}/${parts[2].toLowerCase()}#${parts[3]}` : null;
}

export function sameGithubPullRequest(left: string | null | undefined, right: string | null | undefined): boolean {
  const identity = githubPullRequestIdentity(left);
  return identity !== null && identity === githubPullRequestIdentity(right);
}
