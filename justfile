# TjuaeCLI justfile——使用 `just <recipe>` 运行任务
# 通过 `vx just <recipe>` 调用时，vx 会按 vx.toml 固定工具版本；recipe 本身使用
# 标准 cargo 命令，因此也能在普通 Rust 环境中直接运行。
# 此处所有内容均支持跨平台：recipe 主体避免依赖 shell 内置命令和外部 Unix 工具，
# 改用 just 自身的函数，使同一份 justfile 可用于 macOS、Linux 和 Windows。

# 逐行 recipe 使用的跨平台默认 shell。
set shell := ["sh", "-cu"]
set windows-shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]

# CI、vx 环境和普通 Rust 开发环境都应在 PATH 中提供 cargo。
cargo := "cargo"

# Unix `_run` 彩色命令回显使用的粗体青色与重置 ANSI 代码。
# just 的 `style("command")` 只会加粗而不着色，因此在此显式定义。
CYAN := "\u{1b}[1;36m"
NORMAL := "\u{1b}[0m"

# 默认操作：列出全部 recipe
default:
    @just --list

# 先以粗体青色回显命令，再运行命令。所有操作 recipe 都通过这里执行，以集中处理颜色。
# 命令作为一个带引号的完整字符串传入，保留其中的引号（例如 -E 'test(...)'）。
# 按操作系统分别实现：Unix 通过 `printf` 输出原始 ANSI，Windows 使用 pwsh 原生的
# 彩色 `Write-Host`，它在旧版 Windows 控制台中比 ANSI 更可靠。
[unix]
_run cmd:
    @printf '%s\n' "{{ CYAN }}{{ cmd }}{{ NORMAL }}"
    @{{ cmd }}

[windows]
_run cmd:
    @Write-Host "{{ cmd }}" -ForegroundColor Cyan
    @{{ cmd }}

# ── 构建 ───────────────────────────────────────────────────────────────────
build:
    @just _run "{{ cargo }} build --workspace"

build-release:
    @just _run "{{ cargo }} build --workspace --release"

# ── 测试 ───────────────────────────────────────────────────────────────────

# 使用 nextest 运行单元测试和集成测试（default profile——本地开发）
test:
    @just _run "{{ cargo }} nextest run --workspace --profile default"

# 使用 nextest 运行单元测试和集成测试（CI profile——用于 GitHub Actions）
test-ci:
    @just _run "{{ cargo }} nextest run --workspace --profile ci"

# 按名称运行单个测试
test-one NAME:
    @just _run "{{ cargo }} nextest run --workspace -E 'test({{ NAME }})'"

# 显示测试输出（用于本地调试失败测试）
test-verbose:
    @just _run "{{ cargo }} nextest run --workspace --profile default --no-capture"

# ── 端到端测试 ─────────────────────────────────────────────────────────────
# 需要环境变量：ANTHROPIC_API_KEY 和/或 OPENAI_API_KEY
# 使用专用的 e2e nextest profile（顺序执行、长超时、不重试）
test-e2e:
    @just _run "{{ cargo }} nextest run --workspace --profile e2e --test e2e"

test-e2e-anthropic:
    @just _run "{{ cargo }} nextest run -p tjuae-agent --profile e2e --test e2e -E 'test(anthropic)'"

test-e2e-openai:
    @just _run "{{ cargo }} nextest run -p tjuae-agent --profile e2e --test e2e -E 'test(openai)'"

# ── 验收测试（演进功能验证）────────────────────────────────────────────────
# 需要环境变量：OPENAI_API_KEY 和/或 AWS_PROFILE + CLAUDE_CODE_USE_BEDROCK=1
# 复用 e2e nextest profile（顺序执行、长超时、不重试）
test-acceptance:
    @just _run "{{ cargo }} nextest run -p tjuae-agent --profile e2e --test acceptance"

test-acceptance-memory:
    @just _run "{{ cargo }} nextest run -p tjuae-agent --profile e2e --test acceptance -E 'test(memory)'"

test-acceptance-compact:
    @just _run "{{ cargo }} nextest run -p tjuae-agent --profile e2e --test acceptance -E 'test(compact)'"

# ── Lint / 格式化 ──────────────────────────────────────────────────────────
lint:
    @just _run "{{ cargo }} clippy --workspace --all-targets -- -D warnings"

lint-fix:
    @just _run "{{ cargo }} fix --allow-dirty --allow-staged"
    @just _run "{{ cargo }} clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings"

fmt:
    @just _run "{{ cargo }} fmt --all"

fmt-check:
    @just _run "{{ cargo }} fmt --all -- --check"

# ── Workspace-hack（cargo-hakari）──────────────────────────────────────────
hakari-generate:
    @just _run "{{ cargo }} hakari generate"
    @just _run "{{ cargo }} hakari manage-deps --yes"

hakari-verify:
    @just _run "{{ cargo }} hakari generate --diff"
    @just _run "{{ cargo }} hakari manage-deps --dry-run"
    @just _run "{{ cargo }} hakari verify"

# ── 安全检查 ───────────────────────────────────────────────────────────────
audit:
    @just _run "{{ cargo }} audit"

# ── 覆盖率 ─────────────────────────────────────────────────────────────────
coverage:
    @just _run "{{ cargo }} llvm-cov nextest --workspace --profile ci --lcov --output-path lcov.info"

# ── 发布 ───────────────────────────────────────────────────────────────────
# `cargo pkgid` 输出 `...#<version>`；删除 `#` 及其之前的全部内容。
# 不依赖 Windows 中缺少的 `sed`，而是分别使用各 shell 的原生能力。
[unix]
version:
    @{{ cargo }} pkgid -p tjuae-cli | sed 's/.*#//'

[windows]
version:
    @({{ cargo }} pkgid -p tjuae-cli) -replace '.*#'

# ── 清理 ───────────────────────────────────────────────────────────────────
clean:
    @just _run "{{ cargo }} clean"

# ── 推送前门禁（lint-fix、格式化、自动提交修复、测试，然后推送）────────────
push *ARGS: lint-fix fmt _auto-commit-fixes test hakari-verify
    git push {{ ARGS }}

# 自动提交 fmt/clippy 产生的修复。按 shell 分开实现，使 Windows 路径同时兼容
# 系统自带 Windows PowerShell 和 PowerShell 7。
[unix]
_auto-commit-fixes:
    @git add -A
    @git diff --cached --quiet || git commit -m "chore: auto-commit lint/fmt fixes in just push recipe"

[windows]
_auto-commit-fixes:
    @git add -A
    @git diff --cached --quiet; if ($LASTEXITCODE -eq 1) { git commit -m "chore: auto-commit lint/fmt fixes in just push recipe" } elseif ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# ── 全部检查（与 CI 完全一致）──────────────────────────────────────────────
check-all: fmt-check lint test-ci hakari-verify audit

# 本地与 CI 的标准验证门禁。
verify: check-all
