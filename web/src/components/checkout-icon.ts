import {
  FolderIcon,
  GitBranchIcon,
  GitCommitHorizontalIcon,
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon,
  HouseIcon,
} from "lucide-react";
import type { CheckoutKind } from "../projects";

/**
 * The shape each kind of checkout draws (docs/UI_BEHAVIOR.md, PR chrome): the
 * sidebar row's glyph and the PR card's badge read it from here, so a pull
 * request has one shape wherever it appears.
 */
export const CHECKOUT_KIND_ICON: Record<CheckoutKind, typeof GitBranchIcon> = {
  pr_open: GitPullRequestIcon,
  pr_draft: GitPullRequestDraftIcon,
  pr_merged: GitMergeIcon,
  pr_closed: GitPullRequestClosedIcon,
  folder: FolderIcon,
  primary: HouseIcon,
  detached: GitCommitHorizontalIcon,
  branch: GitBranchIcon,
};
