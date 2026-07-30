# 第三方组件说明

本工作副本的上游 `open-xiaoai` Client 使用 MIT License。

新增 AirPlay 实现依赖：

- `shairplay` 0.7.0，LGPL-3.0-or-later
- 项目地址：<https://github.com/fabianlindfors/shairplay>

新增音乐 HTTP/TLS 实现使用 `reqwest`、`rustls`、`webpki-roots` 及其传递依赖，这些组件采用 MIT、Apache-2.0 或 ISC 等兼容的宽松许可证；准确版本以 `Cargo.lock` 为准。

因此在公开分发二进制前，应同时保留本修改版本源码、依赖版本和相应许可证信息，并按 LGPL-3.0-or-later 的要求处理再链接与源码提供义务。本工作区已保留完整 Rust 源码和 `Cargo.lock`，便于复现构建。
