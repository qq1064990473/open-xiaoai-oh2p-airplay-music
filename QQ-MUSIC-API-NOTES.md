# QQ MusicU 搜索接口核对

核对日期：2026-08-14。

## 结论

当前 Client 的请求结构正确：向 `https://u.y.qq.com/cgi-bin/musicu.fcg` 发送 JSON POST，请求项使用：

```json
{
  "music.search.SearchCgiService": {
    "method": "DoSearchForQQMusicDesktop",
    "module": "music.search.SearchCgiService",
    "param": {
      "num_per_page": 20,
      "page_num": 1,
      "query": "周杰伦的晴天",
      "search_type": 0
    }
  }
}
```

本机用相同 URL、请求体和 User-Agent 实时验证时，顶层 `code` 与服务 `code` 均为 `0`，返回 20 首歌曲，首项为“晴天 / 周杰伦”。公开项目 [MergeMusicDesktop](https://github.com/flwfdd/MergeMusicDesktop/blob/02ca7b89d96199ec51d3a7a7383c2d33686fd1d5/src/main/java/xyz/flwfdd/mergemusicdesktop/music/QQMusic.java#L70) 也使用相同的 module、method 和分页参数。

QQ 音乐没有为该内部接口提供公开稳定性承诺。旧的 `client_search_cp` 接口在 [Rain120/qq-music-api#113](https://github.com/Rain120/qq-music-api/issues/113) 中已被报告为 `code:0` 但列表为空，并建议迁移至 MusicU；[相关修复 PR](https://github.com/Rain120/qq-music-api/pull/121) 也说明不能只根据 HTTP 状态或顶层 `code` 判断搜索成功。

## 本版容错

- HTTP、TLS、超时和 JSON 错误继续使用原有传输重试。
- 校验顶层 `code` 与 `music.search.SearchCgiService.code`。
- 第一页歌曲列表为空、缺失或没有可用 MID 时，默认额外重试两次。
- `empty_result_retries` 可在外置 JSON 中调整；旧配置缺少该字段时自动取默认值 `2`。
- 第二页及以后不做空结果重试，正常表示分页结束。
- 重试耗尽后仍返回空结果，由现有音乐路由报告未找到歌曲，不会错误播放其他内容。

这项修复降低长时间待机后首次搜索被临时空响应影响的概率，但无法保证第三方内部接口永久可用。
