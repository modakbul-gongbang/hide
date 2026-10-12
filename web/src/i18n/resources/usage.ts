import type { Catalog, Catalogs } from "../schema";


export const usageEnglish = {
  "settings.tabs.usage": "Usage",
  "usage.title": "Usage",
  "usage.chooseChips": "Choose chips",
  "usage.window.five_hour": "5 hours",
  "usage.window.weekly": "7 days",
  "usage.window.monthly": "Monthly",
  "usage.notStarted": "Not started",
  "usage.expired": "Reset passed",
  "usage.hide": "Hide",
  "usage.show": "Show",
  "usage.hidden": "Hidden",
  "usage.hideAgent": "Hide {{agent}}",
  "usage.showAgent": "Show {{agent}}",
  "usage.pinLimit": "The sidebar shows at most three chips. Switch one off to pick this one.",
  "usage.note": "Only the windows you choose appear in the sidebar. Hidden agents stay out of the popover too.",
  "usage.noLimits": "No limit reading",
  "usage.reason.notBuilt": "Hide doesn’t read {{agent}}’s weekly limit yet.",
  "usage.reason.account": "No limit of its own; it counts toward the account it signs in with.",
  "usage.reason.opencode": "Go plan limits need an OpenCode API key, which Hide doesn’t hold.",
  "usage.reason.cursor": "Not supported: its limits need Cursor’s stored sign-in.",
  "usage.signedOut": "Signed out · run {{command}} to sign in",
  "usage.notInstalled": "{{command}} is not installed"
} as const;

const ko: Catalog<typeof usageEnglish> = {
  "settings.tabs.usage": "사용량",
  "usage.title": "사용량",
  "usage.chooseChips": "칩 선택",
  "usage.window.five_hour": "5시간",
  "usage.window.weekly": "7일",
  "usage.window.monthly": "월간",
  "usage.notStarted": "시작 전",
  "usage.expired": "리셋 지남",
  "usage.hide": "숨기기",
  "usage.show": "표시",
  "usage.hidden": "숨김",
  "usage.hideAgent": "{{agent}} 숨기기",
  "usage.showAgent": "{{agent}} 표시",
  "usage.pinLimit": "사이드바에는 칩을 세 개까지 표시할 수 있어요. 다른 칩을 하나 끄면 선택할 수 있어요.",
  "usage.note": "선택한 창만 사이드바에 표시됩니다. 숨긴 에이전트는 팝오버에서도 보이지 않습니다.",
  "usage.noLimits": "한도 읽기 미지원",
  "usage.reason.notBuilt": "Hide는 아직 {{agent}}의 주간 한도를 읽지 않습니다.",
  "usage.reason.account": "자체 한도가 없으며 로그인한 계정의 사용량에 합산됩니다.",
  "usage.reason.opencode": "Go 플랜 한도에는 OpenCode API 키가 필요하며, Hide는 이 키를 보관하지 않습니다.",
  "usage.reason.cursor": "미지원: 한도를 읽으려면 Cursor에 저장된 로그인 정보가 필요합니다.",
  "usage.signedOut": "로그아웃됨 · {{command}}를 실행해 로그인하세요",
  "usage.notInstalled": "{{command}}가 설치되어 있지 않습니다"
};

const ja: Catalog<typeof usageEnglish> = {
  "settings.tabs.usage": "使用量",
  "usage.title": "使用量",
  "usage.chooseChips": "チップを選択",
  "usage.window.five_hour": "5時間",
  "usage.window.weekly": "7日間",
  "usage.window.monthly": "月間",
  "usage.notStarted": "開始前",
  "usage.expired": "リセット済み",
  "usage.hide": "非表示",
  "usage.show": "表示",
  "usage.hidden": "非表示",
  "usage.hideAgent": "{{agent}}を非表示",
  "usage.showAgent": "{{agent}}を表示",
  "usage.pinLimit": "サイドバーに表示できるチップは3つまでです。ほかのチップをオフにすると選択できます。",
  "usage.note": "選択した期間だけをサイドバーに表示します。非表示のエージェントはポップオーバーにも表示されません。",
  "usage.noLimits": "上限の読み取り未対応",
  "usage.reason.notBuilt": "Hideはまだ{{agent}}の週間上限を読み取れません。",
  "usage.reason.account": "独自の上限はなく、ログインしたアカウントの使用量に加算されます。",
  "usage.reason.opencode": "Goプランの上限にはOpenCode APIキーが必要ですが、Hideは保持しません。",
  "usage.reason.cursor": "未対応：上限の読み取りにはCursorに保存されたログイン情報が必要です。",
  "usage.signedOut": "ログアウト中 · {{command}}を実行してログインしてください",
  "usage.notInstalled": "{{command}}がインストールされていません"
};

const zhCN: Catalog<typeof usageEnglish> = {
  "settings.tabs.usage": "用量",
  "usage.title": "用量",
  "usage.chooseChips": "选择芯片",
  "usage.window.five_hour": "5小时",
  "usage.window.weekly": "7天",
  "usage.window.monthly": "每月",
  "usage.notStarted": "尚未开始",
  "usage.expired": "已重置",
  "usage.hide": "隐藏",
  "usage.show": "显示",
  "usage.hidden": "已隐藏",
  "usage.hideAgent": "隐藏{{agent}}",
  "usage.showAgent": "显示{{agent}}",
  "usage.pinLimit": "侧边栏最多显示三个芯片。关闭一个后即可选择此项。",
  "usage.note": "侧边栏只显示所选时段。隐藏的智能体也不会出现在弹出窗口中。",
  "usage.noLimits": "不支持读取限额",
  "usage.reason.notBuilt": "Hide尚未读取{{agent}}的每周限额。",
  "usage.reason.account": "没有独立限额，用量计入登录的账户。",
  "usage.reason.opencode": "Go套餐限额需要OpenCode API密钥，Hide不持有该密钥。",
  "usage.reason.cursor": "不支持：读取限额需要Cursor保存的登录信息。",
  "usage.signedOut": "已退出登录 · 运行{{command}}登录",
  "usage.notInstalled": "未安装{{command}}"
};

export const usageCatalogs = { en: usageEnglish, ko, ja, "zh-CN": zhCN } satisfies Catalogs<typeof usageEnglish>;
