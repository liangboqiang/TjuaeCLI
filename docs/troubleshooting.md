# 故障排查

## 未配置 API 密钥

```text
未找到 API 密钥。请通过 --api-key、配置文件或环境变量（API_KEY、ANTHROPIC_API_KEY 或 OPENAI_API_KEY）提供，或运行 'tjuae-cli auth login'。
```

请通过配置文件、`--api-key` 参数或环境变量提供 API 密钥。

## API 密钥无效

```text
[错误] API 错误 401：...
```

确认 API 密钥正确且仍处于有效状态。

## 找不到配置档

```text
配置中未找到配置档 'xxx'
```

确认配置文件中已经定义该配置档。

## 模型不可用

```text
[错误] API 错误 404：...
```

确认 `--model` 拼写正确，并且当前 API 密钥有权访问该模型。

## 请求过大

```text
[错误] API 错误 413：...
```

对话历史过长。请重新启动智能体，或减小 `--max-turns`。

## 请求受到限流

```text
[错误] 请求受到限流，请在 5000 毫秒后重试
```

API 调用频率过高。智能体会在提示的延迟后自动重试。

## 命令超时

```text
命令在 120000 毫秒后超时
```

ExecCommand 工具执行命令时超过了时限。可通过工具的 `timeout` 参数增大超时时间。

## 未安装 ripgrep

Grep 工具会自动回退到系统 `grep`。若要获得更好的搜索性能，请安装 ripgrep：

```bash
brew install ripgrep  # macOS
sudo apt install ripgrep  # Debian/Ubuntu
```
