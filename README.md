# webhook-wecom

把外部 webhook 转成企业微信自建应用的纯文本通知，供成员在微信插件中接收。目前支持 Gitea 原生 webhook 和 ntfy 兼容发布；Atlas 和部署材料将在后续 ticket 实现。

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

HTTP 集成测试在进程内运行服务及假企业微信服务器，不需要真实账号。测试包含全部 Gitea 事件类型和兜底、ntfy 文本/JSON/全部 header 别名/优先级/tags/鉴权/主题/非法字段、可选字段、真实 HTTP 投递、签名、正文限制、UTF-8 截断、token 缓存及一次刷新、下游失败、日志和环境变量启动检查。Gitea payload 样例取自 gitea-ntfy 测试，[来源及固定版本](tests/fixtures/README.md)随样例保存。

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
