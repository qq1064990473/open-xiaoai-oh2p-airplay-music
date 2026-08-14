# v1.4.0-airplay-music-ha.3

修复 OH2P 长时间待机后，第一次 QQ Music 搜索偶尔收到空列表而直接报告“未找到歌曲”的问题。

## 变更

- 第一页歌曲搜索返回空列表、缺失列表或列表中没有有效 MID 时，默认等待 500 ms 后重试，最多额外两次。
- 校验 MusicU 顶层业务码和服务业务码，不再将业务错误当作正常空结果。
- 新增简洁诊断日志，记录重试次数、业务码、列表存在状态和 QQ `qc` 建议词。
- 新增外置配置 `music.search.empty_result_retries`，默认值为 `2`；旧配置不需要修改即可使用默认值。
- 保持 AirPlay、Home Assistant、LED、播放队列、MID 缓存和播放 URL 获取逻辑不变。

## 构建

- 目标：OH2P / ARMv7 / glibc 2.25 / hard-float
- 解释器：`/lib/ld-linux-armhf.so.3`
- 最高 GLIBC：`GLIBC_2.25`
- 二进制：`client-airplay-music-ha-oh2p-armv7-glibc2.25-20260814`
- SHA-256：`38074109a9e3dc8239992ccc0422e7a4b8011004b6324aa5fcbd15b215c0d6c8`

本版本是 `/data` 用户态 Client 更新包，不是刷写固件分区的镜像。先以新文件名前台测试，确认正常后再替换现有 `/data/open-xiaoai/client-ha.new`。
