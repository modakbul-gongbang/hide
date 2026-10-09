//! The operator's language and the questions the engine itself asks in it
//! (docs/factory.md, The operator's language). A judgment and a worker are
//! asked to write in it; the engine's own questions are composed here, so a
//! 결정 필요 item never mixes two languages.

use serde::{Deserialize, Serialize};

/// The language Hide's interface is in: the core's explicit choice, else the
/// machine's primary language, else English (docs/LOCALIZATION.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "ja")]
    Japanese,
}

impl Language {
    pub const ALL: [Self; 4] = [
        Self::English,
        Self::Korean,
        Self::SimplifiedChinese,
        Self::Japanese,
    ];

    /// The tag the interface catalogs use.
    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Korean => "ko",
            Self::SimplifiedChinese => "zh-CN",
            Self::Japanese => "ja",
        }
    }

    /// The language's English name, as a judgment's instructions say it.
    pub fn english_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Korean => "Korean",
            Self::SimplifiedChinese => "Simplified Chinese",
            Self::Japanese => "Japanese",
        }
    }

    /// The language's own name, as the worker's prompt says it.
    pub fn own_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Korean => "한국어",
            Self::SimplifiedChinese => "简体中文",
            Self::Japanese => "日本語",
        }
    }
}

/// Why a main recovery stopped for a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryStop {
    /// The daemon restarted while the recovery ran.
    Restarted,
    /// No single merge could be named as the cause.
    NoCause,
    /// The merge to revert was not found.
    NoMerge,
    /// The revert could not be made.
    RevertNotMade,
    /// The revert could not be merged.
    RevertNotMerged,
    /// The revert's verification failed.
    RevertRed,
}

/// The question a person reads when the engine itself asks (D-33, D-37):
/// one sentence in the operator's language, without ids, command names or
/// paths; what each choice does is the screen's to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked<'a> {
    /// The intake review could not run; `disabled` when Hide AI is off.
    ReviewFailed { disabled: bool },
    /// Verification failed as many times as the limit allows.
    VerifyFailed { failures: u32 },
    /// The same environment failure keeps coming back for this Task only.
    EnvironmentRepeated,
    /// The Task reached its new-Task limit.
    NewTaskCap { limit: u32 },
    /// A proposal claimed an autonomy scope the review did not see it fit.
    OutsideScope { scope: &'a str },
    /// The review proposed splitting the Task.
    Split { pieces: usize },
    /// A person edited the issue body of a Task already running.
    IssueEdited,
    /// The producer sent a changed card or PRD for a Task already running.
    CardChanged,
    /// A worker asked to widen its scope.
    ScopeChange { text: &'a str },
    /// A worker proposed a prerequisite Task.
    ProposedTask { title: &'a str },
    /// An outside push broke main; the drafted fix Task waits for a person.
    MainFix,
    /// A main recovery stopped and a person picks the next step.
    MainRecovery { why: RecoveryStop },
}

pub fn asked(language: Language, question: Asked<'_>) -> String {
    use Language::*;
    match question {
        Asked::ReviewFailed { disabled: true } => match language {
            English => "The intake review could not run because Hide AI is off. Turn it on to review the card, or start from the issue as written.".into(),
            Korean => "Hide AI가 꺼져 있어 접수 리뷰를 하지 못했습니다. 켜서 카드를 검토할지, 이슈 그대로 시작할지 골라 주세요.".into(),
            SimplifiedChinese => "Hide AI 已关闭，无法进行接收审查。请选择开启后审查卡片，或按 issue 原文开始。".into(),
            Japanese => "Hide AI がオフのため受付レビューができませんでした。オンにしてカードを確認するか、issue のまま始めるかを選んでください。".into(),
        },
        Asked::ReviewFailed { disabled: false } => match language {
            English => "The intake review could not run. Fix the AI provider in Settings and review again, or start from the issue as written.".into(),
            Korean => "접수 리뷰를 하지 못했습니다. 설정에서 AI 제공자를 고친 뒤 다시 검토할지, 이슈 그대로 시작할지 골라 주세요.".into(),
            SimplifiedChinese => "接收审查未能运行。请在设置中修复 AI 提供方后重新审查，或按 issue 原文开始。".into(),
            Japanese => "受付レビューができませんでした。設定で AI プロバイダーを直して再レビューするか、issue のまま始めるかを選んでください。".into(),
        },
        Asked::VerifyFailed { failures } => match language {
            English => format!("Verification failed {failures} times. Try again, or stop this Task?"),
            Korean => format!("검증이 {failures}번 실패했습니다. 다시 시도할까요, 이 Task를 멈출까요?"),
            SimplifiedChinese => format!("验证已失败 {failures} 次。要重试，还是停止这个 Task？"),
            Japanese => format!("検証が {failures} 回失敗しました。もう一度試しますか、この Task を止めますか？"),
        },
        Asked::EnvironmentRepeated => match language {
            English => "The same environment failure came back three times for this Task only. Try again, or stop it?".into(),
            Korean => "같은 환경 실패가 이 Task에서만 세 번 반복되었습니다. 다시 시도할까요, 멈출까요?".into(),
            SimplifiedChinese => "同一环境故障仅在这个 Task 上重复了三次。要重试，还是停止？".into(),
            Japanese => "同じ環境の失敗がこの Task だけで 3 回繰り返されました。もう一度試しますか、止めますか？".into(),
        },
        Asked::NewTaskCap { limit } => match language {
            English => format!("This Task has proposed {limit} new Tasks. Split it, let it continue, or stop it?"),
            Korean => format!("이 Task가 새 Task를 {limit}개 만들었습니다. 쪼갤까요, 계속할까요, 멈출까요?"),
            SimplifiedChinese => format!("这个 Task 已提出 {limit} 个新 Task。要拆分、继续，还是停止？"),
            Japanese => format!("この Task は新しい Task を {limit} 個作りました。分割しますか、続けますか、止めますか？"),
        },
        Asked::OutsideScope { scope } => match language {
            English => format!("This Task was proposed to start on its own as \"{scope}\", but the review did not find it fits. Start it anyway?"),
            Korean => format!("이 Task는 '{scope}' 범위로 혼자 시작하도록 제안됐지만 리뷰가 범위에 맞다고 보지 않았습니다. 그래도 시작할까요?"),
            SimplifiedChinese => format!("这个 Task 被提议以“{scope}”范围自行开始，但审查认为不符合。仍要开始吗？"),
            Japanese => format!("この Task は「{scope}」の範囲で自動で始めるよう提案されましたが、レビューは範囲に合うと見ませんでした。それでも始めますか？"),
        },
        Asked::Split { pieces } => match language {
            English => format!("Split this Task into {pieces} pieces?"),
            Korean => format!("이 Task를 {pieces}개로 쪼갤까요?"),
            SimplifiedChinese => format!("将这个 Task 拆分为 {pieces} 个？"),
            Japanese => format!("この Task を {pieces} 個に分けますか？"),
        },
        Asked::IssueEdited => match language {
            English => "Someone edited the issue while the work runs. Take the new scope?".into(),
            Korean => "작업 중에 누군가 이슈 본문을 고쳤습니다. 새 범위로 바꿀까요?".into(),
            SimplifiedChinese => "工作进行中有人修改了 issue 正文。要采用新的范围吗？".into(),
            Japanese => "作業中に誰かが issue 本文を変更しました。新しい範囲にしますか？".into(),
        },
        Asked::CardChanged => match language {
            English => "The card or its PRD changed while the work runs. Take the new scope?".into(),
            Korean => "작업 중에 카드나 PRD가 바뀌었습니다. 새 범위로 바꿀까요?".into(),
            SimplifiedChinese => "工作进行中卡片或 PRD 发生了变化。要采用新的范围吗？".into(),
            Japanese => "作業中にカードか PRD が変わりました。新しい範囲にしますか？".into(),
        },
        Asked::ScopeChange { text } => {
            let text = text.trim();
            match language {
                English => format!("The worker asks to widen the scope: {text}"),
                Korean => format!("작업자가 범위를 넓히자고 합니다: {text}"),
                SimplifiedChinese => format!("工作者请求扩大范围：{text}"),
                Japanese => format!("作業者が範囲を広げたいと言っています: {text}"),
            }
        }
        Asked::ProposedTask { title } => match language {
            English => format!("The worker proposes a new Task it needs first: {title}"),
            Korean => format!("작업자가 먼저 필요한 새 Task를 제안합니다: {title}"),
            SimplifiedChinese => format!("工作者提议先做一个新 Task：{title}"),
            Japanese => format!("作業者が先に必要な新しい Task を提案しています: {title}"),
        },
        Asked::MainFix => match language {
            English => "A push outside the Factory broke main, so automatic merges stopped. Start the drafted fix Task?".into(),
            Korean => "Factory 밖의 push로 main이 깨져 자동 머지를 멈췄습니다. 초안으로 만든 수정 Task를 시작할까요?".into(),
            SimplifiedChinese => "Factory 之外的 push 破坏了 main，自动合并已停止。要开始草拟的修复 Task 吗？".into(),
            Japanese => "Factory 外の push で main が壊れたため、自動マージを止めました。下書きした修正 Task を始めますか？".into(),
        },
        Asked::MainRecovery { why } => {
            let why = match (language, why) {
                (English, RecoveryStop::Restarted) => "the daemon restarted during it",
                (English, RecoveryStop::NoCause) => "no single merge could be named as the cause",
                (English, RecoveryStop::NoMerge) => "the merge to revert was not found",
                (English, RecoveryStop::RevertNotMade) => "the revert could not be made",
                (English, RecoveryStop::RevertNotMerged) => "the revert could not be merged",
                (English, RecoveryStop::RevertRed) => "the revert failed its verification",
                (Korean, RecoveryStop::Restarted) => "도중에 Hide가 다시 시작했습니다",
                (Korean, RecoveryStop::NoCause) => "원인 머지를 하나로 정하지 못했습니다",
                (Korean, RecoveryStop::NoMerge) => "되돌릴 머지를 찾지 못했습니다",
                (Korean, RecoveryStop::RevertNotMade) => "되돌리기를 만들지 못했습니다",
                (Korean, RecoveryStop::RevertNotMerged) => "되돌리기를 머지하지 못했습니다",
                (Korean, RecoveryStop::RevertRed) => "되돌리기의 검증이 실패했습니다",
                (SimplifiedChinese, RecoveryStop::Restarted) => "过程中 Hide 重新启动了",
                (SimplifiedChinese, RecoveryStop::NoCause) => "无法确定是哪一次合并导致的",
                (SimplifiedChinese, RecoveryStop::NoMerge) => "找不到要还原的合并",
                (SimplifiedChinese, RecoveryStop::RevertNotMade) => "无法创建还原",
                (SimplifiedChinese, RecoveryStop::RevertNotMerged) => "无法合并还原",
                (SimplifiedChinese, RecoveryStop::RevertRed) => "还原的验证失败了",
                (Japanese, RecoveryStop::Restarted) => "途中で Hide が再起動しました",
                (Japanese, RecoveryStop::NoCause) => "原因のマージを一つに絞れませんでした",
                (Japanese, RecoveryStop::NoMerge) => "戻すマージが見つかりませんでした",
                (Japanese, RecoveryStop::RevertNotMade) => "取り消しを作れませんでした",
                (Japanese, RecoveryStop::RevertNotMerged) => "取り消しをマージできませんでした",
                (Japanese, RecoveryStop::RevertRed) => "取り消しの検証が失敗しました",
            };
            match language {
                English => format!("Fixing the broken main stopped: {why}. Which step next?"),
                Korean => format!("깨진 main 복구를 멈췄습니다: {why}. 다음에 무엇을 할까요?"),
                SimplifiedChinese => format!("修复损坏的 main 已停止：{why}。下一步做什么？"),
                Japanese => format!("壊れた main の復旧を止めました: {why}。次に何をしますか？"),
            }
        }
    }
}

/// The worker's first-prompt rule naming the language it reports in. The
/// prompt is Korean; the language is named in its own words, so no
/// particle has to follow it.
pub fn report_language_rule(language: Language) -> String {
    format!(
        "- 사람이 읽는 보고(done의 결과·바뀐 것·확인한 것·확인 못 한 것, ask와 block의 질문·제안·기본 행동·선택지, propose와 decide의 내용)는 모두 이 언어로 쓰세요: {} ({})\n",
        language.own_name(),
        language.tag()
    )
}

/// The instruction every judgment ends with: what a person reads is in the
/// operator's language whatever language its input is in.
pub fn judgment_language_rule(language: Language) -> String {
    format!(
        " Write every text a person reads (questions, suggestions, default actions, choices, flags, summaries, warnings, actions, causes, impacts, answers, reasons and card text) in {}, whatever language the input is in; keep code, commands, paths, identifiers and quoted names as they are.",
        language.english_name()
    )
}

/// What the engine tells a person in the Task's activity, in their language.
#[derive(Clone, Copy, Debug)]
pub enum Noted<'a> {
    /// A verification's answer could not be read three times in a row.
    VerifyUnread { check: &'a str, detail: &'a str },
}

pub fn noted(language: Language, note: Noted<'_>) -> String {
    use Language::*;
    match note {
        Noted::VerifyUnread { check, detail } => match language {
            English => format!(
                "The verification result could not be read, so verification has not finished ({check}): {detail}. It continues as soon as it can be read."
            ),
            Korean => format!(
                "검증 결과를 읽지 못해 검증이 끝나지 않습니다 ({check}): {detail}. 읽히는 대로 이어집니다."
            ),
            SimplifiedChinese => {
                format!("无法读取验证结果，验证尚未结束（{check}）：{detail}。一旦能读取就会继续。")
            }
            Japanese => format!(
                "検証結果を読めないため検証が終わっていません（{check}）: {detail}。読めしだい続けます。"
            ),
        },
    }
}

/// A decision Factory AI made that the engine records in its own words,
/// shown on the Task page as Factory AI's (B36).
#[derive(Clone, Copy, Debug)]
pub enum Decided<'a> {
    /// Factory AI's fix for a wrong card was applied.
    CardFixed { title: &'a str },
    /// Factory AI made a new Task from the request.
    NewTask { id: &'a str, title: &'a str },
    /// Factory AI approved merging a change to a risk path.
    RiskMerge,
}

pub fn decided(language: Language, decision: Decided<'_>) -> String {
    use Language::*;
    match decision {
        Decided::CardFixed { title } => match language {
            English => format!("Card fixed: {title}"),
            Korean => format!("카드 고침: {title}"),
            SimplifiedChinese => format!("已修改卡片：{title}"),
            Japanese => format!("カードを修正: {title}"),
        },
        Decided::NewTask { id, title } => match language {
            English => format!("New Task {id}: {title}"),
            Korean => format!("새 Task {id}: {title}"),
            SimplifiedChinese => format!("新 Task {id}：{title}"),
            Japanese => format!("新しい Task {id}: {title}"),
        },
        Decided::RiskMerge => match language {
            English => "Approved merging a change to a risk path".into(),
            Korean => "위험 경로 변경의 머지를 승인함".into(),
            SimplifiedChinese => "已批准合并涉及风险路径的更改".into(),
            Japanese => "リスクのあるパスの変更のマージを承認".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A question the engine asks names no command, id or path, and ends
    /// each sentence once, in every language (D-33).
    #[test]
    fn an_engine_question_names_no_command_and_ends_each_sentence_once() {
        let questions = [
            Asked::ReviewFailed { disabled: true },
            Asked::ReviewFailed { disabled: false },
            Asked::VerifyFailed { failures: 3 },
            Asked::EnvironmentRepeated,
            Asked::NewTaskCap { limit: 3 },
            Asked::OutsideScope {
                scope: "flaky tests",
            },
            Asked::Split { pieces: 2 },
            Asked::IssueEdited,
            Asked::CardChanged,
            Asked::MainFix,
            Asked::MainRecovery {
                why: RecoveryStop::RevertRed,
            },
        ];
        for language in Language::ALL {
            for question in questions.clone() {
                let text = asked(language, question);
                for forbidden in ["hide ", "retry", "T-", "/", "--"] {
                    assert!(!text.contains(forbidden), "{text}");
                }
                for doubled in ["..", "。。", "??", "？？"] {
                    assert!(!text.contains(doubled), "{text}");
                }
            }
        }
    }
}
