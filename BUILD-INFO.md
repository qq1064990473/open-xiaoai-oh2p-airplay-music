# AirPlay + Music Client 构建记录

- 构建日期：2026-08-03（Asia/Shanghai）
- 上游项目：`idootop/open-xiaoai`，`packages/client-rust`
- 上游基线提交：`bc3396c64e2a435f354eb5cb12a203981f1fe422`
- 本版本：`open-xiaoai 1.3.0-airplay-music-native-events.1`
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
- Windows GNU 测试二进制成功运行：11 passed、0 failed。
- ELF32、ARMv7、EABI5、little-endian、hard-float ABI。
- PIE，动态解释器 `/lib/ld-linux-armhf.so.3`。
- 最高引用符号版本为 `GLIBC_2.25`。
- 动态依赖仅为 `libm.so.6`、`libc.so.6`、`libpthread.so.0`、`libdl.so.2`。
- 发布文件：`client-airplay-music-native-events-oh2p-armv7-glibc2.25-20260803`。
- 发布二进制大小：3,171,896 bytes。
- SHA-256：`2be60f7593c1295818afe2e62665f4feab20ed4098ee1884d40790dfdacfed3e`。
- 队列只预取 QQ MID；播放 URL 按需解析并在内存中缓存。
- haitangw 主接口最小请求间隔 3 秒，失败后冷却 60 秒；冷却期间立即使用备用接口。
- 备用接口来自用户提供的 MusicFree QQ 插件，三首歌曲实测均返回有效 MP3 URL，音频地址支持 HTTP 206 分段读取。
- 主备接口均失败时保留当前 MID 并低频重试，默认最多重试 2 次，不遍历后续 MID。
- 新示例配置使用 OH2P 真机验证可用的原生 14 号灯效：AirPlay 或本地音乐播放时显示，暂停、停止或会话结束时关闭。
- 原来的 `L=8 + rgb` 自定义颜色在当前 OH2P `ledd` 上不生效，已从示例配置移除。
- 新增只读 UBUS 监听：实体播放键在本地音乐活跃时切换暂停/继续，空闲时保留小米原生行为。
- 新增 LED 14 恢复：小米 TTS 结束后若本地音乐仍在播放，观察到任意原生 LED 关闭动作都会自动重新启用灯效。
- UBUS 原始内容不会写入 Client 日志，避免复制临时设备 token。

当前验证属于本地构建、真实 UBUS 样本解析与静态检查。实体键分流、原生播放器清理和 TTS 后 LED 恢复仍需在 OH2P 上使用新文件名前台测试。该 Client 不修改固件分区、麦克风通道或 ALSA 配置。
