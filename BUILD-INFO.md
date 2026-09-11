# AirPlay + Music + HA Client 构建记录

- 构建日期：2026-08-14（Asia/Shanghai）
- 上游项目：`idootop/open-xiaoai`，`packages/client-rust`
- 上游基线提交：`bc3396c64e2a435f354eb5cb12a203981f1fe422`
- 本版本：`open-xiaoai 1.4.0-airplay-music-ha.3`
- 目标设备：OH2P / Xiaomi 智能音箱 Pro
- Rust：1.96.0
- Zig：0.16.0
- 目标：`armv7-unknown-linux-gnueabihf.2.25`

构建命令：

```shell
cargo zigbuild --release \
  --target armv7-unknown-linux-gnueabihf.2.25 \
  --bin client
```

验证结果：

- ARM 测试目标成功编译；由于本机不是 ARM 环境，未执行 ARM 测试二进制。
- ARMv7/glibc 2.25 release 目标成功编译。
- ELF32、ARMv7、EABI5、little-endian、hard-float ABI。
- PIE，动态解释器 `/lib/ld-linux-armhf.so.3`。
- 最高引用符号版本为 `GLIBC_2.25`。
- 动态依赖仅为 `libm.so.6`、`libc.so.6`、`libpthread.so.0`、`libdl.so.2`。
- 发布文件：`client-airplay-music-ha-oh2p-armv7-glibc2.25-20260814`。
- 发布二进制大小：3,227,024 bytes。
- SHA-256：`38074109a9e3dc8239992ccc0422e7a4b8011004b6324aa5fcbd15b215c0d6c8`。
- QQ MusicU 搜索响应会校验顶层 `code` 和服务 `code`，避免把业务错误误报成无结果。
- 第一页返回空或缺失的歌曲列表时默认重试两次，每次间隔 500 ms；后续分页保持无结果即结束，避免无意义请求。
- 空结果重试会记录尝试次数、业务码、列表是否存在及 `qc` 建议词，不写入完整响应体。
- 队列只预取 QQ MID；播放 URL 按需解析并在内存中缓存。
- haitangw 主接口最小请求间隔 3 秒，失败后冷却 60 秒；冷却期间立即使用备用接口。
- 备用接口来自用户提供的 MusicFree QQ 插件，三首歌曲实测均返回有效 MP3 URL，音频地址支持 HTTP 206 分段读取。
- 主备接口均失败时保留当前 MID 并低频重试，默认最多重试 2 次，不遍历后续 MID。
- 新示例配置使用 OH2P 真机验证可用的原生 14 号灯效：AirPlay 或本地音乐播放时显示，暂停、停止或会话结束时关闭。
- 原来的 `L=8 + rgb` 自定义颜色在当前 OH2P `ledd` 上不生效，已从示例配置移除。
- 新增只读 UBUS 监听：实体播放键在本地音乐活跃时切换暂停/继续，空闲时保留小米原生行为。
- 新增 LED 14 恢复：小米 TTS 结束后若本地音乐仍在播放，观察到任意原生 LED 关闭动作都会自动重新启用灯效。
- UBUS 原始内容不会写入 Client 日志，避免复制临时设备 token。
- 新增 HA Conversation REST 路由：健康检查通过后才发布 ready 标记，最终 ASR 文本优先经过本地音乐规则，再提交 HA。
- 新增小爱回退：HA `no_intent_match`、鉴权失败和连接错误时通过 `nlp_text` 恢复原生问答；HA 请求超时默认不回退，避免不确定执行后的重复动作。
- 新增 fail-open 启动脚本：`/data/pns.lab` 指向 `/tmp` 运行时标记，断电、Client 退出或 HA 未就绪时均保持原生 NLP/TTS。
- HA 启动健康检查对 DNS、连接、超时和 HTTP 5xx 进行有限重试；默认 60 次、间隔 1 秒。
- `init-ha` 在 Client 整个生命周期持续等待 HA ready 标记，不再只等待 30 秒。
- 真机验证公网 HA 入口可达（未鉴权返回 HTTP 401，连接约 13 ms）；真机验证 TTS-only 会产生完整 `Dialog.Finish`。

本次构建未部署到 OH2P，因为设备当前不在本机可访问的局域网。该 Client 不修改固件分区、麦克风通道或 ALSA 配置。
