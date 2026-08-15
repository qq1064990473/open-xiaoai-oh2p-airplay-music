# Open-XiaoAI OH2P AirPlay + Music + HA Client

基于 [idootop/open-xiaoai](https://github.com/idootop/open-xiaoai) `packages/client-rust` 的 OH2P 本地版本，为 Xiaomi 智能音箱 Pro 增加 AirPlay 1、固定词音乐播放、播放队列、LED 状态灯和 Home Assistant Conversation 路由。音乐及 HA 控制都不依赖 LLM。

## 功能

- AirPlay 服务独立于 WebSocket 重连循环，Server 断线不影响 AirPlay。
- AIVS 最终 ASR 固定规则识别歌曲、歌手、歌单及播放控制。
- 支持下一首、上一首、暂停、恢复、停止、随机播放、单曲循环、列表循环和自然结束续播。
- QQ Music 搜索只提前缓存歌曲 MID；歌曲即将播放时才解析 URL。
- QQ Music 首页搜索若收到 HTTP 200 但歌曲列表为空或缺失，会进行两次有限重试，并校验顶层及服务业务码。
- URL 按 MID 在内存中缓存，上一首、循环和短时间重播不重复取链。
- haitangw 主接口带最小访问间隔和失败冷却；失败时回退到外置 MusicFree QQ 接口。
- 主备接口均失败时保留当前 MID，低频有限重试，不扫描后续队列。
- AirPlay 与本地音乐互斥仲裁，小爱唤醒期间自动暂停或降低音量。
- AirPlay 与本地音乐播放使用 OH2P 原生 14 号灯效，暂停或停止时关闭。
- 实体播放键在本地音乐活跃时控制暂停/继续，其他时间保留小米原生行为。
- 小米 TTS 结束覆盖 LED 后，自动恢复仍在播放的网络音乐灯效。
- 复用小米原生唤醒、AEC、降噪、VAD 和云端 ASR，将最终文字直接提交给 HA Conversation。
- HA 无法匹配、鉴权失败或连接失败时，将原 ASR 文字回退给小爱原生 NLP；超时默认不重复执行，避免同一设备动作触发两次。
- HA 鉴权成功前不进入 ASR-only；Client 异常退出或重启后自动恢复原生小爱，避免音箱失去应答能力。
- 开机时 HA 健康检查会重试暂时性的 DNS、连接、超时和 HTTP 5xx，避免网络尚未就绪导致 HA 在本进程内永久禁用。
- 不修改固件分区、麦克风通道或 ALSA 配置。

## 下载

OH2P / ARMv7 / glibc 2.25 二进制：

```text
dist/client-airplay-music-ha-oh2p-armv7-glibc2.25-20260814
```

SHA-256：

```text
38074109a9e3dc8239992ccc0422e7a4b8011004b6324aa5fcbd15b215c0d6c8
```

HA 版本的启用、测试和回滚步骤见 [DEPLOY-HA-OH2P.md](DEPLOY-HA-OH2P.md)。旧媒体版本的手动部署说明仍保留在 [DEPLOY-OH2P.md](DEPLOY-OH2P.md)。

`client.example.json` 内含可被 Client 安全忽略的中文 `_说明` 字段，仍是可直接使用的标准 JSON。通过 SSH 注入最终 ASR 文字进行远程分路测试的方法见 [SSH-ROUTING-TEST.md](SSH-ROUTING-TEST.md)。

## 配置

将 `client.example.json` 上传为 `/data/open-xiaoai/client.json`，按需修改 AirPlay 名称、硬件地址、音乐参数和 `home_assistant`：

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

只有本地音乐会话活跃时，短控制词才会被音乐路由接管。HA 就绪时，其余最终 ASR 文本先交给 HA Conversation；HA 无法匹配时再交给原生小爱。

HA 长期访问令牌只存放在音箱的 `/data/open-xiaoai/ha.token`，不要写进 JSON 或提交到 Git。`init-ha.example.sh` 只有在 Client 使用该令牌成功访问 `/api/` 后，才会创建运行时 ASR-only 标记并重启 AIVS。HA 返回 `no_intent_match` 时，Client 使用 `mibrain ai_service` 把原文字交还给小爱。

## 音乐接口

QQ Music MusicU 只用于搜索 MID。播放 URL 获取顺序：

```text
内存 URL 缓存 -> haitangw 主接口 -> MusicFree 备用接口
```

备用接口来自 `https://13413.kstore.vip/yuanli/qq.js`。其取链接口是第三方明文 HTTP 服务，返回的实际媒体 URL 通常为 HTTPS。它可能随时失效，也存在响应被篡改的风险；不接受该信任边界时，将 `music.play_url.backup_enabled` 设为 `false`。

MusicU 请求使用 `music.search.SearchCgiService / DoSearchForQQMusicDesktop`，参数为 `query`、`page_num`、`num_per_page` 和 `search_type`。该结构与 [MergeMusicDesktop 的公开实现](https://github.com/flwfdd/MergeMusicDesktop/blob/02ca7b89d96199ec51d3a7a7383c2d33686fd1d5/src/main/java/xyz/flwfdd/mergemusicdesktop/music/QQMusic.java#L70) 一致，本机实时请求也返回了有效歌曲列表。它属于 QQ 音乐内部接口，并非有稳定性承诺的官方开放 API；相关结论和降级策略见 [QQ-MUSIC-API-NOTES.md](QQ-MUSIC-API-NOTES.md)。

## LED

示例配置针对已验证的 OH2P 原生 14 号灯效：

```shell
/bin/show_led 14
/bin/shut_led 14
```

AirPlay 开始或本地音乐播放时显示 14 号原生灯效，本地音乐暂停、停止或 AirPlay 会话结束时关闭该灯效。若小米 TTS 在播报结束时关闭 `L=14`，Client 会在确认网络音乐仍为播放状态后恢复灯效。当前 OH2P 的 `ledd` 不支持原示例使用的 `L=8 + rgb` 自定义颜色，因此本版不再按 PCM 音量分级切换颜色。所有命令仍保留在外置配置文件中。

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
- 公网 `http://` HA 地址会以明文传输 Bearer token。生产使用应改为 HTTPS、WireGuard/Tailscale 等私网入口，并将 `allow_insecure_http` 恢复为 `false`。
- 本项目仅供个人设备研究和测试，请遵守音乐内容及第三方服务的许可要求。

## 上游与许可

上游项目：[idootop/open-xiaoai](https://github.com/idootop/open-xiaoai)，基线提交 `bc3396c64e2a435f354eb5cb12a203981f1fe422`。

项目沿用上游 MIT License，第三方组件说明见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。
