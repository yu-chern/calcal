# Cloudflare 部署

## 当前进度（2026-09-27）

- 已在控制台确认 `finanio.app` 为 Active，使用 Cloudflare DNS。
- 部署前 DNS 记录为 0 条，没有现有网站路由需要覆盖。
- 账户已有 Zero Trust，团队域名为 `langload.cloudflareaccess.com`；保留其既有设置与其他应用。
- 已创建本项目专用 Tunnel：`calcal-finanio`，UUID 为 `c5295efd-21f9-4f3b-9005-da25886e082a`。
- 本机配置 `deploy/cloudflared.local.yml` 已通过 ingress 校验，凭据位于 `deploy/runtime/tunnel.json`。两者均被 Git 忽略。
- **尚未上线**：还需要唯一获准登录邮箱、专用 Access 应用、域名 DNS 路由、生产进程及外网登录验收。
- 现有 `cloudflared` 证书可管理 Tunnel，但通过 API 读取 `finanio.app` 和账户 Access 配置的权限不完整；控制台会话可以访问这些配置。不要把 API 空结果当作账户没有资源。

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
./scripts/build.sh
./scripts/serve.sh
```

`serve.sh` 读取本机 `.env`，只允许 Cloudflare 模式。`.env` 是由 shell 读取的可信本地配置，不应放入不可信内容。默认服务监听 `127.0.0.1:3000`。另开一个终端启动 Tunnel：

```bash
cloudflared tunnel --config deploy/cloudflared.local.yml ingress validate
cloudflared tunnel --config deploy/cloudflared.local.yml run calcal-finanio
```

首轮部署先用两个前台终端观察行为。控制台中的服务启动信息写 stderr，prompt 原文写 stdout。需要后台常驻时，使用独立的 macOS launchd 用户服务监督这两个进程，记录具体服务名与日志位置；不要依赖一个退出后会关闭的临时终端。当前尚未安装 launchd 服务。

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

## 手机验收

- 蜂窝网络打开 `https://finanio.app`，未登录时进入 Access 登录界面。
- 获准邮箱登录后可以提交 prompt，页面出现打印确认，电脑日志出现原文。
- 未获准身份不能进入网站或直接调用 `/api/messages`。
- 无 JWT / 伪造 JWT 直接调用本机生产服务应返回 401；不能通过邮箱标头绕过。
- 手机窄屏下输入框、发送按钮可操作；历史对话可切换，刷新后从当前浏览器恢复。
- 登录过期、后端断开时显示明确失败，不把失败显示为已打印。

本地自动测试覆盖 JWT 和 API 边界，无法代替真实 Cloudflare 策略及手机外网验收。

## 迭代与回退

修改前端后运行 `npm --prefix web run build`；修改 Rust 后运行 `cargo build --release --locked` 并重启 `serve.sh`。域名和 Tunnel 可以保持原样。

回退时先停止本项目的 Tunnel 进程，使外网无法再到达本机；然后停止 Rust 服务。如需撤销域名入口，只删除这次新增的 `finanio.app` CNAME，保留其他 DNS 和账户设置。Access 策略可继续保留保护；确认不再使用后再删除本项目的应用或 Tunnel，不动其他项目。
