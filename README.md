# webhook-wecom

把外部 webhook 转成企业微信自建应用的纯文本通知，供成员在微信插件中接收。目前支持 Gitea 原生 webhook、ntfy 兼容发布和 MongoDB Atlas 默认 webhook。

## 手工部署到 Linux

目标机器需要 x86_64 Linux、systemd、curl 和 Caddy 2；不需要 Rust、glibc 或容器运行时。先按 [Caddy 官方安装文档](https://caddyserver.com/docs/install)安装包含 systemd 服务的发行版软件包。准备域名（下文以 `webhooks.example.com` 为例），将 DNS 指向目标机器，放行入站 TCP 80/443，并允许服务访问企业微信 HTTPS API。

先在企业微信管理后台创建自建应用，取得企业 ID、应用 AgentId 和 Secret，配置应用可见范围及接收人；需要在个人微信中收取时，还须由成员按企业微信指引绑定微信插件。若应用管理要求 API 可信 IP，填写服务器实际出口公网 IP；本服务不自动配置企业微信后台。

### 下载并安装

在 [GitHub Releases](https://github.com/karoyqiu/webhook-wecom/releases)选择已发布版本，将 `VERSION` 改为该 tag（不能保留占位符）。Release 附带二进制 `webhook-wecom-x86_64-unknown-linux-musl`、其 `SHA256SUMS`、`env.example`、`webhook-wecom.service` 和 `Caddyfile`。以下命令在一个新建的普通用户工作目录执行，后续升级也使用新的工作目录：

```sh
VERSION='vX.Y.Z'
mkdir "webhook-wecom-$VERSION"
cd "webhook-wecom-$VERSION"
BASE_URL="https://github.com/karoyqiu/webhook-wecom/releases/download/$VERSION"
for asset in webhook-wecom-x86_64-unknown-linux-musl SHA256SUMS env.example webhook-wecom.service Caddyfile; do
  curl --fail --location --output "$asset" "$BASE_URL/$asset" || exit 1
done
sha256sum --check SHA256SUMS || exit 1
# 仅在校验成功后继续安装。
sudo install -o root -g root -m 755 webhook-wecom-x86_64-unknown-linux-musl /usr/local/bin/webhook-wecom
```

### 配置服务和 HTTPS

创建配置文件后用 `sudoedit` 填写真实值。`env` 必须由 root 所有、权限 600；父目录权限 700，普通用户无法读取。不要将真实配置提交到仓库，也不要把它放进 Caddy 站点目录。systemd 以 root 读取文件后把配置交给动态用户，程序本身不需要读取该文件。

```sh
sudo install -d -o root -g root -m 700 /etc/webhook-wecom
sudo install -o root -g root -m 600 env.example /etc/webhook-wecom/env
sudoedit /etc/webhook-wecom/env
sudo install -o root -g root -m 644 webhook-wecom.service /etc/systemd/system/webhook-wecom.service
sudo systemd-analyze verify /etc/systemd/system/webhook-wecom.service
sudo systemctl daemon-reload
sudo systemctl enable --now webhook-wecom
curl --fail http://127.0.0.1:8080/health
sudo systemctl status webhook-wecom --no-pager
```

环境文件使用 `KEY=value` 格式，不写 `export`，不会展开 `$变量` 或执行 shell 命令；含特殊字符的值按 systemd `EnvironmentFile` 规则引用。必填变量和来源凭据见下文「本地运行」及 `env.example`。未配置凭据的来源返回 404。默认 `LISTEN_ADDR=127.0.0.1:8080`，无需对公网开放 8080；若更改监听地址，下面健康检查和 Caddy 代理目标也要同步修改。

编辑下载的 `Caddyfile`，将 `webhooks.example.com` 改为真实域名。新机器可直接安装；已有 Caddy 站点时，将示例的站点块合并到现有配置，保留原站点。

```sh
# 仅适用于新机器；已有配置时先备份并合并站点块。
sudo install -o root -g root -m 644 Caddyfile /etc/caddy/Caddyfile
sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile
sudo systemctl enable --now caddy
sudo systemctl reload caddy
curl --fail https://webhooks.example.com/health
sudo journalctl -u webhook-wecom -n 50 --no-pager
```

Caddy 自动申请和续期 TLS 证书。`/health` 返回 200 仅证明 HTTP 服务运行，不会调用企业微信；按下文配置来源后，用一次真实投递验证应用凭据、接收人及通知。服务以 `DynamicUser=yes` 运行，文件系统只读，失败自动重启；stdout/stderr 由 journald 收集。排错可用 `sudo journalctl -u webhook-wecom -f`。修改 `/etc/webhook-wecom/env` 后执行 `sudo systemctl restart webhook-wecom`。

### 升级与回滚

先在新的工作目录按「下载并安装」中的下载和校验命令获取目标版本，完成校验后再执行以下替换步骤。不要先执行首次安装命令覆盖正在使用的文件。保留旧版本作为回滚文件，以同目录重命名替换可执行文件，避免写入运行中的文件：

```sh
sudo cp -p /usr/local/bin/webhook-wecom /usr/local/bin/webhook-wecom.previous
sudo install -o root -g root -m 755 webhook-wecom-x86_64-unknown-linux-musl /usr/local/bin/webhook-wecom.new
sudo mv /usr/local/bin/webhook-wecom.new /usr/local/bin/webhook-wecom
sudo systemctl restart webhook-wecom
curl --fail http://127.0.0.1:8080/health
sudo journalctl -u webhook-wecom -n 50 --no-pager
```

升级失败时回滚二进制并重启：

```sh
sudo install -o root -g root -m 755 /usr/local/bin/webhook-wecom.previous /usr/local/bin/webhook-wecom.new
sudo mv /usr/local/bin/webhook-wecom.new /usr/local/bin/webhook-wecom
sudo systemctl restart webhook-wecom
curl --fail http://127.0.0.1:8080/health
```

没有持久化数据需要迁移。升级前阅读 Release 说明；如需更改环境文件、unit 或 Caddy 配置，先分别备份，回滚时一并恢复。unit 变更后运行 `systemctl daemon-reload`；Caddy 变更后先 validate 再 reload。重启期间可能有短暂投递失败，来源需自行重试或手动重发。

### 发布流程

维护者将 `v*` tag 推送到 GitHub 后，Actions 在 Ubuntu 上执行 fmt/clippy/test，使用 `cargo build --release --locked --target x86_64-unknown-linux-musl` 构建，再检查静态链接和无 ELF interpreter、在 Alpine 容器请求 `/health`、执行 `systemd-analyze verify` 与 `caddy validate`。全部成功才创建 Release，上传同一份已验证产物；CI 不登录服务器或自动部署。push 和 PR 也执行这些构建与部署材料检查，产物保存在名为 `webhook-wecom-x86_64-unknown-linux-musl` 的 Actions artifact 中。

包含 `-` 的 tag（如 `v0.0.0-spec1-test.1`）标记为预发布，显式不设为 Latest。正式发布使用如 `v0.1.0` 的版本 tag；二进制文件名不含版本号，下载 URL 由 tag 固定版本。

## 本地运行

安装当前稳定版 Rust。配置全部来自环境变量；程序不会自动加载 `.env` 文件。[`.env.example`](.env.example) 列出配置及说明。真实配置请保存在仓库外。

Linux/macOS shell 示例：

```sh
export WECOM_CORP_ID='your-corp-id'
export WECOM_AGENT_ID='1000002'
export WECOM_SECRET='your-app-secret'
export WECOM_TO_USER='your-user-id'
export GITEA_WEBHOOK_SECRET='your-webhook-secret'
cargo run --locked
```

PowerShell 可使用 `$env:WECOM_CORP_ID = 'your-corp-id'` 等方式逐项设置，再执行 `cargo run --locked`。

`WECOM_CORP_ID`、正整数 `WECOM_AGENT_ID`、`WECOM_SECRET` 必填；`WECOM_TO_USER`、`WECOM_TO_PARTY`、`WECOM_TO_TAG` 至少填写一种。空白值视为未配置。缺失或非法配置会导致启动失败，错误只包含变量名，不输出配置值。

默认监听 `127.0.0.1:8080`，可用 `LISTEN_ADDR` 修改。`WECOM_API_BASE` 默认为 `https://qyapi.weixin.qq.com`，测试时可指向假服务器的 HTTP 地址。生产使用官方 HTTPS 地址。公网访问需在服务前配置 HTTPS 反向代理。

## Gitea 配置

在仓库的「设置 → Webhooks → 添加 Webhook」中选择 **Gitea** 类型，目标地址填写 `https://你的域名/gitea`，请求方式 POST，内容类型 `application/json`，密钥与服务的 `GITEA_WEBHOOK_SECRET` 一致，并启用 webhook。不能选择其他服务的 webhook 格式。

事件可选择「所有事件」，或按需要勾选推送（`push`）、拉取请求（`pull_request`）、议题（`issues`）、评论（`issue_comment`）、发布（`release`）和工作流运行（`workflow_run`，需要 Gitea 支持此事件并启用 Actions）。保存后可用 Gitea 的测试投递及投递记录检查结果。

服务校验原始请求体的 HMAC-SHA256 十六进制 `X-Gitea-Signature`。缺少配置返回 404；签名错误或缺失返回 401。

标题格式为 `[仓库全名] N 次提交 -> 分支`，正文展示前五条提交首行和作者，最后一行保留 `compare_url`（没有链接时省略）。字段选择沿用 [gitea-ntfy 的 push 格式与测试样例](https://github.com/karoyqiu/gitea-ntfy/blob/develop/test/formatters.test.js)。计数取 `commits` 数组长度。

PR 和议题通知展示编号、动作、原始标题和作者，附原文链接；评论展示评论者和内容，附评论链接；发布展示动作、tag、名称和说明，附发布链接。评论及发布说明先截断至 300 个 Unicode 字符，超长附加 `…`。PR 关闭用 🔀，发布用 🚀 标记标题。工作流通知展示工作流名、运行编号、分支、触发方式和结论，附运行链接；成功用 ✅、失败用 ❌、其他状态用 ⚙️。

所有标题包含 `repository.full_name`。其他事件仍投递兜底通知，包含仓库、事件类型和触发人，没有链接。缺少可选字段时使用空值或中文占位，不拒绝请求。服务文案及已知动作、状态使用中文，来源标题、正文、分支等保持原文；未来新增的动作或状态保留原值。

通知按 UTF-8 字节计算，总长不超过 2048 字节，仅截断正文并附加 `…`。如果完整标题、分隔换行和链接已无法容纳，或正文截断后连省略号也放不下，返回 400，不投递。

`GET /health` 返回 200。请求体上限为 1 MiB（1,048,576 字节），超限返回 413；非法 JSON 返回 400。投递成功返回 204，下游失败返回 502。access_token 在内存缓存至过期；仅当消息接口返回 40014 或 42001 时，刷新并重试一次。没有队列或其他重试，请由来源决定是否重发。

每次请求在 stdout 写一行固定字段日志：`source=gitea event=push result=204`。不记录原始路径、查询、请求头、payload 或下游错误文本，避免泄露来源密钥、应用 secret 和 access_token。

## 开发

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

HTTP 集成测试在进程内运行服务及假企业微信服务器，不需要真实账号。测试包含全部 Gitea 事件类型和兜底、ntfy 文本/JSON/全部 header 别名/优先级/tags/鉴权/主题/非法字段、Atlas OPEN/CLOSED/可选字段/签名及非法输入、真实 HTTP 投递、正文限制、UTF-8 截断、token 缓存及一次刷新、下游失败、日志和环境变量启动检查。Gitea payload 样例取自 gitea-ntfy 测试，[来源及固定版本](tests/fixtures/README.md)随样例保存。

同步 HTTP 使用 [tiny_http](https://docs.rs/tiny_http/0.12.0/tiny_http/)、[ureq + rustls](https://docs.rs/ureq/3.5.2/ureq/)，无异步运行时或 OpenSSL。GitHub Actions 对 push 和 PR 执行上述检查。

企业微信官方参考：[发送应用消息](https://developer.work.weixin.qq.com/document/path/90236)、[获取 access_token](https://developer.work.weixin.qq.com/document/path/91039)、[全局错误码](https://developer.work.weixin.qq.com/document/path/90313)。实现期间官方页面无法访问，2048 字节规则及 40014/42001 处理遵循 spec #1，尚未用真实企业微信账号验证。

## ntfy 发布

设置服务环境变量 `NTFY_TOKEN` 为自行生成的随机令牌。未配置或空白时 `/ntfy/<主题>` 返回 404；缺少或错误的 `Authorization: Bearer <NTFY_TOKEN>` 返回 401，不投递。发布方配置完整地址 `https://你的域名/ntfy/backups` 和 Bearer 令牌。主题只用于标题，不改变接收人。

纯文本正文加 header：

```sh
curl -X POST 'https://你的域名/ntfy/backups' \
  -H "Authorization: Bearer $NTFY_TOKEN" \
  -H 'Title: Backup complete' \
  -H 'Priority: high' \
  -H 'Tags: tada,heavy_check_mark' \
  -H 'Click: https://example.com/backups' \
  --data-binary 'Backup completed successfully'
```

JSON 同样发布到 `/ntfy/<主题>`：

```sh
curl -X POST 'https://你的域名/ntfy/backups' \
  -H "Authorization: Bearer $NTFY_TOKEN" \
  -H 'Content-Type: application/json; charset=utf-8' \
  --data-binary '{"title":"备份失败","message":"请检查备份任务","priority":5,"tags":["warning"],"click":"https://example.com/backups"}'
```

JSON 读取 `title`、`message`、整数 `priority`（1–5）、字符串数组 `tags` 和 `click`。缺省值为空内容及优先级 3；支持字段类型错误、非法 JSON 或非 UTF-8 文本返回 400。Content-Type 忽略大小写并允许 charset 参数。JSON 中 `topic` 被忽略，主题始终取 URL；JSON 发布不读取文本格式的标题等 header。

文本格式支持全部常用别名：`X-Title` / `Title` / `ti` / `t`，`X-Priority` / `Priority` / `prio` / `p`，`X-Tags` / `Tags` / `tag` / `ta`，`X-Click` / `Click`。header 名忽略大小写；同组同时存在时按此顺序取第一个。优先级支持 1–5 及 `min`、`low`、`default`、`high`、`max` / `urgent`。tags 用逗号分隔。

标题为 `[主题] 标题`，无标题时只有 `[主题]`。优先级 4 加 ⚠️，5 加 🚨；可识别的 tags 依次转为 emoji 放在标题前，未知 tag 忽略。emoji 对照表完整固定在仓库中，[来源与许可证](src/data/README.md)随数据保存。附件、动作按钮、延迟、markdown 等不支持字段或 header 均忽略，仍立即投递纯文本。

仅支持 POST；没有 ntfy 原生根路径 JSON 发布、PUT、订阅或延迟发送接口。主题必须为非空单路径段；允许 URL 百分号编码的 UTF-8，不允许斜杠、反斜杠、空白、控制字符或 `.` / `..`，查询参数不进入主题。请求体、2048 字节通知限制及 204/400/413/502 结果规则与其他来源相同。日志使用固定的 `source=ntfy event=publish`，不记录主题或请求内容。

参考：[ntfy 发布文档](https://docs.ntfy.sh/publish/)、[emoji 列表](https://docs.ntfy.sh/emojis/)。

## MongoDB Atlas 配置

设置服务环境变量 `ATLAS_WEBHOOK_SECRET` 为自行生成的随机密钥。使用 Atlas 的 Organization Owner 或 Project Owner 账号，进入目标项目的 **Project Settings → Integrations → Webhook Settings → Configure**：Webhook URL 填 `https://你的域名/atlas`，Webhook Secret 填相同密钥，Headers Template 和 Body Template 保持默认，保存。随后在项目 **Alerts** 中创建或编辑告警规则，在通知方式中添加 **Webhook** 并保存。仅配置项目集成不会自动让所有告警都使用 webhook。

已于 2026-11-25 对照 [Atlas webhook 官方文档](https://www.mongodb.com/docs/atlas/tutorial/webhook-integration/)核实：Atlas 以 HTTP POST 投递，`X-MMS-Signature` 是使用 Webhook Secret 对**原始请求体字节**计算 HMAC-SHA1 后的 Base64 值；secret 不作为 Bearer 令牌发送。服务解码签名后用 HMAC 的常量时间校验拒绝伪造请求。未配置或空白密钥返回 404，签名缺失、非法或不匹配返回 401，不访问企业微信。

默认 JSON 包含 `status`、`eventTypeName`、`humanReadable`；官方 webhook 样例包含项目 ID `groupId`。完整字段结构见 [Atlas Alerts API](https://www.mongodb.com/docs/api/doc/atlas-admin-api-v2/operation/operation-listgroupalerts)：`groupId` 标识项目，`clusterName` 仅在适用的集群告警中返回，不能假定存在。自定义 body/header 模板可能改变结构或签名头，本服务只支持默认格式。

标题为 `[Atlas <status>] <eventTypeName>（<clusterName>）`，正文保留 `humanReadable`，末行链接为 `https://cloud.mongodb.com/v2/<groupId>#/alerts`。OPEN 和 CLOSED 都投递。缺少或 null 的状态、事件、集群分别显示「未知状态」「未知事件」「未知集群」，描述显示「无告警描述」；缺少、null 或空的 `groupId` 时省略链接。非法 JSON、非对象或上述字段为非字符串且非 null 时返回 400。1 MiB 请求体限制、2048 UTF-8 字节通知限制及 204/413/502 规则与其他来源相同。

Atlas 日志仅记录固定 `source=atlas`、白名单中的 `X-MMS-Event`（如 `alert.open`、`alert.close`，其他值记 `unknown`）和 HTTP 结果，不记录 payload 或密钥。通知方式操作见 [Atlas 告警配置文档](https://www.mongodb.com/docs/atlas/configure-alerts/)。Atlas 不提供手动测试告警；本仓库测试使用假下游，尚未修改或验证真实 Atlas 项目配置。
