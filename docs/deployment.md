# Cloudflare 部署

## 当前进度（2026-09-27）

- 已在控制台确认 `finanio.app` 为 Active，使用 Cloudflare DNS。
- 部署前 DNS 记录为 0 条，没有现有网站路由需要覆盖。
- 账户已有 Zero Trust，团队域名为 `langload.cloudflareaccess.com`；保留其既有设置与其他应用。
- 已创建本项目专用 Tunnel：`calcal-finanio`，UUID 为 `c5295efd-21f9-4f3b-9005-da25886e082a`。
- 本机配置 `deploy/cloudflared.local.yml` 已通过 ingress 校验，凭据位于 `deploy/runtime/tunnel.json`。两者均被 Git 忽略。
- **尚未上线**：还需要唯一获准登录邮箱、专用 Access 应用、域名 DNS 路由、生产进程及外网登录验收。
- 本次本机复查：Tunnel 配置与凭据文件存在，生产 `.env` 缺失；已新增 `scripts/service.sh` 统一管理命令。未修改 Cloudflare 资源，也未完成公网登录验收。
- 脚本本地验收已覆盖四个命令、重复启动/停止、启动会话退出后的后台运行、端口释放、单组件退出后的联动停止，以及通过 Vite 代理提交中文 prompt。生产配置缺失时已验证拒绝启动；生产整套启动和手机外网登录仍待配置完成后验收。
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

## 手机验收

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
