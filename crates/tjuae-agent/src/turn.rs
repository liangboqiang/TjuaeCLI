use crate::error::AgentError;
use crate::stream::StreamOutcome;
use crate::tool_call::{
    DEFAULT_MAX_ALL_ERROR_TOOL_ROUNDS, DEFAULT_MAX_TOOL_CALL_CYCLE_REPETITIONS, ToolCallAllErrorRoundTracker,
    ToolCallCycle, ToolCallCycleTracker, ToolCallFailureFingerprint, ToolCallFailureTracker,
    ToolCallMalformedFingerprint, ToolCallMalformedTracker,
};
use tjuae_types::message::StopReason;

const EXACT_FAILURE_WARNING_COUNT: usize = 2;
const ALL_ERROR_ROUND_WARNING_COUNT: usize = 3;
const TOOL_CALL_CYCLE_WARNING_REPETITIONS: usize = 2;

pub(crate) enum TurnOutcome {
    ToolRound(StreamOutcome),
    Final(StreamOutcome),
    Truncated(StreamOutcome),
    EmptyFinal(StreamOutcome),
}

impl TurnOutcome {
    pub(crate) fn from_stream(outcome: StreamOutcome) -> Self {
        if !outcome.tool_calls.is_empty() {
            return Self::ToolRound(outcome);
        }

        match outcome.stop_reason {
            StopReason::EndTurn if !outcome.assistant_text.trim().is_empty() => Self::Final(outcome),
            StopReason::MaxTokens => Self::Truncated(outcome),
            _ => Self::EmptyFinal(outcome),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalizationReason {
    TurnBudget,
    ToolFailure,
    MaxTokens,
    EmptyFinal,
}

impl FinalizationReason {
    pub(crate) fn fallback_prompt(self) -> &'static str {
        match self {
            FinalizationReason::TurnBudget => "模型尚未生成最终答案就已达到轮次上限，运行已停止。",
            FinalizationReason::ToolFailure => "工具执行反复失败且没有进展。请查看最近的工具错误，解决阻塞问题后重试。",
            FinalizationReason::MaxTokens => "响应因达到 token 上限而被截断，无法自动完成。",
            FinalizationReason::EmptyFinal => "重试一次后，模型仍未生成可见的回答文本。",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnKind {
    Normal,
    Finalization(FinalizationReason),
}

impl TurnKind {
    pub(crate) fn disable_tools(self) -> bool {
        matches!(self, Self::Finalization(_))
    }

    pub(crate) fn control_prompt(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Finalization(FinalizationReason::TurnBudget) => {
                Some("不要再调用任何工具。请使用已有的工具结果，立即给出最终答案。")
            }
            Self::Finalization(FinalizationReason::ToolFailure) => Some(
                "工具执行反复失败且没有进展。不要再调用任何工具。请总结已完成的工作，结合最近的工具结果说明具体阻塞原因，并指出用户下一步应修改或提供什么。不要提及内部重试计数。",
            ),
            Self::Finalization(FinalizationReason::MaxTokens) => {
                Some("上一次响应因达到 token 上限而被截断。现在请完成回答，不要调用任何工具。")
            }
            Self::Finalization(FinalizationReason::EmptyFinal) => Some(
                "上一次助手响应结束时没有可见的回答文本。现在请给出简洁且可见的答案，不要只发送推理过程，也不要调用任何工具。",
            ),
        }
    }

    pub(crate) fn diagnostic_phase(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Finalization(FinalizationReason::TurnBudget) => "turn_budget_finalization",
            Self::Finalization(FinalizationReason::ToolFailure) => "tool_failure_finalization",
            Self::Finalization(FinalizationReason::MaxTokens) => "max_tokens_finalization",
            Self::Finalization(FinalizationReason::EmptyFinal) => "empty_final_retry",
        }
    }
}

#[derive(Debug)]
pub(crate) struct TurnTracker {
    count: usize,
    limit: Option<usize>,
}

impl TurnTracker {
    pub(crate) fn new(limit: Option<usize>) -> Self {
        Self { count: 0, limit }
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn observe(&mut self) -> usize {
        self.count += 1;
        self.count
    }

    pub(crate) fn limit_reached(&self) -> Option<usize> {
        self.limit.filter(|&limit| self.count >= limit)
    }
}

/// Per-`run` loop-termination bookkeeping: the turn counter plus the
/// tool-call-malformed and tool-call-failure breakers. Keeps the counters and
/// their thresholds out of the loop body so the main loop has a single stop
/// decision: [`TurnGuards::after_tool_round`].
pub(crate) struct TurnGuards {
    /// Number of counted normal model turns so far.
    turns: TurnTracker,
    tool_call_malformed: ToolCallMalformedTracker,
    tool_call_failures: ToolCallFailureTracker,
    all_error_tool_rounds: ToolCallAllErrorRoundTracker,
    tool_call_cycles: ToolCallCycleTracker,
    tool_call_cycle_warning_emitted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolLoopWarning {
    ExactFailure {
        count: usize,
        limit: usize,
    },
    AllErrorRounds {
        count: usize,
        limit: usize,
    },
    Cycle {
        period: usize,
        repetitions: usize,
        limit: usize,
    },
}

impl ToolLoopWarning {
    pub(crate) fn guidance(self) -> String {
        match self {
            Self::ExactFailure { count, limit } => format!(
                "[需要恢复工具调用：同一个工具调用已失败 {count}/{limit} 次。不要原样重复。请检查最近的错误，修改参数或策略，改用其他工具，或在最终答案中说明阻塞原因。]"
            ),
            Self::AllErrorRounds { count, limit } => format!(
                "[需要恢复工具调用：所有工具调用已连续失败 {count}/{limit} 轮。不要机械重试。请诊断最近的错误，选择实质不同的方法，或在最终答案中说明阻塞原因。]"
            ),
            Self::Cycle {
                period,
                repetitions,
                limit,
            } => format!(
                "[需要恢复工具调用：一个 {period} 轮的工具调用循环已重复 {repetitions}/{limit} 次且没有进展。请更改策略打破循环，或在最终答案中说明阻塞原因。]"
            ),
        }
    }
}

pub(crate) enum TurnGuardAction {
    Continue,
    Warn(ToolLoopWarning),
    Finalize(FinalizationReason),
    Stop(AgentError),
}

impl TurnGuards {
    pub(crate) fn new(
        max_turns_per_run: Option<usize>,
        max_tool_call_malformed_turns: usize,
        max_tool_call_failure_turns: usize,
    ) -> Self {
        let tool_failure_guards_enabled = max_tool_call_failure_turns > 0;
        Self {
            turns: TurnTracker::new(max_turns_per_run),
            tool_call_malformed: ToolCallMalformedTracker::new(max_tool_call_malformed_turns),
            tool_call_failures: ToolCallFailureTracker::new(max_tool_call_failure_turns),
            all_error_tool_rounds: ToolCallAllErrorRoundTracker::new(if tool_failure_guards_enabled {
                DEFAULT_MAX_ALL_ERROR_TOOL_ROUNDS
            } else {
                0
            }),
            tool_call_cycles: ToolCallCycleTracker::new(tool_failure_guards_enabled),
            tool_call_cycle_warning_emitted: false,
        }
    }

    pub(crate) fn counted_turns(&self) -> usize {
        self.turns.count()
    }

    /// Returns the configured limit when the turn budget is exhausted, else `None`.
    pub(crate) fn turn_budget_reached(&self) -> Option<usize> {
        self.turns.limit_reached()
    }

    pub(crate) fn record_counted_turn(&mut self) {
        self.turns.observe();
    }

    /// Fold one tool round into the breakers and return the loop action. Must
    /// be called once per tool round, after the results are recorded.
    pub(crate) fn after_tool_round(
        &mut self,
        tool_call_malformed_fingerprint: Option<ToolCallMalformedFingerprint>,
        tool_call_failure_fingerprint: Option<ToolCallFailureFingerprint>,
        all_tool_results_error: bool,
    ) -> TurnGuardAction {
        let malformed_count = self.tool_call_malformed.observe(tool_call_malformed_fingerprint);
        if self.tool_call_malformed.is_limit_exceeded() {
            tracing::warn!(
                target: "tjuae_agent",
                count = malformed_count,
                limit = self.tool_call_malformed.limit(),
                "正在停止格式错误的工具调用循环"
            );
            return TurnGuardAction::Stop(AgentError::ToolCallMalformed {
                count: malformed_count,
                limit: self.tool_call_malformed.limit(),
            });
        }

        let tool_call_failure_count = self.tool_call_failures.observe(tool_call_failure_fingerprint.clone());
        let all_error_round_count = self.all_error_tool_rounds.observe(all_tool_results_error);
        let cycle = self.tool_call_cycles.observe(tool_call_failure_fingerprint);
        if cycle.is_none() {
            self.tool_call_cycle_warning_emitted = false;
        }

        if self.tool_call_failures.is_limit_exceeded() {
            tracing::warn!(
                target: "tjuae_agent",
                count = tool_call_failure_count,
                limit = self.tool_call_failures.limit(),
                loop_kind = "exact_failure",
                "工具调用反复失败，正在收尾"
            );
            return TurnGuardAction::Finalize(FinalizationReason::ToolFailure);
        }

        if let Some(ToolCallCycle { period, repetitions }) = cycle
            && repetitions >= DEFAULT_MAX_TOOL_CALL_CYCLE_REPETITIONS
        {
            tracing::warn!(
                target: "tjuae_agent",
                period,
                repetitions,
                limit = DEFAULT_MAX_TOOL_CALL_CYCLE_REPETITIONS,
                loop_kind = "cycle",
                "工具调用反复循环，正在收尾"
            );
            return TurnGuardAction::Finalize(FinalizationReason::ToolFailure);
        }

        if self.all_error_tool_rounds.is_limit_exceeded() {
            tracing::warn!(
                target: "tjuae_agent",
                count = all_error_round_count,
                limit = self.all_error_tool_rounds.limit(),
                loop_kind = "all_error_rounds",
                "工具轮次连续全部出错，正在收尾"
            );
            return TurnGuardAction::Finalize(FinalizationReason::ToolFailure);
        }

        if self.turn_budget_reached().is_some() {
            return TurnGuardAction::Finalize(FinalizationReason::TurnBudget);
        }

        if self.tool_call_failures.limit() > EXACT_FAILURE_WARNING_COUNT
            && tool_call_failure_count == EXACT_FAILURE_WARNING_COUNT
        {
            return TurnGuardAction::Warn(ToolLoopWarning::ExactFailure {
                count: tool_call_failure_count,
                limit: self.tool_call_failures.limit(),
            });
        }

        if let Some(ToolCallCycle { period, repetitions }) = cycle
            && repetitions == TOOL_CALL_CYCLE_WARNING_REPETITIONS
            && !self.tool_call_cycle_warning_emitted
        {
            self.tool_call_cycle_warning_emitted = true;
            return TurnGuardAction::Warn(ToolLoopWarning::Cycle {
                period,
                repetitions,
                limit: DEFAULT_MAX_TOOL_CALL_CYCLE_REPETITIONS,
            });
        }

        if self.all_error_tool_rounds.limit() > ALL_ERROR_ROUND_WARNING_COUNT
            && all_error_round_count == ALL_ERROR_ROUND_WARNING_COUNT
        {
            return TurnGuardAction::Warn(ToolLoopWarning::AllErrorRounds {
                count: all_error_round_count,
                limit: self.all_error_tool_rounds.limit(),
            });
        }

        TurnGuardAction::Continue
    }

    #[cfg(test)]
    pub(crate) fn tool_call_failure_count(&self) -> usize {
        self.tool_call_failures.count()
    }

    #[cfg(test)]
    pub(crate) fn all_error_tool_round_count(&self) -> usize {
        self.all_error_tool_rounds.count()
    }
}
