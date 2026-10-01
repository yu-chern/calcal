# Cloudflare 部署

## 来源绑定计算版本：部署准备（2026-10-01）

本次将生产工具切换为compute v2与clarify v2；实现见[协议说明](verified-computation.md)。当前段落在提交时仅记录部署准备，下面历史记录仍描述上一生产版本；实际重启与公网核验完成后更新本段。

- Rust/前端完整检查通过，隔离数据库旧协议兼容及新协议来源、答案、澄清测试通过。
- 24类真实模型案例通过（先跑完整集，再对原文解析修复项和编号澄清续接做定向回归）；最新结果每类均为一次模型调用。结果及run ID保存在[评估记录](validation/verified-computation.json)。这些是隔离测试库结果。
- 备份位于`deploy/runtime/backups/20261001-235328-verified/`：数据库归档经pg_restore --list检查、旧二进制、前端产物、HEAD源码归档和匹配的system_config。备份前生产运行中任务为0。无数据库迁移。
- 回退版本为`43b8542`（业务代码`dee6d5a`）：恢复该提交的源码和system_config后重启生产，保留数据库、.env、Tunnel及未提交工作。旧二进制SHA-256为`0406a7d2a7c9613bb318326058e1f68c58b4655a4388137a9b099ee58d0ccab9`。本次无迁移，应用回退通常不需要恢复数据库。

## 日期与计算优化已部署（2026-10-01）

**代码提交 `dee6d5a` 已推送至 origin/master，部署完成核验时间为柏林时间 21:05:50；已通过 `scripts/service.sh restart prod` 部署到 https://finanio.app。** 包括 compute 批量/依赖执行、日期条件搜索、服务端实际日期上下文、clarify 澄清终止与续接、UI 澄清标记。实现和部署前验证见 [执行优化说明](agent-execution.md)。

### 本次实际变更与验收

- 部署前确认无运行中任务，保存数据库 custom-format 备份，并用 pg_restore --list 检查可读取；另保存旧二进制、当时 web/dist、旧源码和旧 system_config。备份目录：`deploy/runtime/backups/20261001-210239/`，未提交。
- 先完成 release 构建，再重启生产 Rust 与专用 Tunnel；新 supervisor PID 为59479。Rust仍只监听127.0.0.1:3000。未修改 DNS、Access策略、Tunnel配置或凭据；未运行针对生产库的集成测试。
- 无新增数据库迁移；新运行快照及工具事件增加JSON字段。重启前两次检查均无运行中任务。
- 新 release SHA-256：`0406a7d2a7c9613bb318326058e1f68c58b4655a4388137a9b099ee58d0ccab9`。本机部署证据保存在被Git忽略的 `deploy/runtime/deployment.json`。
- 已确认原有DNS解析和专用Tunnel连接；实际公网HTTPS和直接API检查如下。

| 检查 | 本次结果 |
| --- | --- |
| 未登录公网首页、GET /api/session、GET /api/conversations、POST /api/messages | 均302到langload.cloudflareaccess.com，TLS验证成功 |
| 本机生产API，无令牌 | 401 |
| 本机生产API，只有伪造邮箱标头 | 401 |
| 本机生产API，无效JWT | 401 |
| 浏览器打开公网 | 实际显示Calcal的Cloudflare Access登录页 |
| 获准身份登录后的生产UI、compute/clarify完整往返 | 待用户测试；本次验收浏览器没有有效登录会话，没有冒充已通过 |
| 未获准身份完成真实登录后的拒绝 | 本次未重测；无第二个已验证身份。伪造标头/JWT检查不替代这项验收 |

部署前真实模型测试在隔离测试库完成：10类场景和追加3项澄清回归通过，不能等同于本次获准身份的公网端到端验收。现有用户刷新页面即可测试；如Access会话过期，按正常登录流程登录。

### 建议用户直接测试

1. 新建对话，发送“2026-10-01到2026-10-15按end-start的天数计费，每天120元，共多少元？”；预期14天、1680元，执行记录出现组合计算。
2. 新建对话，发送“最近一个2月最后一天为星期五的闰年是哪年？”；预期询问过去/未来/双向，UI显示等待补充条件。
3. 在同一对话回复“只向过去找，以2026年10月1日为参考。”；预期继续搜索，返回2008年2月29日。

### 回退

本次前一源码版本为 `2e799c1`；回退需要恢复与旧二进制相符的源代码及 system_config，再运行生产重启脚本（脚本会重新构建，不能只覆盖二进制然后重启）。备份包含 `previous-source.tar`、`previous-system_config/`、`previous-calcal`、`previous-web-dist/` 和 `calcal.dump`；备份的前端是在前一轮检查后已重建、但重启前实际提供的静态产物。保留现有.env、Tunnel/Access资源、其他未提交工作和数据库。此次无迁移，正常应用回退不需要恢复数据库。旧版不理解新的澄清链事件，回退后未完成的问题需要用户重述完整条件。

## Agent 版本状态（2026-09-28）

**已将提交 `8604475` 推送至 `origin/master`，并通过 `scripts/service.sh restart prod` 部署到 https://finanio.app。** 新版包括 Agent Loop、OpenAI、Calculator/Calendar、Postgres JSONB 与 UI 活动记录。下文2026-09-27记录属于旧 Print 版本。

生产模型为 `gpt-6-luna`，`reasoning_effort = "medium"`，通过 Responses API 的 `reasoning.effort` 显式传递。已从公网发送组合问题并完成 Calendar → Calculator → 最终回答；数据库中的此次运行快照确认模型和推理级别，终止原因为 completed。

### 实际部署变更与检查

- 更新 Rust release 和前端构建产物，重启生产 Rust 与 cloudflared；Rust 仅监听 `127.0.0.1:3000`。Tunnel 查询显示4条边缘连接，指向同一源站。
- 复用现有 DNS、Access 与专用 Tunnel，未更改这些配置。控制台确认 `finanio.app` 为代理 Tunnel 记录；Calcal 绑定该域名，`Calcal owner only` 策略为 Allow，唯一 Include 为指定完整邮箱。
- 服务端从 `.env` 读取 OpenAI 和 Cloudflare 配置，从被 Git 忽略的 `deploy/runtime/postgres.env` 读取数据库连接。项目 Postgres 运行于 `127.0.0.1:55432`，无其他预览服务连接同一业务库。
- 部署前备份位于本机 `deploy/runtime/backups/20260928-223437/`：`calcal.dump` 为数据库归档，已检查归档目录；`pre-agent-source.tar.gz` 为旧提交 `1b2d456` 的源码。备份没有进入 Git。
- `scripts/check.sh` 通过 Rust fmt/check/test/clippy、13项 Rust 单元/API测试及4项前端交互测试、前端格式/lint/类型检查/构建。`scripts/test-db.sh` 隔离数据库集成测试通过，生产 release 构建通过。`nom 1.2.4` 仍有编译器未来兼容性警告，当前构建成功。

### Agent 公网实测结果

| 检查 | 实测结果 |
| --- | --- |
| 未登录公网首页、`GET /api/session`、`GET /api/conversations`、`POST /api/messages` | curl 验证 TLS 成功，均返回302到团队 Access 登录页 |
| 无令牌访问本机生产首页和 session API | 401 |
| 本机 API 仅伪造邮箱标头或提供伪造 JWT | 401 |
| 获准身份的现有浏览器会话访问公网 | 可进入新版聊天界面并发送；本次未重新执行邮箱验证码登录 |
| 日期与计算组合问题 | 2026-10-01到2026-10-15相隔14天，每天120元，最终回答1680元 |
| UI 活动与完成状态 | 运行中显示“正在分析问题”，完成后可展开7条记录，包括“正在查询日期”和“正在计算” |
| Postgres 运行及消息记录 | completed；3次模型响应、2次工具调用及结果（calendar、calculator），用户/助手消息与7条活动已保存 |
| 刷新公网页面 | 从服务端恢复相同对话及执行记录 |
| 手机蜂窝网络、跨设备历史、登录过期 | 待用户正式测试 |
| 未获准邮箱真实登录 | 待用户使用自己控制的其他邮箱验收；代码拒绝测试与策略检查不替代真实身份测试 |

上述未登录 HTTP 结果以 curl 为准；Python urllib 的同类请求返回403，未作为302验收依据。浏览器直接打开 session JSON 页被浏览器工具阻止；获准身份的 API 可用性通过聊天页发送、轮询与刷新取回历史确认。

### 用户正式测试

1. 电脑保持联网、唤醒，Postgres、Rust 与 Tunnel 保持运行。在手机使用4G/5G打开 https://finanio.app；若无有效会话，用唯一获准邮箱完成验证码登录。
2. 发送“请查询2026-10-01到2026-10-15相隔多少天，再计算每天120元的总金额”。应得到14天、1680元，并能展开真实执行记录。界面接收请求不等于模型已完成回答。
3. 刷新并在另一设备用同一身份登录，确认数据库历史恢复。再检查发送中刷新、草稿刷新保留、网络失败提示和登录过期处理。
4. 无痕窗口直接访问 `/api/session`，应要求登录。使用自己控制的未获准邮箱尝试登录，应无法进入聊天页或读取 API 数据；不要分享验证码或令牌。

重启电脑后先运行 `scripts/postgres.sh start`，再运行 `scripts/service.sh start prod`。目前没有开机自启、崩溃自动重启或持续可用性保障；电脑休眠、断网或进程退出会影响公网访问。

回退：先停止生产服务，恢复 `1b2d456`（或上述源码归档）及其配套前端构建后重新启动，保留本机 `.env` 与 Tunnel 凭据。保留 Postgres 数据及迁移记录，不删除新表；旧 Print 版本不会读取这些历史。若需恢复数据库，使用部署前备份并单独确认恢复范围。不要撤销其他项目资源。

## Print 版本历史进度（2026-09-27）

- 已在控制台确认 `finanio.app` 为 Active，使用 Cloudflare DNS。
- 部署前 DNS 记录为 0 条，没有现有网站路由需要覆盖。
- 账户已有 Zero Trust，团队域名为 `langload.cloudflareaccess.com`；保留其既有设置与其他应用。
- 已创建本项目专用 Tunnel：`calcal-finanio`，UUID 为 `c5295efd-21f9-4f3b-9005-da25886e082a`。
- 本机配置 `deploy/cloudflared.local.yml` 已通过 ingress 校验，凭据位于 `deploy/runtime/tunnel.json`。两者均被 Git 忽略。
- **公网 HTTPS 入口及认证拦截已生效，手机登录后发送验收待完成。** 未登录访问 `https://finanio.app` 已实际出现 “Log in to Calcal” 邮箱验证码页面。
- 已创建专用 Access 应用 `Calcal`，ID `cfb7ef9a-235c-483d-9cd7-31235c46b085`，保护 `finanio.app` 全部路径；唯一 Allow 策略 `Calcal owner only`，ID `f552193b-e9d4-4b20-b9bc-3c0bcb5aeda0`，精确允许用户指定邮箱。应用会话 24 小时，使用 One-time PIN，启用 HttpOnly Cookie。未更改其他项目应用与策略。
- 已新增根域名代理 CNAME，指向本项目 Tunnel；部署前确认域名无其他 DNS 记录。未添加 `www` 路由。
- 已创建本机 `.env`，权限 `600`，采用实际 Access AUD 和指定邮箱，Git 忽略；不在文档记录凭据或会话令牌。
- 已通过 `scripts/service.sh start prod` 启动 Rust 与 cloudflared，开发模式已停止。Tunnel 查询显示 4 条边缘连接（Düsseldorf / Frankfurt）；状态是当次查询结果，电脑休眠、断网或进程停止后会变化。
- 脚本本地验收覆盖四个命令、重复启动/停止、启动会话退出后的后台运行、端口释放、单组件退出后的联动停止，以及通过 Vite 代理提交中文 prompt。本次部署运行完整 `scripts/check.sh` 和生产构建，均通过。
- 现有 `cloudflared` 证书可管理 Tunnel，但通过 API 读取 `finanio.app` 和账户 Access 配置的权限不完整；控制台会话可以访问这些配置。不要把 API 空结果当作账户没有资源。

### 本次公网验证结果

| 检查 | 实测结果 |
| --- | --- |
| 公网 HTTPS 首页 | TLS 验证成功；未登录返回 302 到团队 Access 登录页，浏览器显示 Calcal 登录表单 |
| 公网 `GET /api/session`、`POST /api/messages` | 未登录均返回 302 到 Access；不会到达打印业务 |
| 本机生产首页，无 JWT | 401 |
| 本机生产 API，仅伪造邮箱标头 | 401 |
| 本机生产发送 API，伪造 JWT | 401 |
| 获准邮箱经手机蜂窝网络登录并发送 | 待用户完成下方测试并核对日志 |
| 未获准邮箱真实登录 | 待手机验收；JWT 单元测试已覆盖其他邮箱拒绝，但不替代真实登录测试 |

## 单用户 Access 应用

1. 在现有 Zero Trust 账户创建独立 Self-hosted 应用，名称 `Calcal`，公开主机名 `finanio.app`，路径留空，以覆盖所有页面、静态资源与 `/api/*`。
2. 添加独立 Allow 策略，Include 使用唯一获准的完整邮箱。不要复用其他项目的多人策略，不要设置 Everyone / Bypass。
3. 选择可验证该邮箱的登录方式（例如一次性邮箱验证码），配置有限会话时长，如 24 小时。
4. 保存应用后记录 Application Audience (AUD)。在启用域名路由前验证策略覆盖范围。
5. 复制 `.env.example` 为 `.env`，填写 `CF_ACCESS_TEAM=langload`、实际 AUD、`ALLOWED_EMAIL`；保持 `AUTH_MODE=cloudflare` 和 `APP_ORIGIN=https://finanio.app`。

后端从团队域名获取 JWKS，仅接受 RS256，验证签名、`iss`、`aud`、`exp`、可选 `nbf`、`sub` 与邮箱。公钥每五分钟按需刷新；获取失败时拒绝访问并退避五分钟，恢复后可重启服务立即重取。所有页面和接口都经过相同校验。

## 构建与启动

在仓库根目录运行：

```bash
./scripts/check.sh
./scripts/service.sh stop         # 若开发模式在运行，先释放 3000 端口
./scripts/service.sh start prod   # 自动构建前端和 Rust，后台启动 Rust 与 Tunnel
./scripts/service.sh status prod
./scripts/service.sh restart prod # 停止整套服务、重新构建、重新启动
./scripts/service.sh stop prod
```

每条生产命令必须带 `prod`，省略时管理的是开发模式。`start prod` 先检查配置、端口及 ingress，再调用 `build.sh`、`serve.sh` 和 cloudflared。Rust 同时提供前端静态页面与 API，不启动 Vite。`serve.sh` 读取本机 `.env`，只允许 Cloudflare 模式。`.env` 是由 shell 读取的可信本地配置，不应放入不可信内容。统一脚本固定生产服务为 `127.0.0.1:3000`、来源为 `https://finanio.app`，与本项目 Tunnel 配置一致。

查看日志：

```bash
tail -f deploy/runtime/service/prod.log
```

启动信息与 prompt 原文追加写入上述日志；日志与 PID 状态均被 Git 忽略。脚本在独立后台会话中监督本次启动的两个进程，终端关闭后继续运行，任一组件退出会停止另一组件。重复启动不会多开；不强行停止占用端口的其他进程。它不提供开机自启或崩溃自动重启，电脑重启后需手动启动；当前未安装 launchd 服务。`status prod` 与启动成功只确认本机进程及本地 HTTP 响应，Tunnel 连接、DNS 和 Access 必须另行验收。

## 域名路由

先确认 Access 保护与后端生产模式已生效，再为根域名添加代理 CNAME：

| 字段 | 值 |
| --- | --- |
| Type | CNAME |
| Name | `@` |
| Target | `c5295efd-21f9-4f3b-9005-da25886e082a.cfargotunnel.com` |
| Proxy | Proxied |
| TTL | Auto |

若使用对该域名有权限的 cloudflared 证书，也可以执行 `cloudflared tunnel route dns calcal-finanio finanio.app`。先检查当前 DNS，再添加记录，不覆盖无关资源。

Tunnel 原理与配置参考 [Cloudflare 官方文档](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/create-local-tunnel/)。仅公开此 HTTP 服务，不公开 Vite、SSH 或其他本机端口。

## Print 版本历史手机验收步骤

以下为旧版步骤，不用于验收新的 Agent 版本：

1. 电脑保持联网与唤醒。在手机关闭 Wi-Fi、开启 4G/5G，使用 Safari 或 Chrome 打开 `https://finanio.app`（不加 `www` 或端口）。
2. 首次访问应出现 **Log in to Calcal**。输入唯一获准邮箱，点 **Send login code**，到邮箱取码并在网页输入。登录跳转到 `langload.cloudflareaccess.com` 是已有 Cloudflare 团队的正常认证流程；成功后回到 `finanio.app`。
3. 进入聊天页后发送 `手机外网验收-927`，应显示 `消息已在 Rust 后端打印。`。电脑可执行 `tail -f deploy/runtime/service/prod.log` 核对相同原文。此版本只打印，不产生模型回答。
4. 刷新页面，确认当前手机浏览器的对话仍保留。发送前输入一段草稿再刷新，草稿也应保留；电脑与手机的历史不会同步。
5. 打开独立无痕窗口，直接访问 `https://finanio.app/api/session`，未登录时仍应进入验证，不能直接读到 JSON。使用你自己控制的另一个、未获准邮箱尝试登录，不应进入网站；验证码界面可能统一提示已发送，不代表获得了访问权。
6. 若出现问题，记录时间、停在哪一步和屏幕提示；不要分享验证码或会话令牌。只有成功登录、发送确认和电脑日志一致，才算完成手机端到端验收。

日常启动与重启使用 `./scripts/service.sh start prod` 和 `./scripts/service.sh restart prod`；只执行默认 `start` 会启动本地开发模式，不能提供上述公网访问。

完成上面的 Access、`.env`、启动和域名路由步骤后，手机无需加入电脑所在的 Wi-Fi，也无需安装 VPN。电脑必须联网、保持唤醒，Rust 和 Tunnel 必须都在运行；不要把手机浏览器指向 `127.0.0.1`，那是手机自身的地址。Tunnel 使用电脑主动发起的连接，正常情况下无需路由器端口映射。

如果使用邮箱验证码，在 Cloudflare Zero Trust 的登录方式中启用 One-time PIN，并将它用于 Calcal 应用；Allow 策略仍只包含唯一邮箱。手机打开网址后输入该邮箱，从邮箱取验证码完成登录。配置参考 [Self-hosted 应用](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/self-hosted-public-app/) 与 [One-time PIN](https://developers.cloudflare.com/cloudflare-one/integrations/identity-providers/one-time-pin/) 官方文档。

- 蜂窝网络打开 `https://finanio.app`，未登录时进入 Access 登录界面。
- 获准邮箱登录后可以提交 prompt，页面出现打印确认，电脑日志出现原文。
- 未获准身份不能进入网站或直接调用 `/api/messages`。
- 无 JWT / 伪造 JWT 直接调用本机生产服务应返回 401；不能通过邮箱标头绕过。
- 手机窄屏下输入框、发送按钮可操作；历史对话可切换，刷新后从当前浏览器恢复。
- 登录过期、后端断开时显示明确失败，不把失败显示为已打印。

本地自动测试覆盖 JWT 和 API 边界，无法代替真实 Cloudflare 策略及手机外网验收。

## 迭代与回退

修改前端或 Rust 后运行 `./scripts/service.sh restart prod`，会重新构建并启动整套服务。域名和 Tunnel 配置可以保持原样。重建期间网站暂时不可用；如果构建失败，修复后再运行 `start prod`。

回退时运行 `./scripts/service.sh stop prod`，停止本脚本管理的 Tunnel 与 Rust。如需撤销域名入口，只删除这次新增的 `finanio.app` CNAME，保留其他 DNS 和账户设置。Access 策略可继续保留保护；确认不再使用后再删除本项目的应用或 Tunnel，不动其他项目。
