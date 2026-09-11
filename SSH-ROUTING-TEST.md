# 通过 SSH 测试语音文字分路

这个测试不会远程打开麦克风，也不会模拟真实唤醒、AEC、降噪、VAD 或小米 ASR。它模拟的是音箱已完成 ASR、得到最终文字之后的 `SpeechRecognizer.RecognizeResult`，因此适合验证当前 Client 的：

```text
音乐固定规则 -> Home Assistant Conversation -> 小爱原生回退
```

## 推荐方法

将 `tools/oh2p-route-test.sh` 上传到音箱：

```shell
scp tools/oh2p-route-test.sh root@音箱IP:/data/open-xiaoai/
ssh root@音箱IP 'chmod 755 /data/open-xiaoai/oh2p-route-test.sh'
```

远程执行测试：

```shell
ssh root@音箱IP "/data/open-xiaoai/oh2p-route-test.sh '打开次卧台灯'"
ssh root@音箱IP "/data/open-xiaoai/oh2p-route-test.sh '播放周杰伦的晴天'"
ssh root@音箱IP "/data/open-xiaoai/oh2p-route-test.sh '今天天气怎么样'"
```

查看分路日志：

```shell
ssh root@音箱IP "tail -n 100 /tmp/open-xiaoai-client.log"
```

关键日志含义：

```text
[routing] claimed music ASR       本地音乐规则接管
[routing] sending ASR to Home Assistant
[ha] handled                      HA 成功处理
[ha] falling back to XiaoAI       HA 未匹配，回退小爱
```

前提条件：Client 正在运行，并且监听的是 `/tmp/mico_aivs_lab/instruction.log`。测试 HA 分路时还必须存在 `/tmp/open-xiaoai-pns.lab`，并看到过 `[ha] authenticated and ready`。

## 只测试小爱原生 NLP

以下命令会直接把文字提交给小爱云端，不经过当前 Client 的音乐或 HA 分路：

```shell
ssh root@音箱IP 'ubus -t 5 call mibrain ai_service '\''{"tts":1,"nlp":1,"nlp_text":"今天天气怎么样"}'\'''
```

它适合确认小爱原生服务是否正常，但不能用于判断一句话最终由音乐、HA 还是小爱处理。
