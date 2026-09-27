# Calcal

用于学习 agent 工作机制的个人实验项目。React 提供手机和桌面聊天界面，Rust 接收请求；逐步从消息传递扩展到 agent loop。

**当前版本是 Print demo：发送文字 → Rust 原样 `println!` → 页面显示接收确认。没有 LLM、自动回复或 agent loop。** 不需要 API Key。

目标入口：`https://finanio.app`。当前本地版本已可运行，公网部署尚未完成；已创建独立 Tunnel，正在等待确定唯一登录邮箱和配置 Access / DNS。实际进度见 [部署说明](docs/deployment.md)。

## 快速开始

需要 Rust 1.98+、Node.js 22.12+（建议使用满足 Vite 要求的受支持 Node LTS）和 npm。

```bash
npm --prefix web ci
./scripts/dev.sh
```

打开 <http://127.0.0.1:5180>。Vite 自动更新前端，Rust 在 `127.0.0.1:3000` 接收 `/api` 请求。提交的 prompt 会出现在运行脚本的终端中。修改 Rust 后停止脚本并重新运行；Ctrl+C 同时停止两个开发进程。

开发脚本显式启用本地免登录模式。请使用 `127.0.0.1:5180`，因为写入接口只接受配置的精确来源。代码默认使用 Cloudflare 身份校验，缺少配置会拒绝启动。

验证与公网一致的“Rust 提供页面及 API”路径：

```bash
npm --prefix web run build
AUTH_MODE=local APP_ORIGIN=http://127.0.0.1:3000 cargo run --locked
```

打开 <http://127.0.0.1:3000>。这仍然是本地测试，不应连接公网 Tunnel。

## 目录导航

```text
calcal/
├── AGENTS.md                  # coding agent 的约定、任务入口和检查命令
├── Cargo.toml / Cargo.lock    # 唯一 Rust 包；不提前拆 workspace
├── src/
│   ├── main.rs                # 配置、监听、进程启动
│   ├── lib.rs                 # 模块入口，便于集成测试
│   ├── config.rs              # 环境变量、运行模式
│   ├── http.rs                # 路由、请求校验、静态文件
│   ├── access.rs              # Cloudflare JWT 校验与公钥缓存
│   └── prompt.rs              # 唯一业务动作：打印 prompt
├── web/
│   ├── src/App.tsx            # 聊天状态、消息列表、输入框
│   ├── src/ConversationHistory.tsx # 历史对话抽屉
│   ├── src/conversations.ts   # 对话类型、本机历史读取与校验
│   ├── src/api.ts             # 同源 API 请求和错误提示
│   ├── src/styles.css         # 响应式样式
│   ├── src/main.tsx           # React 挂载
│   ├── public/                # favicon 等静态资源
│   └── package.json           # 前端命令；package-lock.json 固定依赖
├── tests/
│   ├── api.rs                 # 输入、来源与本地访问边界
│   ├── access.rs              # 有效/伪造/过期/其他用户 JWT
│   └── fixtures/              # 仅用于离线测试的公开测试密钥
├── scripts/                   # dev / check / build / serve
├── deploy/                    # Tunnel 配置示例；本机配置和凭据已忽略
└── docs/deployment.md          # Cloudflare 配置、启动、验证与回退
```

不引入数据库、Node 后端或 agent 框架。需要改后端行为时从 `src/prompt.rs` 开始；需要改界面时从 `web/src/App.tsx` 开始。后续真正引入循环时再增加 `src/agent/`。

## 当前行为

- 输入文字并发送；支持中文、emoji 和多行，后端保留原文。
- 桌面 Enter 发送、Shift+Enter 换行；中文输入法选词不会误发送。手机通过发送按钮提交。
- 明确显示发送中、打印成功与失败；失败保留草稿，不自动重发。
- 顶部左侧打开历史对话，右侧 `+` 新建对话；后端返回的文字直接展示在消息区。
- 历史和草稿保存在当前浏览器的 localStorage，刷新后可恢复；服务端不保存聊天历史，不同浏览器或设备之间不自动同步。清除网站数据会删除本机历史。
- 每条消息最多 4000 个 Unicode 字符，请求体最多 64 KiB；空白消息拒绝发送。
- **按此 demo 的目的，消息正文会进入后端标准输出。** 不要输入密钥；如将输出重定向到日志文件，日志也会保留正文。

## HTTP 接口

所有接口和静态文件共用身份校验。生产模式验证 Cloudflare Access JWT 的签名、签发方、受众、有效期及允许的邮箱，不能用邮箱标头代替令牌。实现依据 [Cloudflare JWT 验证文档](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/)。

| 请求 | 输入 | 成功响应 |
| --- | --- | --- |
| `GET /api/session` | 无 | `{"mode":"print","auth":"local","max_prompt_chars":4000}`，生产环境 `auth` 为 `cloudflare` |
| `POST /api/messages` | `{"prompt":"你好"}` | `{"status":"printed","message":"消息已在 Rust 后端打印。"}` |

在 `scripts/dev.sh` 启动后，可用以下命令直接验证：

```bash
curl --fail-with-body http://127.0.0.1:3000/api/messages \
  -H 'Origin: http://127.0.0.1:5180' \
  -H 'Content-Type: application/json' \
  --data '{"prompt":"你好，Rust！"}'
```

错误返回 `{"error":"..."}`：无效输入 400、未认证 401、来源不匹配 403、请求超时 408、请求体过大 413。Rust 始终监听回环地址。本地模式还会拒绝公网 Host 和带 Cloudflare 转发标头的请求，防止误接入 Tunnel。

## 检查与构建

```bash
./scripts/check.sh  # Rust fmt/check/test/clippy + 前端格式/lint/类型检查/构建
./scripts/build.sh  # npm ci + 前端构建 + Rust release 构建
```

前端格式化：`npm --prefix web run format`；Rust 格式化：`cargo fmt`。提交锁文件，不提交构建产物、`.env` 或 Tunnel 凭据。

## 部署与后续开发

部署采用 `finanio.app → Cloudflare Access → Tunnel → 本机 Rust → React 静态页面 / API`。电脑和服务需要保持运行，休眠或断网时无法从手机访问。具体步骤及状态见 [部署说明](docs/deployment.md)。

用户通过 Codex Remote 操作开发电脑，在手机浏览器测试这个项目；此仓库不承担远程终端或桌面控制。

后续按实验需要依次增加：agent loop、模型适配器、工具调用、运行轨迹和持久化。不要把当前的“打印成功”解释为模型回复。
