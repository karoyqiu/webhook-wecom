# webhook-wecom

把外部 webhook 转成企业微信自建应用的纯文本通知，供成员在微信插件中接收。目前支持 Gitea 原生 `push` 事件；其他 Gitea 事件、ntfy、Atlas 和部署材料将在后续 ticket 实现。

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

## Gitea push

在 Gitea 仓库添加原生 Gitea webhook：地址 `https://你的域名/gitea`，请求方式 POST，内容类型 JSON，密钥与 `GITEA_WEBHOOK_SECRET` 一致，选择 push 事件。服务校验原始请求体的 HMAC-SHA256 十六进制 `X-Gitea-Signature`。缺少配置返回 404；签名错误或缺失返回 401。未实现的事件暂时返回 400。

标题格式为 `[仓库全名] N 次提交 -> 分支`，正文展示前五条提交首行和作者，最后一行保留 `compare_url`（没有链接时省略）。字段选择沿用 [gitea-ntfy 的 push 格式与测试样例](https://github.com/karoyqiu/gitea-ntfy/blob/develop/test/formatters.test.js)。计数取 `commits` 数组长度。

通知按 UTF-8 字节计算，总长不超过 2048 字节，仅截断正文并附加 `…`。如果完整标题、分隔换行和链接已无法容纳，或正文截断后连省略号也放不下，返回 400，不投递。

`GET /health` 返回 200。请求体上限为 1 MiB（1,048,576 字节），超限返回 413；非法 JSON 返回 400。投递成功返回 204，下游失败返回 502。access_token 在内存缓存至过期；仅当消息接口返回 40014 或 42001 时，刷新并重试一次。没有队列或其他重试，请由来源决定是否重发。

每次请求在 stdout 写一行固定字段日志：`source=gitea event=push result=204`。不记录原始路径、查询、请求头、payload 或下游错误文本，避免泄露来源密钥、应用 secret 和 access_token。

## 开发

```sh
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

HTTP 集成测试在进程内运行服务及假企业微信服务器，不需要真实账号。测试包含真实 HTTP 投递、签名、正文限制、UTF-8 截断、token 缓存及一次刷新、下游失败、日志和环境变量启动检查。push 样例来自 gitea-ntfy develop 分支 `test/formatters.test.js` 的 push case。

同步 HTTP 使用 [tiny_http](https://docs.rs/tiny_http/0.12.0/tiny_http/)、[ureq + rustls](https://docs.rs/ureq/3.5.2/ureq/)，无异步运行时或 OpenSSL。GitHub Actions 对 push 和 PR 执行上述检查。

企业微信官方参考：[发送应用消息](https://developer.work.weixin.qq.com/document/path/90236)、[获取 access_token](https://developer.work.weixin.qq.com/document/path/91039)、[全局错误码](https://developer.work.weixin.qq.com/document/path/90313)。实现期间官方页面无法访问，2048 字节规则及 40014/42001 处理遵循 spec #1，尚未用真实企业微信账号验证。
