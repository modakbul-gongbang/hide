//! The operator's language and the few sentences the engine composes around
//! a judgment's or a worker's text in it (docs/factory.md, The operator's
//! language). A judgment and a worker are asked to write in it; these join
//! their words with the engine's own, so a line never mixes two languages.

use serde::{Deserialize, Serialize};

use crate::model::RecoveryAction;

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

/// The text without the sentence end it may carry, so the engine's own
/// sentence end is the only one (`… 없음..` was a judgment's period and the
/// engine's).
fn clause(text: &str) -> &str {
    text.trim()
        .trim_end_matches(['.', '。', '．', '!', '！', '?', '？'])
        .trim_end()
}

/// What a closed-list recovery action does and frees, asked as a question.
fn recovery_question(language: Language, action: RecoveryAction) -> &'static str {
    use Language::*;
    use RecoveryAction::*;
    match (language, action) {
        (English, RemoveFinishedWorktrees) => {
            "Remove the worktrees of finished Tasks, and of cancelled Tasks past their keep period, to free disk space?"
        }
        (Korean, RemoveFinishedWorktrees) => {
            "끝난 Task와 보관 기간이 지난 취소 Task의 worktree를 지워 디스크 공간을 확보할까요?"
        }
        (SimplifiedChinese, RemoveFinishedWorktrees) => {
            "删除已完成的 Task 以及超过保留期的已取消 Task 的 worktree，以释放磁盘空间？"
        }
        (Japanese, RemoveFinishedWorktrees) => {
            "完了した Task と保持期間を過ぎた取り消し済み Task の worktree を削除して、ディスク容量を空けますか？"
        }
        (English, RestartWorker) => {
            "Start the workers of stopped Tasks again, in the same worktree and session?"
        }
        (Korean, RestartWorker) => "멈춘 Task의 worker를 같은 worktree와 세션에서 다시 시작할까요?",
        (SimplifiedChinese, RestartWorker) => {
            "在同一 worktree 和会话中重新启动已停止 Task 的 worker？"
        }
        (Japanese, RestartWorker) => {
            "停止した Task の worker を同じ worktree とセッションで再開しますか？"
        }
        (English, SleepWakeWorker) => {
            "Put workers waiting on input to sleep and wake them in the same session to carry on?"
        }
        (Korean, SleepWakeWorker) => {
            "입력을 기다리는 worker를 재웠다가 같은 세션에서 깨워 하던 일을 이어가게 할까요?"
        }
        (SimplifiedChinese, SleepWakeWorker) => {
            "让等待输入的 worker 休眠，再在同一会话中唤醒以继续工作？"
        }
        (Japanese, SleepWakeWorker) => {
            "入力待ちの worker をスリープさせ、同じセッションで起こして作業を続けさせますか？"
        }
        (English, SwitchRuntime) => {
            "Start new Tasks with another worker agent instead of the default one for an hour?"
        }
        (Korean, SwitchRuntime) => {
            "한 시간 동안 새 Task를 기본 agent 대신 다른 worker agent로 시작할까요?"
        }
        (SimplifiedChinese, SwitchRuntime) => {
            "在一小时内改用其他 worker agent 而不是默认 agent 启动新 Task？"
        }
        (Japanese, SwitchRuntime) => {
            "1 時間、新しい Task を既定の agent ではなく別の worker agent で開始しますか？"
        }
        (English, RetryReadsAndReconnect) => {
            "Read GitHub again now and check whether starts may resume?"
        }
        (Korean, RetryReadsAndReconnect) => "GitHub를 지금 다시 읽고 시작 보류를 다시 확인할까요?",
        (SimplifiedChinese, RetryReadsAndReconnect) => {
            "立即重新读取 GitHub，并重新检查是否可以恢复启动？"
        }
        (Japanese, RetryReadsAndReconnect) => {
            "今すぐ GitHub を読み直し、開始の保留を再確認しますか？"
        }
    }
}

/// An environment diagnosis that names a closed-list action that is off: the
/// cause, then the action asked in words a person recognises.
pub fn recovery_proposal(language: Language, cause: &str, action: RecoveryAction) -> String {
    let cause = clause(cause);
    let question = recovery_question(language, action);
    match language {
        Language::English => format!("Environment problem: {cause}. {question}"),
        Language::Korean => format!("환경 문제: {cause}. {question}"),
        Language::SimplifiedChinese => format!("环境问题：{cause}。{question}"),
        Language::Japanese => format!("環境の問題: {cause}。{question}"),
    }
}

/// An environment diagnosis that names a command a person runs themselves.
pub fn command_proposal(language: Language, cause: &str, command: &str, impact: &str) -> String {
    let cause = clause(cause);
    let impact = clause(impact);
    match language {
        Language::English => {
            format!(
                "Environment problem: {cause}. A command to run yourself: {command} (impact: {impact})"
            )
        }
        Language::Korean => {
            format!("환경 문제: {cause}. 직접 실행할 명령: {command} (영향: {impact})")
        }
        Language::SimplifiedChinese => {
            format!("环境问题：{cause}。请自行运行的命令：{command}（影响：{impact}）")
        }
        Language::Japanese => {
            format!("環境の問題: {cause}。自分で実行するコマンド: {command}（影響: {impact}）")
        }
    }
}

/// A watch warning raised as a notice, with the action it proposes.
pub fn watch_notice(language: Language, text: &str, action: &str) -> String {
    let text = text.trim();
    let action = clause(action);
    match language {
        Language::English => format!("Watch: {text} (to do: {action})"),
        Language::Korean => format!("감시: {text} (할 일: {action})"),
        Language::SimplifiedChinese => format!("监视：{text}（待办：{action}）"),
        Language::Japanese => format!("監視: {text}（対応: {action}）"),
    }
}

/// A worker's unrelated discovery raised as a notice.
pub fn unrelated_notice(language: Language, text: &str) -> String {
    let text = text.trim();
    match language {
        Language::English => format!("Unrelated finding: {text}"),
        Language::Korean => format!("무관한 발견: {text}"),
        Language::SimplifiedChinese => format!("无关发现：{text}"),
        Language::Japanese => format!("無関係な発見: {text}"),
    }
}

/// The worker's first-prompt rule naming the language it reports in. The
/// prompt is Korean; the language is named in its own words, so no
/// particle has to follow it.
pub fn report_language_rule(language: Language) -> String {
    format!(
        "- 사람이 읽는 보고(done의 요약, ask와 block의 질문·제안·기본 행동·선택지, propose와 decide의 내용)는 모두 이 언어로 쓰세요: {} ({})\n",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_question_ends_each_sentence_once_and_names_no_action_id() {
        for language in Language::ALL {
            for action in RecoveryAction::ALL {
                let text = recovery_proposal(language, "disk is low (4.7 GB).", action);
                assert!(!text.contains(action.as_str()), "{text}");
                for doubled in ["..", "。。", ".。", "。."] {
                    assert!(!text.contains(doubled), "{text}");
                }
            }
        }
        assert_eq!(
            recovery_proposal(
                Language::Korean,
                "디스크 여유 공간이 약 4.7GB로 기준보다 작아 보류 상태이며, 최근 실패나 중단은 없음.",
                RecoveryAction::RemoveFinishedWorktrees
            ),
            "환경 문제: 디스크 여유 공간이 약 4.7GB로 기준보다 작아 보류 상태이며, 최근 실패나 중단은 없음. 끝난 Task와 보관 기간이 지난 취소 Task의 worktree를 지워 디스크 공간을 확보할까요?"
        );
    }
}
