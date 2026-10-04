import type { Catalog, Catalogs } from "../schema";

export const prListEnglish = {
  "prList.takeHint": "Assign to an agent\nOpen the Start dialog on the PR branch. First instruction = failed checks and review comments",
  "prList.linkHint": "Link issue\nLink this PR to an issue. If none is available, create one from the PR title and body",
  "prList.cleanHint": "Clean up\nRemove merged worktrees or those with missing folders, and their resting agents. A confirmation dialog appears first",
  "prList.gitHubHint": "GitHub\nReview and merge",
  "prList.empty": "No open PRs.",
  "prList.openFailedChecks": "View failed checks on GitHub",
  "prList.openChecks": "View checks on GitHub",
  "prList.openIssue": "Open {{issue}} on GitHub",
  "prList.linkIssue": "Link issue",
  "prList.searchIssues": "Search issues",
  "prList.newIssue": "Create new issue",
  "prList.delegate": "Assign",
  "prList.cleanup": "Clean up",
  "prList.actions": "PR actions",
  "prList.copyBranch": "Copy branch name",
} as const;

const ko = {
  "prList.takeHint": "에이전트에게 맡기기\nPR 브랜치에서 시작 대화상자를 연다. 첫 지시 = 실패한 검사 · 리뷰 코멘트",
  "prList.linkHint": "이슈 잇기\n이 PR을 이슈에 잇는다. 이을 이슈가 없으면 PR 제목 · 본문으로 새로 만든다",
  "prList.cleanHint": "정리\n머지됐거나 폴더가 없는 워크트리와 거기서 쉬는 에이전트를 지운다. 확인 대화상자가 먼저 뜬다",
  "prList.gitHubHint": "GitHub\n리뷰하고 머지",
  "prList.empty": "열린 PR이 없습니다.",
  "prList.openFailedChecks": "실패한 검사 GitHub에서 보기",
  "prList.openChecks": "검사 GitHub에서 보기",
  "prList.openIssue": "{{issue}} GitHub에서 열기",
  "prList.linkIssue": "이슈 잇기",
  "prList.searchIssues": "이슈 검색",
  "prList.newIssue": "새 이슈 만들기",
  "prList.delegate": "맡기기",
  "prList.cleanup": "정리",
  "prList.actions": "PR 동작",
  "prList.copyBranch": "브랜치 이름 복사",
} satisfies Catalog<typeof prListEnglish>;

const zhCN = {
  "prList.takeHint": "交给智能体\n在 PR 分支上打开启动对话框。初始指令 = 失败的检查和审查评论",
  "prList.linkHint": "关联议题\n将此 PR 关联到议题。如果没有可关联的议题，则根据 PR 标题和正文创建新议题",
  "prList.cleanHint": "清理\n移除已合并或文件夹缺失的工作树，以及其中空闲的智能体。操作前会显示确认对话框",
  "prList.gitHubHint": "GitHub\n审查并合并",
  "prList.empty": "暂无未关闭的 PR。",
  "prList.openFailedChecks": "在 GitHub 上查看失败的检查",
  "prList.openChecks": "在 GitHub 上查看检查",
  "prList.openIssue": "在 GitHub 上打开 {{issue}}",
  "prList.linkIssue": "关联议题",
  "prList.searchIssues": "搜索议题",
  "prList.newIssue": "创建新议题",
  "prList.delegate": "交给智能体",
  "prList.cleanup": "清理",
  "prList.actions": "PR 操作",
  "prList.copyBranch": "复制分支名称",
} satisfies Catalog<typeof prListEnglish>;

const ja = {
  "prList.takeHint": "エージェントに任せる\nPR のブランチで開始ダイアログを開きます。最初の指示 = 失敗したチェックとレビューコメント",
  "prList.linkHint": "課題を関連付ける\nこの PR を課題に関連付けます。該当する課題がなければ PR のタイトルと本文から作成します",
  "prList.cleanHint": "整理\nマージ済み、またはフォルダーのないワークツリーと、そこで待機中のエージェントを削除します。先に確認ダイアログが表示されます",
  "prList.gitHubHint": "GitHub\nレビューしてマージ",
  "prList.empty": "オープンな PR はありません。",
  "prList.openFailedChecks": "GitHub で失敗したチェックを表示",
  "prList.openChecks": "GitHub でチェックを表示",
  "prList.openIssue": "GitHub で{{issue}}を開く",
  "prList.linkIssue": "課題を関連付ける",
  "prList.searchIssues": "課題を検索",
  "prList.newIssue": "新しい課題を作成",
  "prList.delegate": "任せる",
  "prList.cleanup": "整理",
  "prList.actions": "PR の操作",
  "prList.copyBranch": "ブランチ名をコピー",
} satisfies Catalog<typeof prListEnglish>;

export const prListCatalogs = { en: prListEnglish, ko, "zh-CN": zhCN, ja } satisfies Catalogs<typeof prListEnglish>;
