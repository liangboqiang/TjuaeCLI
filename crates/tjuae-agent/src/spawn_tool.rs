use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::spawner::{AgentSpawner, SubAgentConfig};
use tjuae_protocol::events::ToolCategory;
use tjuae_types::tool::{JsonSchema, ToolResult};

use tjuae_tools::Tool;

const DEFAULT_SUB_AGENT_MAX_TURNS: usize = 200;
const DEFAULT_SUB_AGENT_MAX_TOKENS: u32 = 4096;
const MAX_SUB_AGENTS: usize = 5;

pub struct SpawnTool {
    spawner: Arc<AgentSpawner>,
}

impl SpawnTool {
    pub fn new(spawner: Arc<AgentSpawner>) -> Self {
        Self { spawner }
    }
}

#[async_trait]
impl Tool for SpawnTool {
    fn name(&self) -> &str {
        "Spawn"
    }

    fn description(&self) -> &str {
        "生成一个或多个子智能体并行处理任务。\
         每个子智能体都有独立的对话上下文和工具访问权限。\n\n\
         - 每次最多生成 5 个子智能体。\n\
         - 每个子智能体最多运行 200 个模型轮次，输出上限为 4096 token。\n\
         - 适用于相互独立、可并行的任务（例如搜索不同模块或分别进行分析）。\n\
         - 不要用于需要共享状态或顺序协调的任务。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "description": "由子智能体并行执行的任务列表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "任务的简短描述性名称"
                            },
                            "prompt": {
                                "type": "string",
                                "description": "提供给子智能体的任务说明或提示词"
                            }
                        },
                        "required": ["name", "prompt"]
                    }
                }
            },
            "required": ["tasks"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false // manages its own concurrency
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let tasks = match parse_tasks(&input) {
            Ok(tasks) => tasks,
            Err(e) => {
                return ToolResult {
                    content: e,
                    is_error: true,
                };
            }
        };

        if tasks.is_empty() {
            return ToolResult {
                content: "未提供任务".to_string(),
                is_error: true,
            };
        }

        if tasks.len() > MAX_SUB_AGENTS {
            return ToolResult {
                content: format!("子智能体数量过多：{}（最多 {} 个）", tasks.len(), MAX_SUB_AGENTS),
                is_error: true,
            };
        }

        let results = self.spawner.spawn_parallel(tasks).await;

        let output: Vec<String> = results
            .iter()
            .map(|r| {
                let status = if r.is_error { "错误" } else { "成功" };
                format!(
                    "## {} [{}]\n{}\n[轮次：{} | token：输入 {} / 输出 {}]",
                    r.name, status, r.text, r.turns, r.usage.input_tokens, r.usage.output_tokens
                )
            })
            .collect();

        let all_error = results.iter().all(|r| r.is_error);

        ToolResult {
            content: output.join("\n\n---\n\n"),
            is_error: all_error,
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn describe(&self, input: &Value) -> String {
        let task = input.get("task").and_then(|v| v.as_str()).unwrap_or("子智能体");
        format!("生成子智能体：{}", tjuae_tools::truncate_utf8(task, 80))
    }
}

fn parse_tasks(input: &Value) -> Result<Vec<SubAgentConfig>, String> {
    let tasks_arr = input["tasks"].as_array().ok_or("缺少 'tasks' 数组或其格式无效")?;

    let mut configs = Vec::new();
    for task in tasks_arr {
        let name = task["name"]
            .as_str()
            .ok_or("每个任务都必须包含字符串类型的 'name'")?
            .to_string();
        let prompt = task["prompt"]
            .as_str()
            .ok_or("每个任务都必须包含字符串类型的 'prompt'")?
            .to_string();

        configs.push(SubAgentConfig {
            name,
            prompt,
            max_turns: DEFAULT_SUB_AGENT_MAX_TURNS,
            max_tokens: DEFAULT_SUB_AGENT_MAX_TOKENS,
            system_prompt: None,
        });
    }

    Ok(configs)
}
