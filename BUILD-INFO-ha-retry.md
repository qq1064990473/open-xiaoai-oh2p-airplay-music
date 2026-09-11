# HA 启动重试版构建记录

- 构建日期：2026-08-06（Asia/Shanghai）
- 版本：`open-xiaoai 1.4.0-airplay-music-ha.2`
- 目标设备：OH2P / Xiaomi 智能音箱 Pro
- 目标：`armv7-unknown-linux-gnueabihf.2.25`
- 构建命令：`cargo zigbuild --release --target armv7-unknown-linux-gnueabihf.2.25 --bin client`
- 发布文件：`dist/client-airplay-music-ha-oh2p-armv7-glibc2.25-20260806`
- SHA-256：`b402726658ce41954dc94f7985788975bd751aab7c2dc30d6cae22a2fce94853`

## 本版修复

- HA 启动健康检查对 DNS、连接、超时和 HTTP 5xx 进行有限重试。
- 默认最多 60 次、间隔 1 秒；可通过 `home_assistant.startup_retry_attempts` 和 `startup_retry_interval_ms` 调整。
- HTTP 401/403、其他配置错误和空 token 不重试，仍保持 fail-open。
- `init-ha` 在 Client 的整个生命周期持续等待就绪标记，网络晚于进程启动时也能切换到原生 ASR-only 模式。
- HA 不可达时，音乐、AirPlay 和原生小爱继续按 fail-open 逻辑运行。
