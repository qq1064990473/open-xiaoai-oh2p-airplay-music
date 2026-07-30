# Open-XiaoAI OH2P AirPlay + Music Client

基于 [idootop/open-xiaoai](https://github.com/idootop/open-xiaoai) `packages/client-rust` 的 OH2P 本地媒体版本，为 Xiaomi 智能音箱 Pro 增加 AirPlay 1、固定词音乐播放、播放队列和 LED 状态灯。音乐指令不依赖 LLM。

## 功能

- AirPlay 服务独立于 WebSocket 重连循环，Server 断线不影响 AirPlay。
- AIVS 最终 ASR 固定规则识别歌曲、歌手、歌单及播放控制。
- 支持下一首、上一首、暂停、恢复、停止、随机播放、单曲循环、列表循环和自然结束续播。
- QQ Music 搜索只提前缓存歌曲 MID；歌曲即将播放时才解析 URL。
- URL 按 MID 在内存中缓存，上一首、循环和短时间重播不重复取链。
- haitangw 主接口带最小访问间隔和失败冷却；失败时回退到外置 MusicFree QQ 接口。
- 主备接口均失败时保留当前 MID，低频有限重试，不扫描后续队列。
- AirPlay 与本地音乐互斥仲裁，小爱唤醒期间自动暂停或降低音量。
- AirPlay 使用青蓝色 PCM 音量灯效；本地音乐播放为绿色、暂停为琥珀色。
- 不修改固件分区、麦克风通道或 ALSA 配置。

## 下载

OH2P / ARMv7 / glibc 2.25 二进制：

```text
dist/client-airplay-music-backup-led-oh2p-armv7-glibc2.25-20260729
```

SHA-256：

```text
2c20c5205f68864a64c776a8c0dc7ab02228a0c1ae55b618c717ece0c208c9c8
```

完整上传、前台测试和回滚步骤见 [DEPLOY-OH2P.md](DEPLOY-OH2P.md)。

## 配置

将 `client.example.json` 上传为 `/data/open-xiaoai/client.json`，按需修改 AirPlay 名称、硬件地址和音乐参数：

```shell
/data/open-xiaoai/client-media.new \
  ws://127.0.0.1:4399 \
  -c /data/open-xiaoai/client.json
```

常用语音命令：

- `播放周杰伦的晴天`
- `播放周杰伦的歌`
- `播放歌单华语经典`
- `随机播放`
- `下一首`、`上一首`、`暂停`、`继续播放`、`停止播放`

只有本地音乐会话活跃时，短控制词才会被 Client 接管；其他问答继续交给原生小爱。

## 音乐接口

QQ Music MusicU 只用于搜索 MID。播放 URL 获取顺序：

```text
内存 URL 缓存 -> haitangw 主接口 -> MusicFree 备用接口
```

备用接口来自 `https://13413.kstore.vip/yuanli/qq.js`。其取链接口是第三方明文 HTTP 服务，返回的实际媒体 URL 通常为 HTTPS。它可能随时失效，也存在响应被篡改的风险；不接受该信任边界时，将 `music.play_url.backup_enabled` 设为 `false`。

## LED

示例配置针对 OH2P 使用：

```shell
/bin/show_led 8 RRGGBB
/bin/shut_led 8
```

AirPlay 能直接读取 PCM，因此按 RMS/dBFS 映射为六档青蓝色亮度。本地音乐由系统 `miplayer` 播放，Client 无法读取其 PCM，只显示播放、暂停和停止状态色。所有颜色均在配置文件中外置。

## 构建

本发布版使用 Rust 1.96、Zig 0.16 和 `cargo-zigbuild`：

```shell
cargo zigbuild --release \
  --target armv7-unknown-linux-gnueabihf.2.25 \
  --bin client
```

已验证 ELF32 ARM、EABI5 hard-float、解释器 `/lib/ld-linux-armhf.so.3`，最高引用 `GLIBC_2.25`。构建和静态检查记录见 [BUILD-INFO.md](BUILD-INFO.md)，实现说明见 [MUSIC-PLAN.md](MUSIC-PLAN.md)。

## 安全

- 先用新文件名前台运行，确认功能后再替换现有 Client。
- 不要把小米账号、pasSToken、AirPlay 密码或其他密钥提交到配置文件。
- WebSocket Server 可要求 Client 执行设备端操作，只连接可信地址。
- 本项目仅供个人设备研究和测试，请遵守音乐内容及第三方服务的许可要求。

## 上游与许可

上游项目：[idootop/open-xiaoai](https://github.com/idootop/open-xiaoai)，基线提交 `bc3396c64e2a435f354eb5cb12a203981f1fe422`。

项目沿用上游 MIT License，第三方组件说明见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
