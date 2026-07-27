use super::{BundledSkillDefinition, register_bundled_skill};

/// Register the built-in "hello" skill used to validate the bundled skill framework.
pub fn register_hello_skill() {
    register_bundled_skill(BundledSkillDefinition {
        name: "hello",
        description: "用于测试内置技能框架的简单问候技能。",
        content: "你好！我是一个内置技能。今天需要我帮你做什么？\n\n$ARGUMENTS",
        user_invocable: true,
        when_to_use: None,
        argument_hint: None,
        allowed_tools: &[],
        model: None,
        disable_model_invocation: false,
        context: None,
        agent: None,
        files: &[],
    });
}

#[cfg(test)]
#[path = "hello_test.rs"]
mod hello_test;
