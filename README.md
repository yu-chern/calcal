# Calcal

个人轻量 Agent 实验项目：React 手机聊天界面 + Rust Agent Loop + Postgres JSONB 对话记录。模型适配器使用 OpenAI Responses API，生产工具为 compute v2、clarify v2 与 respond v1。模型绑定用户原文并生成受限计算程序，Rust 校验来源、执行数学/日期/列表运算并直接呈现成功结果。自由文本计算答案不能绕过后端校验。问候、能力介绍与已覆盖概念使用后端知识条目正常结束；混合问题仍执行计算程序，解释历史计算只能引用同会话成功记录。详见 [回答分支](docs/non-computation-replies.md)及[来源绑定计算协议](docs/verified-computation.md)。

UI 在运行时显示“正在分析问题”“正在查询日期”“正在计算”等真实步骤，完成后可展开执行记录。它不显示模型内部推理，也不把提交成功当作模型回答。

**来源绑定计算版本已于2026-10-01部署到 [finanio.app](https://finanio.app)，仅允许指定邮箱登录。** 当前提交`8b322fc`，生产模型为`gpt-6-luna`、推理级别`medium`。24类隔离真实模型案例通过；本次公网未登录保护与本机身份拒绝检查通过，登录后的生产UI尚待用户验证，详见[部署说明](docs/deployment.md)。本机需保持联网与唤醒。

## 日期与计算执行优化（2026-10-01）

当前源码增加批量/依赖计算、有界日期条件搜索、可信当前日期上下文与显式澄清续接。改动和验证见 [优化说明](docs/agent-execution.md)。代码 `dee6d5a` 已于2026-10-01提交、推送并重启部署到 [finanio.app](https://finanio.app)。生产服务、Tunnel及未登录/API保护已检查；获准身份登录后的UI测试待用户完成，详见 [部署记录](docs/deployment.md)。

## 本地启动

需要 Rust 1.98+、Node.js 22.22.2+（22.x）、24.15+（24.x）或26+、npm、PostgreSQL 17+。本机脚本还需要 Bash、Python 3、curl、lsof，以及 PostgreSQL 的 initdb/pg_ctl/psql/createdb。

1. 在本机 `.env` 设置 `OPENAI_API_KEY`。不要覆盖已有 Cloudflare 配置，也不要提交密钥。
2. 使用已有 Postgres 时，在 `.env` 设置 `DATABASE_URL`；或者启动项目独立的数据库：

```bash
./scripts/postgres.sh start
npm --prefix web ci
./scripts/service.sh start
```

打开 <http://127.0.0.1:5180>。后台开发服务使用 Vite + Rust；停止、状态、重启分别使用 `./scripts/service.sh stop|status|restart`。前台开发可用 `./scripts/dev.sh`。

`postgres.sh` 只管理 `deploy/runtime/postgres/data`，固定监听 `127.0.0.1:55432`，启用 SCRAM 密码认证，创建 `calcal` 和隔离测试库 `calcal_test`。随机凭据保存在被 Git 忽略、权限受限的 `deploy/runtime/postgres.env` 和 `postgres/pgpass`。它不会修改系统 PostgreSQL，也不安装开机服务。可用 `./scripts/postgres.sh status|stop` 管理；停止应用不会自动停止数据库。

应用依次读取现有环境变量、`.env`、`deploy/runtime/postgres.env`，前者优先。数据库连接或 API Key 缺失会拒绝启动，不回退到打印或模拟回复。启动会应用 `migrations/` 中的 SQL 迁移。单个数据库只允许一个 Agent 服务实例，以便安全标记上次异常退出的运行。

如果 3000 端口已有生产服务，请使用另一个端口测试，避免中断它：

```bash
npm --prefix web run build
AUTH_MODE=local APP_ORIGIN=http://127.0.0.1:3002 PORT=3002 cargo run --locked
```

这时 Rust 同时提供页面和 API，访问 <http://127.0.0.1:3002>。本地免登录服务和 Vite 均不能连接公网 Tunnel。

## 配置和扩展

```text
system_config/
  config.toml                  模型、限额、数据库环境变量名、时区、启用工具
  system_prompt.md             Agent 行为说明
  tools/*.json                 版本化工具定义、JSON Schema、活动文案、实现绑定
src/
  agent/config.rs              TOML 读取与启动校验
  agent/mod.rs                 Loop、限额、运行快照、任务关闭
  models/mod.rs                模型无关的请求、响应及 ModelAdapter 接口
  models/openai.rs             OpenAI Responses 协议与续接信息
  tools/mod.rs                Tool 接口、注册表、校验和执行
  tools/calculator.rs          受限数学表达式
  tools/calendar.rs            日期与时区运算
  tools/program/              来源绑定的受限计算语言、精确数值与答案呈现
  tools/compute.rs             旧版计算计划（仅兼容测试）
  tools/date_search.rs         有界日期条件搜索
  tools/clarification.rs       终止当前运行并等待澄清
  storage/mod.rs               Postgres 持久化、幂等请求、历史上下文
  http.rs / access.rs          同源 API、Access JWT 与访问边界
migrations/                   版本化 SQL 迁移
```

修改 `system_config/config.toml` 后重启生效，默认模型 ID 为 `gpt-6-luna`，`reasoning_effort = "medium"`。省略该字段时使用 Rust 常数 `DEFAULT_REASONING_EFFORT`（medium）；允许 none、low、medium、high、xhigh、max，非法值会在启动时拒绝。OpenAI 适配器将其发送为 Responses API 的 `reasoning.effort`，每次运行的配置快照也会保存实际值。[GPT-6 Luna 官方说明](https://developers.openai.com/api/docs/models/gpt-6-luna)。实际访问能力取决于 API 账户。API Key 和数据库密码不放在 TOML、工具定义或前端中。

模型实现 `ModelAdapter` 后，在启动组装处注册/选择即可；Loop 和工具不依赖 OpenAI。当前只实现 OpenAI 适配器，没有声称兼容所有厂商。其他模型的工具调用协议、消息格式和能力需要由新适配器转换。不支持原生工具调用的纯文本模型需要额外校验策略。

新工具实现 `Tool::execute`，在 `ToolRegistry::load` 的实现表添加 binding，并把 JSON 定义加入 TOML 的 definitions。注册表支持 register 替换和 disable 停用；克隆的运行快照保留原定义及实现。配置编辑在重启后生效；Rust 逻辑编辑需重新编译。尚无在线工具编辑界面或任意代码插件加载。`activity` 是受信任配置中的短文案，不来自模型生成的“思考过程”。

## 工具与边界

- Calculator：支持四则运算、括号、幂、sqrt/abs/exp/ln/sin/cos/tan/floor/ceil/round/min/max 等纯数学函数；三角函数使用弧度。最长512字节，拒绝非有限结果。使用 IEEE-754 双精度近似计算，金额应明确舍入，不承诺任意精度。无 shell、文件或网络访问能力。
- Calendar：公历 today/inspect/diff/add；默认 Europe/Berlin，单次运行固定参考时间；星期一为1。diff 为 end-start，add 支持天/周/月/年，负数表示减法。不自动包含首尾两日，不支持节假日/工作日/个人日程。不存在的日期、月份截断或范围溢出返回结构化错误。
- Compute：最多16个有序步骤，批量数学/日期或跨类型依赖可在一次工具往返内完成。通过 JSON pointer 引用此前成功步骤的数值或字符串（例如 `/span/days`），不是字符串插值或任意代码执行。日期搜索支持月份、月日、星期、闰年、月末条件及过去/未来/双向最近，单步最多搜索146097天（400年），最多16个命中，并列截止项全部保留。只问最近一个时 limit=1。未命中仅代表指定范围内无解；不会从有限查询推断全局无解。
- Clarify：有歧义、冲突、关键条件缺失或能力不足时，模型提交问题和可选选项，后端立即结束本轮并等待用户回复，不再调用模型复述问题。如果同一模型响应混有计算调用，优先澄清并记录其他工具未执行。
- 默认最多8次模型请求、12个工具工作单元、90秒总时间、每工具2秒、每模型请求30秒，最多同时运行2个任务。普通工具计1，compute 按计划子步骤数计费；16步是工具上限，默认总预算12仍生效。新版计算运行时还限制共享操作预算、表达式嵌套和1秒内部截止时间；步骤间主动让出执行权。限制由 Rust 执行。
- compute 子步骤在后端直接传递结果，任一步失败后停止剩余步骤；只有成功结果可用于答案。错误交回模型修正，成功后由服务器直接呈现，不增加复述答案的模型轮次。解析及输出来源失败不会被当作已完成。
- HTTP 请求保持10秒上限；POST 仅接收并启动任务，运行进度通过短 GET 请求轮询。断开浏览器不会取消已接受的任务；服务关闭会取消任务，异常退出后标记为中断。
- 每轮把固定参考时间对应的实际日期和时区写入模型 instructions 与运行快照，查询实际今天不必调用工具。上下文默认选择最近的完整用户/助手交流，并受字符预算限制；完整消息仍在数据库。字符预算不是精确 token 预算，目标模型仍可能拒绝超出其上下文限制的请求。历史工具细节不自动跨运行重放。未解决的澄清链单独保存并在后续轮次优先恢复，避免短历史窗口丢失原问题；过长时明确报上下文上限，不静默丢弃条件。

## 数据存储

Postgres 的 conversations 记录会话归属，runs 记录状态、模型/配置/工具版本快照，conversation_entries 按顺序保存 JSONB 用户消息、助手消息、模型输出、工具调用、工具结果和活动事件。内容有 schema_version，单条记录一行。新运行快照为 schema_version=2，保存动态 instructions 与 runtime_context；工具事件增加 step 和 work_units。clarification 事件保存问题和待确认上下文，tool_skipped 明确表示未执行；compute 的逐子步骤原始参数、解析后参数和结果保存在 tool_result 中。新版另以 input_sources 保存编号输入与动态指令，answer_verified 保存工具 call_id 及答案 claims；自由文本绕过尝试记录为 answer_rejected。无需新增数据库迁移，旧记录保持原样。

API 只返回经过筛选的消息和简短活动记录，不返回模型续接信息、系统配置或凭据。会话归属来自已验证身份，客户端不能指定 owner。OpenAI 请求使用 `store:false`，本地保留所需续接内容；这不是对供应商整体数据保留政策的承诺。

浏览器 localStorage 仅保存草稿、活动会话 ID 和待确认请求 ID；刷新后从 Postgres 恢复消息和运行状态。旧 Print 版本的 `calcal.conversations.v1` 不删除、不自动上传，本版界面不读取它；需要时可单独迁移。同一身份可在多个设备访问数据库历史。

请求 ID 用于幂等：网络错误后，相同草稿重试会复用待确认 ID；已经完成的同一请求不会再次调用模型。同一会话同一时间只接受一个运行，冲突返回409。没有数据库恢复时不会声称消息已保存。

**保留原先授权的日志行为：每个新接受的 prompt 原文会 println! 一次。** 日志和数据库都包含聊天正文；不会额外记录 API Key、JWT 或请求头。开发/生产日志位置仍在 `deploy/runtime/service/`。

## HTTP 接口

所有 API 和静态资源共用 Access/本地守卫；POST 要求精确 Origin。生产只允许配置的单一邮箱，验证 JWT 签名、签发方、受众、有效期等，不信任邮箱头。Rust 始终监听回环地址。

| 接口 | 行为 |
| --- | --- |
| `GET /api/session` | `mode:agent`、auth、ready、max_prompt_chars |
| `POST /api/messages` | JSON `{prompt, conversation_id, request_id}`，两个 ID 为 UUID；202返回 `{run_id}` |
| `GET /api/runs/:id` | status、reason、prompt、response、error、activity、activities |
| `GET /api/conversations?offset=0` | 最近100个会话摘要，按更新时间倒序 |
| `GET /api/conversations/:id?offset=0` | 最近50个运行，页内按时间正序；支持加载更早消息 |

状态为 running/completed/failed。终止原因包括 completed、clarification、refused、model_error、timeout、budget_exhausted、context_limit、storage_error、cancelled、interrupted。模型拒绝时返回可见文本且 reason=refused。非最终文本不会提前展示成答案。澄清返回 `status=completed, reason=clarification, activity=等待澄清`：表示这一轮已结束、仍需用户输入，并非已解出原问题；response 是澄清问题。UI 标注“等待你补充条件”，用户继续在同一会话正常发送消息即可，无需新的端点。

每条消息最多4000个 Unicode 字符，请求体64 KiB。错误包含 error：400无效输入、401未认证、403来源错误、404记录不存在或不属于当前身份、409会话忙/幂等冲突、413过大、429并发上限、503存储或配置不可用。数据库错误详情不会返回浏览器。

## 验证

```bash
./scripts/check.sh      # fmt/check/test/clippy + 前端格式/lint/交互测试/类型检查/构建
./scripts/test-db.sh    # 独立 TEST_DATABASE_URL：真实数据库 + 测试专用模型适配器
./scripts/build.sh      # 前端 + Rust release
```

数据库测试默认被 cargo test 标为 ignored，必须单独执行 test-db.sh。该测试不删除记录，不连接 DATABASE_URL；覆盖多工具循环、JSONB记录、幂等、身份隔离、同会话冲突、超时、步数上限、取消与中断恢复。模型替身只在测试中使用。另有显式启用的真实模型回归：`CALCAL_LIVE_EVAL=1 cargo test --locked --test agent_live -- --ignored --nocapture`。它读取本机服务端模型配置，会消耗模型额度，只允许名称以 `_test` 结尾且不同于 `DATABASE_URL` 的测试库；不属于默认测试或 `test-db.sh`。只输出测试问题、可见回答与工具数据，不输出凭据或供应商推理续接内容。

生产启动仍使用 `./scripts/service.sh start prod`，事先配置数据库与 API Key，并停止使用同一数据库的本地预览实例。部署前备份数据库；详细状态、验收和回退见 [docs/deployment.md](docs/deployment.md)。
