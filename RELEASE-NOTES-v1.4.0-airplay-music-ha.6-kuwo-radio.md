# v1.4.0-airplay-music-ha.6-kuwo-radio

## 音源优先级统一

- 主搜索源配置为酷我时，歌曲、歌手、歌单和随机电台均先请求酷我；搜索异常或没有可用结果才回退 QQ MusicU。
- 酷我歌曲仍使用 Kuwo RID 与酷我直链；若两个酷我直链接口都失败，则按歌名/歌手查询 QQ，只有同名结果才允许转到 QQ 主/备用取链。
- QQ 主源配置仍使用原有 QQ 搜索、榜单与 URL resolver，不把两种服务的 ID 混用。

## 歌单与随机电台

- 酷我歌单搜索使用 `ft=playlist`，读取酷我 H5 歌单曲目；空结果或接口失败时回退 QQ 歌单搜索及读取。
- 酷我随机电台默认读取酷我热歌榜 `id=16`，按页随机打乱；接口不可用时回退 QQ MusicU 榜单。
- 随机播放变为持续电台：默认每批 10 首，待播队列不超过 3 首时后台预取；排行榜分页耗尽后循环新一轮，并按 ID 去重。
- 若酷我搜索虽有响应、但歌手过滤后没有可播放候选，歌手队列及单曲后的同歌手续播会再尝试 QQ 搜索。
- 连续获取失败采用递增间隔重试，用户停止或切换内容后取消旧会话补队列。

## 配置

新增配置项：

```json
{
  "music": {
    "search": {
      "kuwo_random_toplist_id": 16,
      "kuwo_toplist_detail_api_url": "https://kbangserver.kuwo.cn/ksong.s",
      "kuwo_playlist_info_api_url": "https://m.kuwo.cn/newh5app/wapi/api/www/playlist/playListInfo"
    },
    "queue": {
      "random_batch_size": 10,
      "random_prefetch_threshold": 3
    }
  }
}
```

旧 JSON 可继续使用；缺少这些字段时程序应用默认值。建议部署前保留原二进制和配置，观察 `[music-api]` / `[music]` 日志。

## 验证边界

- 单元测试覆盖酷我响应字段解析、跨源同名筛选和音乐命令解析。
- 接口手测确认酷我歌单搜索、歌单曲目分页、热歌榜分页以及 QQ 榜单响应可用。
- ARMv7 / glibc 2.25 的发布二进制构建和目标设备替换前仍须通过对应构建、哈希及设备启动检查。
