# v1.4.0-airplay-music-ha.5-kuwo-ranking

## 变更

- 酷我歌曲和歌手搜索请求改为与 `https://13413.kstore.vip/yuanli/kw.js` 使用的播放器搜索参数一致：`client=kt`、`uid=2574109560`、`ver=kwplayer_ar_8.5.4.2`、`vipver=1`、`ft=music`、`cluster=0`、`strategy=2012`、`encoding=utf8`、`rformat=json`、`vermerge=1`、`mobi=1`。
- 默认酷我搜索分页数由 20 调整为该音源使用的 30；`pn` 继续按酷我从 0 开始的规则传递。
- 保留外置配置覆盖：`music.search.kuwo_api_url`、`kuwo_uid`、`kuwo_version` 和 `page_size` 均可在音箱上的 `client.controlfix.json` 修改，无需重新编译。

## 验证

- 2026-09-12 实测完整请求搜索“周杰伦的晴天”，首条返回 `MUSIC_228908`，歌名《晴天》、歌手周杰伦；KTV 伴奏位于第 8 条。
- 已重新构建 OH2P ARMv7/glibc 2.25 发布二进制。请先使用新文件名前台运行验证后，再覆盖运行中的 Client。

第三方音乐服务随时可能调整或失效，请仅在确认版权和服务许可的前提下使用。
