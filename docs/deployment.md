# Cloudflare 部署

## Agent 版本状态（2026-09-28）

本轮实现了 Agent Loop、OpenAI、Calculator/Calendar、Postgres JSONB 与 UI 活动记录，但**未重启或部署公网服务，未修改 DNS、Access、Tunnel**。下面的2026-09-27验收记录仅适用于旧 Print 版本。

默认模型现为 `gpt-6-luna`，`reasoning_effort = "medium"`，通过 Responses API 的 `reasoning.effort` 显式传递。本地3002端口已用该配置实际完成 Calendar → Calculator → 最终回答（14天、1680元）；Rust检查和隔离数据库集成测试通过。此验证不代表公网已切换模型。

新版本上线前：

1. 配置服务端 OPENAI_API_KEY 和 DATABASE_URL；系统参数位于 system_config/config.toml。可用 scripts/postgres.sh start 启动项目独立本机数据库，凭据在 Git 忽略的 deploy/runtime/postgres.env。数据库不自动开机启动。
2. 运行 scripts/check.sh 和 scripts/test-db.sh；备份目标数据库。应用启动会应用 migrations/0001_agent.sql，在专用数据库中创建三张业务表。现有浏览器历史不自动迁移或删除。
3. 停止连接同一数据库的本地预览实例；本版每个数据库只允许一个 Agent 服务实例。再通过 scripts/service.sh restart prod 构建、启动新版本。
4. 重新验收获准身份、未获准身份、未登录页面与直接 API。发送日期与金额组合问题，确认真实工具步骤、最终回答、刷新后的数据库记录和跨设备历史。测试发送中刷新、失败草稿保留和登录过期。

本地独立测试库已验证循环、持久化及失败边界；本地真实模型调用与浏览器验证记录见本次开发交付。它们不替代新的公网验收。

回退：先停止新版本服务，恢复旧代码与其配套前端构建再启动。保留 Postgres 数据及迁移记录，不删除新表；旧 Print 版本不会读取这些历史。若需恢复数据库，使用部署前备份并单独确认恢复范围。不要撤销其他项目资源。

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
