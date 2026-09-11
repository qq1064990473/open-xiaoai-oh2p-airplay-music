# 第三方组件说明

音乐功能默认使用酷我公开搜索端点 `https://search.kuwo.cn/r.s`，并以可配置的
`https://musicapi.haitangw.net/music/kw.php`（长青酷我）和
`http://music.nxinxz.com/kw.php`（念心酷我）作为直链解析源。上述服务均非本项目运营；
部署者须自行确认其可用性、服务条款、版权状态与使用许可。酷我搜索返回的
`MUSIC_<RID>` 会被转换为纯数字 RID 后才发送给酷我直链源，绝不会发送到 QQ 解析源。

本工作副本的上游 `open-xiaoai` Client 使用 MIT License。

新增 AirPlay 实现依赖：

- `shairplay` 0.7.0，LGPL-3.0-or-later
- 项目地址：<https://github.com/fabianlindfors/shairplay>

新增音乐 HTTP/TLS 实现使用 `reqwest`、`rustls`、`webpki-roots` 及其传递依赖，这些组件采用 MIT、Apache-2.0 或 ISC 等兼容的宽松许可证；准确版本以 `Cargo.lock` 为准。

因此在公开分发二进制前，应同时保留本修改版本源码、依赖版本和相应许可证信息，并按 LGPL-3.0-or-later 的要求处理再链接与源码提供义务。本工作区已保留完整 Rust 源码和 `Cargo.lock`，便于复现构建。
