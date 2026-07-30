# AirPlay + Music Client 构建记录

- 构建日期：2026-07-29（Asia/Shanghai）
- 上游项目：`idootop/open-xiaoai`，`packages/client-rust`
- 上游基线提交：`bc3396c64e2a435f354eb5cb12a203981f1fe422`
- 本版本：`open-xiaoai 1.2.0-airplay-music.2`
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

- 全部 ARM 目标成功编译。
- Windows GNU 测试二进制成功运行：9 passed、0 failed。
- ELF32、ARMv7、EABI5、little-endian、hard-float ABI。
- PIE，动态解释器 `/lib/ld-linux-armhf.so.3`。
- 最高引用符号版本为 `GLIBC_2.25`。
- 动态依赖仅为 `libm.so.6`、`libc.so.6`、`libpthread.so.0`、`libdl.so.2`。
- 发布文件：`client-airplay-music-backup-led-oh2p-armv7-glibc2.25-20260729`。
- 发布二进制大小：3,142,520 bytes。
- SHA-256：`2c20c5205f68864a64c776a8c0dc7ab02228a0c1ae55b618c717ece0c208c9c8`。
- 队列只预取 QQ MID；播放 URL 按需解析并在内存中缓存。
- haitangw 主接口最小请求间隔 3 秒，失败后冷却 60 秒；冷却期间立即使用备用接口。
- 备用接口来自用户提供的 MusicFree QQ 插件，三首歌曲实测均返回有效 MP3 URL，音频地址支持 HTTP 206 分段读取。
- 主备接口均失败时保留当前 MID 并低频重试，默认最多重试 2 次，不遍历后续 MID。
- 新示例配置启用 OH2P 自定义 LED：AirPlay 青蓝色音量分级、本地音乐绿色、暂停琥珀色。

当前验证属于本地构建、接口探测与静态检查。AIVS 实时拦截、`miplayer` 播放备用 URL、AirPlay/音乐仲裁和 LED 颜色仍需在 OH2P 上使用新文件名前台测试。该 Client 不修改固件分区、麦克风通道或 ALSA 配置。
