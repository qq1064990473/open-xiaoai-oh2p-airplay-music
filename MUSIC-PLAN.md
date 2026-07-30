# 固定词音乐实现说明

## 数据流

1. Client 监听 `/tmp/mico_aivs_lab/instruction.log`，按 `dialog_id` 去重。
2. 优先采用 `SpeechRecognizer.RecognizeResult` 中 `is_nlp_request=true` 且 `is_stop=true` 的文本；`is_final=true` 仅作为后备。
3. 固定词解析器识别单曲、歌手、歌单、随机播放和播放控制，不调用 LLM。
4. 命中后执行外置的 `native_stop_command`，并继续压制同一对话稍后到达的原生 `AudioPlayer.Play/MUSIC`、`PlaybackController` 和 `SpeechSynthesizer.Speak*`，避免双播和会员提示。
5. QQ Music MusicU 接口提前构建 MID 队列，但不预取播放 URL；歌曲即将播放时才访问主播放接口。
6. 播放 URL 按 MID 缓存；上一首、循环和短时间重播优先复用缓存。缓存播放异常时自动失效。
7. 主接口带最小请求间隔和失败冷却，不连续访问 haitangw；主接口失败或冷却时立即使用已验证的 MusicFree QQ 备用接口。两个接口都失败时才进入队列失败策略。
8. Client 启动并持有 `/usr/bin/miplayer -f URL` 子进程。子进程退出作为歌曲自然结束依据，然后推进队列。

## 状态与可靠性

- 每次播放分配 generation，旧播放器的退出事件不能影响新歌曲。
- 切歌、替换队列、上一首和停止都有独立结束原因，不会被误判为自然播放完成。
- 暂停和恢复使用持有 PID 的 `SIGSTOP` / `SIGCONT`；停止先 `SIGTERM`，超时后才 `SIGKILL`。
- 不使用 `killall miplayer`，避免误杀原生或其他进程。
- URL 失败按配置跳过，连续失败达到上限后停止，避免无限循环。
- 单曲默认补充同歌手队列；歌手结果会检查全部 artist 字段；MID 用于去重。
- 单曲搜索的第一条结果立即进入播放流程，同歌手 MID 队列在后台补充，不阻塞起播。
- `primary_cooldown_retries` 默认是 2：首次失败后最多低频重试两次，仍失败则停止当前队列，避免长期访问第三方接口。
- URL 缓存仅保存在内存中，默认有效 1800 秒、最多 128 条；Client 重启后自动清空，不写入音箱存储。
- MusicFree 备用接口来自 `https://13413.kstore.vip/yuanli/qq.js`，实际取链端点为明文 HTTP；它只在主接口失败或冷却时调用，返回的媒体 URL 仍会进入相同缓存。

## 音频仲裁

- AirPlay 开始：默认暂停本地音乐。
- AirPlay 结束：只恢复因 AirPlay 暂停的音乐。
- AirPlay 活跃时的新音乐请求：默认拒绝，不抢占 AirPlay。
- 小爱唤醒：默认暂停本地音乐；AirPlay 根据配置 duck 或 mute。
- `Dialog.Finish`：恢复因本次唤醒暂停的音源。
- 唤醒期间切换的新歌曲继承暂停状态，不会在 `Dialog.Finish` 前提前播放。

## LED

AirPlay 回调可直接获得 PCM，因此以 RMS/dBFS、attack/release 平滑后映射到青蓝色 `led.level_commands`。Shell/UBUS 命令由 6 Hz LED 任务执行，不在 PCM 回调执行。

本地音乐继续用系统 `miplayer`，Client 无法读取其 PCM，因此使用静态状态色：播放为绿色、暂停为琥珀色、停止关闭编号 8 的自定义灯效。Rust 兼容默认值保持关闭，新 OH2P 示例配置启用该功能。

## 当前验证边界

固定词解析、AIVS 最终文本提取、配置默认值、QQ JSON 解析和 AirPlay 辅助函数已有单元测试。ARMv7/glibc 2.25 已完成静态构建验证；QQ 接口可用性、`miplayer` 网络播放、原生迟到指令拦截和 LED 命令仍需在音箱上前台验证。
