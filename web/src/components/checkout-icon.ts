import { FolderIcon, GitBranchIcon, GitCommitHorizontalIcon, HouseIcon } from "lucide-react";
import type { CheckoutKind } from "../projects";

/**
 * The shape each kind of checkout draws when it has no pull request; a
 * pull request draws its PR mark instead (`components/pr-mark.tsx`), so it
 * has one shape and colour wherever it appears.
 */
export const CHECKOUT_KIND_ICON: Record<Exclude<CheckoutKind, "pull_request">, typeof GitBranchIcon> = {
  folder: FolderIcon,
  primary: HouseIcon,
  detached: GitCommitHorizontalIcon,
  branch: GitBranchIcon,
};
