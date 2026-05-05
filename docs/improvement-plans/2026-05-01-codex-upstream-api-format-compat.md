# Codex 上游 API 格式兼容实现计划

> **给执行型 agent 的说明：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 按任务逐项落实本计划。步骤使用复选框语法（`- [ ]`）追踪进度。

**目标：** 让 Codex 能通过 cc-switch 使用 DeepSeek、Kimi、GLM 等提供商；方式是将每个提供商的上游 API 格式做成可配置，并在需要时桥接 OpenAI Responses API 与 OpenAI Chat Completions API。

**架构：** Codex 客户端继续通过 Responses 兼容端点访问本地 cc-switch 代理。每个提供商在元数据中保存上游协议设置（`apiFormat`）。代理比较客户端协议与提供商上游协议，重写上游端点、转换请求体，并将上游响应再转换回客户端协议。

**技术栈：** Rust/Tauri 后端（`src-tauri`）、React/TypeScript GUI（`src` 与 `cc-switch-main`）、Vitest 前端测试、Rust 单元/集成测试。

---

## 摘要

- Codex GUI 应允许选择 DeepSeek、Kimi、GLM 和自定义提供商。
- 上游 API 格式必须可配置，不能写死。
- 预设提供合理默认值，高级设置支持手动覆盖。
- 首版实现覆盖：文本、工具调用、工具结果、流式响应、usage 映射。
- 多模态输入与完整 reasoning-summary 保真度不在首版范围内。

## 协议判定规则

```text
needs_conversion = client_api_format != provider.meta.apiFormat
```

对 Codex：

```text
Codex 客户端格式：openai_responses
提供商上游格式：openai_responses -> 直通
提供商上游格式：openai_chat      -> Responses <-> Chat 桥接
```

对于使用 Responses-only GPT 上游的 Chat Completions 客户端：

```text
客户端格式：openai_chat
提供商上游格式：openai_responses -> Chat <-> Responses 桥接
```

## 默认值

| 提供商 | 默认上游格式 |
| --- | --- |
| OpenAI 官方 | `openai_responses` |
| Azure OpenAI Responses | `openai_responses` |
| Codex OAuth / ChatGPT Codex 后端 | `openai_responses` |
| DeepSeek | `openai_chat` |
| 智谱 GLM / Z.ai | `openai_chat` |
| Kimi / Moonshot OpenAI 兼容 | `openai_chat` |
| NewAPI / OneAPI / 自定义网关 | `openai_chat`，用户可切换为 `openai_responses` |
| 现有无元数据的 Codex 提供商 | `openai_responses`（向后兼容） |

## 需修改文件

- `src-tauri/src/provider.rs`：按需扩展 Codex model/universal provider 数据结构，并将 `ProviderMeta.api_format` 注释为 Claude/Codex 共用。
- `src-tauri/src/proxy/forwarder.rs`：为 Codex 解析上游格式、重写端点，并返回响应转换所需上下文。
- `src-tauri/src/proxy/handlers.rs`：让 Codex 响应处理走协议感知转换路径。
- `src-tauri/src/proxy/response_processor.rs`：在返回客户端前、以及需要时在 usage 记录前支持响应体转换。
- `src-tauri/src/proxy/providers/transform_responses_chat.rs`：新增专用模块，处理 Responses <-> Chat 转换。
- `src-tauri/src/proxy/providers/mod.rs`：导出新转换模块。
- `src/config/codexProviderPresets.ts`：新增 DeepSeek/Kimi/GLM 预设和上游格式默认值。
- `src/types.ts`：在前端 metadata/model config 中暴露 Codex 上游 API 格式类型。
- `src/components/providers/forms/*`：新增 Codex 高级上游 API 格式选择器并持久化。
- `src/components/universal/UniversalProviderFormModal.tsx`：支持 Universal Provider 的 Codex 上游格式选择。
- `cc-switch-main/src/config/universalProviderPresets.ts`：在合适预设中增加 Codex 上游格式默认值。
- `tests/**/*.test.ts` 和 `src-tauri/src/** #[cfg(test)]`：补充转换与 GUI 持久化覆盖。

## 任务 1：定义共享上游 API 格式

**文件：**

- 修改：`src-tauri/src/provider.rs`
- 修改：`src/types.ts`
- 测试：`tests/utils/providerMetaUtils.test.ts`

- [ ] 新增或明确共享 API 格式类型，包含以下字符串值：

```text
openai_chat
openai_responses
anthropic
gemini_native
```

- [ ] 序列化字段名保持不变：前端为 `apiFormat`，Rust 为 `api_format`。
- [ ] 更新注释，`apiFormat` 不再描述为仅 Claude 使用，应表示“提供商上游 API 格式”。
- [ ] 保持向后兼容：缺失 `apiFormat` 不得影响现有提供商。
- [ ] 运行前端类型检查：

```powershell
pnpm typecheck
```

期望：TypeScript 无报错完成。

## 任务 2：在预设中加入 Codex 上游格式默认值

**文件：**

- 修改：`src/config/codexProviderPresets.ts`
- 修改：`cc-switch-main/src/config/universalProviderPresets.ts`
- 测试：`tests/config/therouterProviderPresets.test.ts` 或新增 `tests/config/codexProviderPresets.apiFormat.test.ts`

- [ ] 在预设元数据中增加上游 API 格式：

```ts
meta: {
  apiFormat: "openai_chat",
}
```

- [ ] 增加 Codex 预设：DeepSeek、智谱 GLM、Kimi/Moonshot OpenAI 兼容端点，且 `apiFormat: "openai_chat"`。
- [ ] OpenAI/Azure/Responses 能力预设保持 `apiFormat: "openai_responses"`。
- [ ] 对 Universal Provider 的 Codex 默认值，增加上游 API 格式字段；通用网关默认 `openai_chat`。
- [ ] 运行预设测试：

```powershell
pnpm test:unit -- tests/config/therouterProviderPresets.test.ts
```

期望：现有测试通过；新增断言确认 DeepSeek/GLM/Kimi 默认 `openai_chat`。

## 任务 3：新增 Codex GUI 选择器

**文件：**

- 修改：`src/components/providers/forms/ProviderForm.tsx`
- 修改：`src/components/providers/forms/hooks/useCodexConfigState.ts`
- 修改：`src/components/providers/forms/*Codex*`（若有专用 Codex 字段组件）
- 修改：`src/i18n/locales/zh.json`
- 修改：`src/i18n/locales/en.json`
- 修改：`src/i18n/locales/ja.json`
- 测试：新增或更新 `tests/components` / `tests/hooks` 下相关 provider form 测试

- [ ] 增加 Codex 高级选项标签：`上游 API 格式` / `Upstream API Format`。
- [ ] 提供两个选项：

```text
OpenAI Chat Completions -> openai_chat
OpenAI Responses        -> openai_responses
```

- [ ] 将选中值持久化到 `provider.meta.apiFormat`。
- [ ] 编辑已有但无 `apiFormat` 的提供商时，展示 `openai_responses`（保持旧行为）。
- [ ] 新建自定义 Codex 提供商时，默认 `openai_chat`。
- [ ] 运行组件/Hook 测试：

```powershell
pnpm test:unit -- tests/hooks/useSettingsForm.test.tsx tests/components/AddProviderDialog.test.tsx
```

期望：测试通过，且新断言证明 `apiFormat` 能在表单中完整往返。

## 任务 4：实现 Responses <-> Chat 请求体转换

**文件：**

- 新建：`src-tauri/src/proxy/providers/transform_responses_chat.rs`
- 修改：`src-tauri/src/proxy/providers/mod.rs`

- [ ] 实现 `responses_to_chat_request(body: Value) -> Result<Value, ProxyError>`。

必需映射：

```text
model -> model
instructions -> first system message
input message items -> messages
input function_call -> assistant message with tool_calls
input function_call_output -> tool role message
max_output_tokens -> max_tokens
stream -> stream
temperature -> temperature
top_p -> top_p
tools function array -> chat tools function array
tool_choice -> compatible chat tool_choice
reasoning.effort -> reasoning_effort
```

- [ ] 实现 `chat_to_responses_request(body: Value) -> Result<Value, ProxyError>`。

必需映射：

```text
model -> model
system message(s) -> instructions
user/assistant/tool messages -> input
max_tokens or max_completion_tokens -> max_output_tokens
stream -> stream
temperature -> temperature
top_p -> top_p
tools -> Responses function tools
tool_choice -> Responses tool_choice
reasoning_effort -> reasoning.effort
```

- [ ] 为纯文本、system instructions、tool calls、tool outputs、token 参数添加单元测试。
- [ ] 运行 Rust 定向测试：

```powershell
cargo test -p cc-switch transform_responses_chat --manifest-path src-tauri/Cargo.toml
```

期望：所有转换测试通过。

## 任务 5：实现非流式响应转换

**文件：**

- 修改：`src-tauri/src/proxy/providers/transform_responses_chat.rs`
- 修改：`src-tauri/src/proxy/response_processor.rs`
- 修改：`src-tauri/src/proxy/forwarder.rs`

- [ ] 实现 `chat_to_responses_response(body: Value) -> Result<Value, ProxyError>`。

必需映射：

```text
id -> id
model -> model
choices[0].message.content -> output message output_text
choices[0].message.tool_calls -> output function_call items
finish_reason stop -> status completed
finish_reason length -> status incomplete + incomplete_details.reason max_output_tokens
usage.prompt_tokens -> usage.input_tokens
usage.completion_tokens -> usage.output_tokens
usage.prompt_tokens_details.cached_tokens -> usage.input_tokens_details.cached_tokens
```

- [ ] 实现 `responses_to_chat_response(body: Value) -> Result<Value, ProxyError>`。

必需映射：

```text
id -> id
model -> model
output message output_text -> choices[0].message.content
output function_call -> choices[0].message.tool_calls
status completed with function_call -> finish_reason tool_calls
status completed without function_call -> finish_reason stop
status incomplete max_output_tokens -> finish_reason length
usage.input_tokens -> usage.prompt_tokens
usage.output_tokens -> usage.completion_tokens
```

- [ ] 在 forwarder 结果里增加小型枚举或上下文字段，例如 `response_transform: Option<ResponseTransform>`。
- [ ] 在响应返回客户端之前应用非流式响应转换。
- [ ] 确保 usage 记录读取的是面向客户端的已转换响应，或自动解析器可同时支持两种结构。
- [ ] 运行 Rust 定向测试：

```powershell
cargo test -p cc-switch transform_responses_chat --manifest-path src-tauri/Cargo.toml
```

期望：响应转换测试通过。

## 任务 6：实现流式 SSE 转换

**文件：**

- 修改：`src-tauri/src/proxy/providers/transform_responses_chat.rs`
- 修改：`src-tauri/src/proxy/response_processor.rs`

- [ ] 为 Codex 客户端兼容实现 Chat SSE -> Responses SSE 转换。

最低支持事件：

```text
chat delta content -> response.output_text.delta
chat delta tool_calls start/arguments -> response.output_item.added + response.function_call_arguments.delta
chat finish_reason stop -> response.completed
chat finish_reason tool_calls -> response.completed with function_call output
chat usage chunk -> response.completed.response.usage
[DONE] -> final SSE termination
```

- [ ] 为使用 Responses 上游的 Chat 客户端实现 Responses SSE -> Chat SSE 转换。

最低支持事件：

```text
response.output_text.delta -> choices[0].delta.content
response.output_item.added function_call -> choices[0].delta.tool_calls
response.function_call_arguments.delta -> choices[0].delta.tool_calls[].function.arguments
response.completed -> final chunk with finish_reason and usage
```

- [ ] 复用 `src-tauri/src/proxy/sse.rs` 现有 SSE 工具，保证 UTF-8 分块安全。
- [ ] 为 UTF-8 分片、文本 delta、tool-call 参数 delta 补充单元测试。
- [ ] 运行流式定向测试：

```powershell
cargo test -p cc-switch transform_responses_chat --manifest-path src-tauri/Cargo.toml
```

期望：所有流式转换测试通过。

## 任务 7：将协议感知路由接入 Codex 代理

**文件：**

- 修改：`src-tauri/src/proxy/forwarder.rs`
- 修改：`src-tauri/src/proxy/handlers.rs`
- 修改：`src-tauri/src/proxy/handler_config.rs`

- [ ] 为 Codex 上游格式增加解析逻辑：

```text
provider.meta.apiFormat == openai_chat      -> openai_chat
provider.meta.apiFormat == openai_responses -> openai_responses
missing apiFormat                           -> openai_responses
```

- [ ] 对上游为 `openai_chat` 的 Codex `/responses` 处理器：

```text
endpoint: /responses -> /v1/chat/completions
request: responses_to_chat_request
response: chat_to_responses_response or Chat SSE -> Responses SSE
```

- [ ] 对上游为 `openai_responses` 的 Codex `/chat/completions` 处理器：

```text
endpoint: /chat/completions -> /v1/responses
request: chat_to_responses_request
response: responses_to_chat_response or Responses SSE -> Chat SSE
```

- [ ] 协议一致时保持原有直通行为不变。
- [ ] 保持现有 Claude 转换行为不变（仅可复用共享 helper）。
- [ ] 运行代理测试：

```powershell
cargo test -p cc-switch proxy --manifest-path src-tauri/Cargo.toml
```

期望：现有代理测试通过，且新增路由测试通过。

## 任务 8：Universal Provider 集成

**文件：**

- 修改：`src-tauri/src/provider.rs`
- 修改：`src/components/universal/UniversalProviderFormModal.tsx`
- 修改：`cc-switch-main/src/config/universalProviderPresets.ts`
- 测试：`tests/utils/providerConfigUtils.codex.test.ts`
- 测试：`src-tauri/tests/provider_service.rs`

- [ ] 在 Universal Provider 的 Codex model settings 中加入 Codex 上游 API 格式。
- [ ] 在 Universal Provider GUI 中，启用 Codex 时展示上游 API 格式。
- [ ] 在 `to_codex_provider` 过程中，将所选上游格式复制到 `Provider.meta.apiFormat`。
- [ ] 保持 Codex live TOML 中 `wire_api = "responses"` 不变，因为它描述的是 Codex -> 本地代理协议，不是本地代理 -> 上游协议。
- [ ] 添加测试，证明 Universal Provider 同步后会按所选 `apiFormat` 生成 Codex provider metadata。
- [ ] 运行定向测试：

```powershell
pnpm test:unit -- tests/utils/providerConfigUtils.codex.test.ts
cargo test -p cc-switch provider_service --manifest-path src-tauri/Cargo.toml
```

期望：测试通过，且 Universal Provider 保留所选上游格式。

## 任务 9：完整验证

**文件：**

- 预计无需新增文件。

- [ ] 运行前端检查：

```powershell
pnpm typecheck
pnpm test:unit
```

期望：typecheck 与 Vitest 全量通过。

- [ ] 运行后端检查：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
```

期望：Rust 测试套件通过。

- [ ] Codex + DeepSeek 手工验证场景：

```text
1. 启动 cc-switch 代理。
2. 添加/选择 Codex DeepSeek 提供商。
3. 确认 GUI 显示上游 API 格式 = OpenAI Chat Completions。
4. 发起命中本地 /v1/responses 的 Codex 请求。
5. 确认日志显示上游端点为 /v1/chat/completions。
6. 确认 Codex 收到的是 Responses 形态响应。
```

- [ ] Responses 上游手工验证场景：

```text
1. 选择 OpenAI/Azure Responses 提供商。
2. 确认上游 API 格式 = OpenAI Responses。
3. 发起到本地 /v1/responses 的 Codex 请求。
4. 确认代理未执行 Responses <-> Chat 转换。
```

## 验收标准

- Codex GUI 可创建或选择 DeepSeek、Kimi、GLM 提供商。
- Codex 提供商具备可见、可编辑的上游 API 格式设置。
- DeepSeek/Kimi/GLM 预设默认 `openai_chat`。
- 现有无 `apiFormat` 的 Codex 提供商继续按 Responses 上游行为工作。
- Codex `/responses` 请求可通过代理转换访问仅支持 Chat Completions 的上游提供商。
- Chat Completions 客户端可通过反向转换使用仅支持 Responses 的 GPT 上游提供商。
- 现有 Claude 提供商转换相关测试继续通过。
