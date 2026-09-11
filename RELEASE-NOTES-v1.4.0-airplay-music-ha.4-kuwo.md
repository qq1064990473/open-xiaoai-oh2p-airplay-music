# v1.4.0-airplay-music-ha.4-kuwo

## 音乐源更新

- 默认歌曲和歌手搜索改为酷我；接口错误或没有可用结果时自动回退 QQ MusicU。
- 酷我搜索结果的 `MUSIC_<RID>` 会转换为 `kw:<数字RID>`；QQ 结果使用
  `qq:<MID>`，两种 ID 在队列、缓存与取链阶段严格隔离。
- 酷我播放链接优先使用长青酷我：
  `https://musicapi.haitangw.net/music/kw.php`；失败后自动尝试念心酷我：
  `http://music.nxinxz.com/kw.php`。
- 保持原有 AirPlay、HA、OH2P 原生 14 号 LED、播放键、队列及 QQ 歌单/随机榜单逻辑。

## 配置迁移

将发布包中的 `client.example.json` 合并到设备的
`/data/open-xiaoai/client.controlfix.json`，特别是 `music.search` 和
`music.play_url` 下新增的 `kuwo_*` 字段。该 JSON 含中文字段说明。

第三方接口并不由本项目运营，可能随时更改或失效；请仅在拥有授权的内容和环境中使用。

## 验证

- Rust 单元测试：18 通过。
- 真实网络验证：酷我搜索返回 `MUSIC_51685512`，剥离前缀后，长青和念心端点均返回
  HTTP 206、`audio/mpeg`。
