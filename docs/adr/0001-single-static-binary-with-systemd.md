# 用 Rust 静态二进制加 systemd 部署，而不是 Node 容器

服务用 Rust 编写，编译为 `x86_64-unknown-linux-musl` 静态二进制，部署时复制一个文件，由 systemd 管理，配置通过 `EnvironmentFile` 传入。前身 gitea-ntfy 用的是零依赖 Node 加容器，这里没有沿用：目标机器不和其他项目共用，也不走 deploy-pattern，为一个小服务装容器运行时和 Node 镜像不划算；静态二进制不受目标发行版 glibc 版本影响，升级和回滚就是换一个文件。

## Consequences

- 放弃 gitea-ntfy 的零依赖做法：Rust 标准库没有 HTTP 服务端和 TLS 客户端，依赖限定为一组同步的小 crate（`tiny_http`、`ureq` + rustls、`serde_json`、`hmac`/`sha2`/`sha1`/`base64`），不引入 tokio，也不依赖 OpenSSL。
- Windows 上交叉编译 musl 麻烦，构建交给 GitHub Actions：打 tag 时编译，产物挂到 GitHub Release。
- gitea-ntfy 的 Gitea 格式化逻辑需要从 JavaScript 移植过来，不能共享代码。

## Considered

- **Node（TypeScript）容器，照搬 gitea-ntfy**：能复用格式化代码，但部署要容器运行时和镜像构建，比复制一个二进制重。
- **Node 容器走 deploy-pattern**：适合与其他项目共用的主机，这台机器不是。
