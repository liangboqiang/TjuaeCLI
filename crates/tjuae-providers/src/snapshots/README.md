# 请求体基准快照

这里保存两种线路协议（OpenAI 兼容格式和 Anthropic Messages 格式）在当前所有行为
分支中的出站 HTTP 请求体基准。

**用途：** 为 `ProviderCompat` 子结构拆分以及
`TransportClient`/`RequestProjector`/`ResponseParser` 边界提取提供回归保护。重构时
对这些 `.snap` 文件的任何变更都必须是有意为之，并通过 `cargo insta review`
审查；意外差异表示重构改变了线路输出。

**场景：** 覆盖矩阵见
`docs/superpowers/plans/2026-06-25-golden-body-snapshot-baseline.md`，包含横跨
OpenAI、Anthropic、Bedrock 和 Vertex 的 13 个场景。

**更新方法：** 确实需要变更线路输出时，运行 `cargo insta review`，逐项检查差异，
只接受符合预期的修改，并将更新后的 `.snap` 与代码变更一同提交。
