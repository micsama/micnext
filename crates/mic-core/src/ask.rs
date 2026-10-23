//! `ask_user`：问题就是调用入参，成功即结束本轮，用户下一条消息是回答。

use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

pub const ASK_USER: &str = "ask_user";
/// 与 `ask_user` 同批的其他调用不执行，返回此结果。
pub(crate) const NOT_ALONE: &str = "Not run: ask_user must be called alone.";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskArgs {
    /// 1–4 questions.
    pub questions: Vec<Question>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// The complete question, ending with a question mark.
    pub question: String,
    /// Very short label shown as a chip, at most 12 characters.
    pub header: String,
    /// 2–4 distinct choices; the recommended one first.
    pub options: Vec<QuestionOption>,
    /// Allow choosing more than one option.
    pub multi_select: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    /// 1–5 words.
    pub label: String,
    /// What choosing it means, or its trade-off.
    pub description: String,
}

pub(crate) fn handle() -> ToolHandle {
    ToolHandle::new(AskTool)
}

struct AskTool;

impl Tool for AskTool {
    type Args = AskArgs;

    fn name(&self) -> &str {
        ASK_USER
    }

    fn description(&self) -> &str {
        "Ask the user when you are blocked on a decision only they can make: one the request, the code, and sensible defaults cannot resolve. If there is a conventional choice or a fact you can check, decide and say so instead of asking. Do not ask whether a plan is fine or whether to continue.\n\
         Call it alone, with no other tools in the same step. Your turn ends after asking; the user's next message is the answer.\n\
         Put the recommended option first and end its label with \"(Recommended)\". The user can always answer in their own words, so do not add an \"Other\" option."
    }

    async fn execute(&self, args: AskArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        if !(1..=4).contains(&args.questions.len()) {
            return Err(ToolError::input("questions must have 1–4 items"));
        }
        if let Some(q) = args
            .questions
            .iter()
            .find(|q| !(2..=4).contains(&q.options.len()))
        {
            return Err(ToolError::input(format!(
                "\"{}\" must have 2–4 options",
                q.question
            )));
        }
        Ok(ToolOutput::completed(vec![ContentPart::Text {
            text: "Question posted. Your turn ends here; the user's next message is the answer."
                .into(),
        }]))
    }
}
