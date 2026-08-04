use tracing::info;

/// Log only hook output cardinality, never the hook's raw stdout/stderr.
pub(crate) fn log_hook_output_summary(hook_kind: &'static str, output_count: usize) {
    if output_count == 0 {
        return;
    }

    info!(
        target: "tjuae_agent",
        hook_kind,
        output_count,
        "hook 执行产生了输出"
    );
}

#[cfg(test)]
#[path = "hook_output_test.rs"]
mod hook_output_test;
