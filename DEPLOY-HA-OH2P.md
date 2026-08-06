# OH2P Home Assistant 路由部署

本版本只替换 `/data` 中的用户态 Client、JSON 配置和启动脚本，不修改固件分区、麦克风通道或 ALSA。启动脚本采用 fail-open 设计：HA token 缺失、鉴权失败或 Client 退出时，音箱继续使用原生小爱 NLP/TTS。

## 文件

- Client：`dist/client-airplay-music-ha-oh2p-armv7-glibc2.25-20260806`
- 示例配置：`dist/client-airplay-music-ha-retry.example.json`
- 启动脚本：`dist/init-ha-oh2p-20260806.sh`
- SHA-256：`b402726658ce41954dc94f7985788975bd751aab7c2dc30d6cae22a2fce94853`

目标路径：

```text
/data/open-xiaoai/client-ha.new
/data/open-xiaoai/client.controlfix.json
/data/init.sh
```

## HA token

在 HA 用户资料页创建长期访问令牌。为避免令牌进入 shell 历史，在音箱终端中关闭回显后写入独立文件：

```shell
umask 077
stty -echo
printf 'HA token: ' >&2
IFS= read -r HA_TOKEN
stty echo
printf '\n' >&2
printf '%s\n' "$HA_TOKEN" > /data/open-xiaoai/ha.token
unset HA_TOKEN
chmod 600 /data/open-xiaoai/ha.token
```

不要把 token 写进 JSON、日志或 Git。配置完成后结束 Client，`init.sh` 监督循环会在 3 秒后重新启动并鉴权：

```shell
kill "$(pidof client-ha.new)"
sleep 6
tail -n 50 /tmp/open-xiaoai-client.log
```

正常状态应包含 `[ha] authenticated and ready`，并同时存在：

```text
/tmp/open-xiaoai-ha-ready
/tmp/open-xiaoai-pns.lab
```

## 测试

依次验证：

1. `打开次卧台灯`：由 HA Conversation 执行并用小米原生 TTS 播报 HA 回答。
2. `今天天气怎么样`：若 HA 不匹配，应回退到原生小爱回答。
3. `播放周杰伦的晴天`、下一首、暂停、继续：仍由本地音乐固定规则处理。
4. AirPlay 连接、播放和断开：端口 5000 及 LED 行为保持原版本逻辑。
5. 手动结束 `client-ha.new`：运行时 lab 标记应被删除，AIVS 重启恢复原生模式，随后 Client 自动拉起。

## 回滚

部署前的设备备份：

```text
/data/init.sh.before-ha-20260805-1038
/data/open-xiaoai/client.controlfix.json.before-ha-20260805-1038
```

恢复文件后删除运行时标记并重启设备：

```shell
cp -p /data/init.sh.before-ha-20260805-1038 /data/init.sh
cp -p /data/open-xiaoai/client.controlfix.json.before-ha-20260805-1038 \
  /data/open-xiaoai/client.controlfix.json
rm -f /tmp/open-xiaoai-pns.lab /tmp/open-xiaoai-ha-ready /data/pns.lab
chmod 755 /data/init.sh
/etc/init.d/mico_aivs_lab restart
reboot
```

## 公网 HTTP 风险

`http://nas.casperteng.com:8123` 会以明文发送 HA Bearer token，互联网链路上的中间节点可能读取并复用该 token。该配置仅用于用户明确要求的临时测试。长期使用应配置 HTTPS 或 WireGuard/Tailscale 等私网入口，并将 `allow_insecure_http` 改回 `false`。
