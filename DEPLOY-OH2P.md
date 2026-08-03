# OH2P AirPlay + Music Client 安全部署与测试

该产物只是 `/data/open-xiaoai/client` 用户态程序，不修改分区、启动镜像、麦克风通道或 ALSA 配置，不需要重新刷固件。建议先以新文件名运行，确认 AirPlay 和音乐路由后再替换当前 Client。

## 1. 上传到临时文件名

Windows PowerShell 中执行（按实际 IP 修改）：

```powershell
scp -O `
  -o HostKeyAlgorithms=+ssh-rsa `
  -o PubkeyAcceptedAlgorithms=+ssh-rsa `
  "C:\Users\Administrator\Desktop\open-xiaoai-修改版本\workspace\open-xiaoai-client-airplay\dist\client-airplay-music-native-events-oh2p-armv7-glibc2.25-20260803" `
  root@192.168.0.122:/data/open-xiaoai/client-media.new

scp -O `
  -o HostKeyAlgorithms=+ssh-rsa `
  -o PubkeyAcceptedAlgorithms=+ssh-rsa `
  "C:\Users\Administrator\Desktop\open-xiaoai-修改版本\workspace\open-xiaoai-client-airplay\dist\client-airplay-music-native-events.example.json" `
  root@192.168.0.122:/data/open-xiaoai/client-native-events.example.json
```

不要直接覆盖已经调好的 `client.json`。修改前先备份：

```shell
cp -p /data/open-xiaoai/client.json /data/open-xiaoai/client.json.before-native-events
```

从新示例配置合并完整的 `native_events` 对象，并同步 `music.player.native_stop_command`。后者新增了对 `media:"common"` 的显式暂停，用于清理实体键触发的原生播放。

## 2. 在音箱上核对，不替换旧程序

```shell
chmod +x /data/open-xiaoai/client-media.new
sha256sum /data/open-xiaoai/client-media.new
/data/open-xiaoai/client-media.new --help
```

预期 SHA-256：

```text
2be60f7593c1295818afe2e62665f4feab20ed4098ee1884d40790dfdacfed3e
```

先查看 5000 端口是否被占用：

```shell
netstat -lntp 2>/dev/null | grep ':5000 '
```

若已占用，先修改 `client.json` 的 `airplay.port`，不要直接结束未知系统进程。

## 3. 前台测试 AirPlay

保持现有 Client 不变，先手动运行新程序；WebSocket 地址不可用也不影响 AirPlay 服务持续运行：

```shell
/data/open-xiaoai/client-media.new \
  ws://127.0.0.1:4399 \
  -c /data/open-xiaoai/client.json
```

正常启动应看到：

```text
[config] loading /data/open-xiaoai/client.json
[airplay] AP1 receiver started: ...
[native-events] monitoring OH2P touchpad and LED UBUS traffic
```

随后在 iPhone/iPad 的 AirPlay 输出列表中选择 `Xiaomi 智能音箱 Pro AirPlay`。首次测试时观察：

- 能否发现设备；
- 建立连接时是否打印 `client connected`；
- 是否启动 `/usr/bin/aplay`；
- 播放、调音量、停止后是否正常结束会话；
- 原生小爱唤醒和回答是否仍正常。

然后依次测试 `播放周杰伦的晴天`、`下一首`、`暂停`、`继续播放`、歌曲自然结束和播放中唤醒。观察 `[routing]`、`[music]` 日志，并确认不会同时启动原生音乐。

新示例配置已经启用 LED。预期 AirPlay 或本地音乐播放时显示原生 14 号灯效，本地音乐暂停、停止或 AirPlay 会话结束时关闭。若希望先隔离测试音乐接口，可临时将 `led.enabled` 改为 `false`，无需更换二进制。

## 4. 测试实体键与 LED 恢复

先通过语音播放一首网络音乐，等待小米播报结束。若播报结束时原生系统关闭 LED，Client 应输出：

```text
[native-events] restoring music LED after native shut
```

随后短按两次实体播放键，第一次暂停、第二次继续。预期依次输出：

```text
[native-events] hardware play key -> Pause
[native-events] hardware play key -> Resume
```

确认暂停时网络音乐停止且不会启动小米原生歌曲，继续时只恢复当前网络音乐。停止网络音乐后再按实体键，原生功能应保持不变。

备用 API 自身使用明文 HTTP，只在 haitangw 失败或冷却时调用，返回的实际音频 URL 为 HTTPS。若不接受该信任边界，将 `music.play_url.backup_enabled` 改为 `false`。

若设备无法被发现，先保留完整日志，再核对 UDP 5353、原生 `mdnsd` 和局域网 AP 隔离；不要先修改固件。

## 5. 确认后再替换及回滚

确认测试正常后再备份旧 Client：

```shell
cp -p /data/open-xiaoai/client /data/open-xiaoai/client.before-media
mv /data/open-xiaoai/client-media.new /data/open-xiaoai/client
chmod +x /data/open-xiaoai/client
```

若需要回滚：

```shell
cp -p /data/open-xiaoai/client.before-media /data/open-xiaoai/client
chmod +x /data/open-xiaoai/client
```
