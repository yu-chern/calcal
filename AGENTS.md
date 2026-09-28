# 项目协作约定

## 目的与当前范围

Calcal 是帮助用户理解 agent 工作机制的个人实验项目。React + TypeScript 提供手机聊天界面，Rust 负责后端。目标域名为 `finanio.app`，只允许一个指定用户登录。

**当前用户指定的行为：Rust 实现轻量 Agent Loop，模型通过适配器接入，工具通过注册表扩展；初期使用 OpenAI、calculator 与 calendar。对话、工具调用和运行记录以 JSONB 保存到 Postgres。UI 显示真实执行步骤的简短摘要，不展示模型内部推理。** 接收成功与最终模型回答必须区分。

公网部署的实际状态和缺项见 `docs/deployment.md`，不要仅根据配置文件存在就声称已上线。Codex Remote 属于用户的独立开发工作流，本项目不实现远程桌面或命令执行。

## 先看哪里

| 任务 | 入口 |
| --- | --- |
| Agent 循环 / 配置 | `src/agent/`、`system_config/` |
| 模型 / 工具 / 存储 | `src/models/`、`src/tools/`、`src/storage/`、`migrations/` |
| HTTP 接口 / 参数 / 来源校验 | `src/http.rs` |
| 登录验证 | `src/access.rs` |
| 环境变量 / 服务启动 | `src/config.rs`、`src/main.rs` |
| 聊天 UI / 交互状态 | `web/src/App.tsx` |
| 前端请求 | `web/src/api.ts` |
| 手机布局 / 样式 | `web/src/styles.css` |
| 本地启动 / 检查 / 构建 | `scripts/` |
| 部署与回退 | `docs/deployment.md`、`deploy/` |
| API / 身份验证测试 | `tests/`，以及 `src/access.rs` 内的守卫测试 |

## 目录与实现原则

- 保持一个根目录 Cargo 包，前端放 `web/`。Node.js 只承担前端工具链，不加第二套业务后端。
- Rust 提供前端构建产物与同源 `/api/*`；本地 Vite 仅代理 API。
- 每个模块一个明确职责。先写直接可读的实现，不提前引入通用仓库层、依赖注入框架、agent 框架或 Cargo workspace。数据库使用已授权的 Postgres。
- `src/agent/` 保持模型适配器、工具执行与 HTTP 独立。必须有步数上限、超时、终止原因和可观察事件。工具更新不能改变已开始运行的版本快照。
- 对话历史以 Postgres 为准；localStorage 只保存草稿、当前对话 ID 与待确认请求 ID。旧版 `calcal.conversations.v1` 保留且不自动上传。消息按请求 ID 去重；同一对话一次只运行一个任务。
- `target/`、`web/node_modules/`、`web/dist/` 和 `deploy/runtime/` 是生成内容或本机状态，常规搜索排除这些目录。

## 本地命令

```bash
npm --prefix web ci
./scripts/postgres.sh start # 可选：启动项目独立本机数据库
./scripts/service.sh start   # 后台启动前后端；另有 stop / status / restart
./scripts/service.sh start prod # 生产构建 + Rust + Tunnel；需先完成部署配置
./scripts/dev.sh             # http://127.0.0.1:5180；Ctrl+C 停止两个进程
./scripts/check.sh           # Rust / 前端检查
./scripts/test-db.sh         # 独立 Postgres 数据库集成测试
./scripts/build.sh           # 前端 + Rust release
./scripts/serve.sh           # 生产模式，读取本机 .env
```

格式化使用 `cargo fmt` 和 `npm --prefix web run format`。新增依赖时更新对应锁文件。使用 `npm ci` 安装已有依赖，不使用 `--force` 或忽略 peer 依赖冲突。

## 访问边界

- 生产模式通过 Cloudflare Access + Tunnel，只允许指定邮箱。保护整个站点和 API，不只隐藏前端元素。
- 验证 Access JWT 的签名、签发方、受众、有效期和邮箱。不能仅信任邮箱请求标头。
- Rust 始终监听 `127.0.0.1`；本地免登录必须显式启用，并拒绝 Cloudflare 转发请求。禁止将免登录模式或 Vite 接到公网 Tunnel。
- 写入请求校验精确 Origin；保留输入大小、并发与超时边界。不开放任意来源的凭据访问。
- API Key、Tunnel 凭据和会话令牌只能留在服务端本机配置。不得输出或提交，也不得使用 `VITE_*` 保存秘密。
- 用户明确要求打印 prompt；仅此消息正文可以输出。README 告知日志行为，界面按用户要求保持简洁；不附带记录身份令牌或请求标头。
- `tests/fixtures/test-only-private.pem` 是故意公开的测试密钥，只用于离线测试，禁止用于部署。
- 部署前检查已有 DNS、Access 和 Tunnel，保留其他项目资源。部署记录包含实际变更与回退办法。

## 工作与验证

- 默认中文沟通，英文代码标识符；修改前检查 Git 状态，保留用户的其他工作。
- 一次推进可验证的小目标，优先完成已授权工作。同步更新 README、部署状态和发生变化的接口说明。
- Rust 改动运行 fmt/check/test；身份验证或 HTTP 改动同时运行 Clippy 和相关边界测试。存储/Agent 改动同时运行 `scripts/test-db.sh`，不可将集成测试指向正式数据库。
- 前端改动运行格式检查、lint、类型检查与构建；验证手机窄屏、中文输入法、发送中、失败提示和草稿保留。
- 公网验收要实际检查未登录、获准身份、未获准身份和直接 API 访问。自动测试通过不等于 Cloudflare 已正确部署。
- 仅文档改动不新增测试。不要运行不相关的重复验证。
- 完成后说明改动、验证结果和真实剩余项；不得声称未执行的检查通过。
