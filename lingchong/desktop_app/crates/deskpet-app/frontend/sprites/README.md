# 精灵图切片说明

## 文件放置

将 PNG 文件放在此目录（`frontend/sprites/`）下。

## 文件格式

每个 PNG 是一张**水平排列的帧序列**（sprite strip），透明背景。

```
┌──────┬──────┬──────┬──────┬──────┬──────┐
│ 帧0  │ 帧1  │ 帧2  │ 帧3  │ 帧4  │ 帧5  │  ← 6帧
└──────┴──────┴──────┴──────┴──────┴──────┘
  整图: 540x106 → 单帧 90x106
```

## 情绪对应表

| 文件名 | 情绪 | 触发场景 |
|---|---|---|
| idle.png | 待机 | 默认状态 |
| happy.png | 开心 | 用户点"喜欢" |
| curious.png | 好奇 | 播报热点时 |
| excited.png | 兴奋 | 高分热点 |
| sleepy.png | 困倦 | 深夜/勿扰 |
| bored.png | 无聊 | 用户点"不感兴趣" |

## 启用方法

1. 把 PNG 文件放入此目录
2. 编辑 `index.html` 中的 `SPRITE_CONFIG`，取消注释对应行
3. 设置 `frames`（帧数）、`fps`（播放速度）、`scale`（放大倍数）

```js
const SPRITE_CONFIG = {
  idle:    { src: 'sprites/idle.png',    frames: 6, fps: 4,  scale: 2 },
  happy:   { src: 'sprites/happy.png',   frames: 6, fps: 8,  scale: 2 },
  // ...
};
```

## 尺寸控制

| 参数 | 说明 | 示例 |
|---|---|---|
| `scale` | 放大倍数（默认 2） | 单帧 90x106 → 显示 180x212 |
| `w`, `h` | 直接指定显示宽高（优先于 scale） | `w: 120, h: 140` → 显示 120x140 |

不同情绪的图可以有不同的尺寸和帧数，引擎自动适配。

## 规则

- 只需放入部分情绪的切片图，未配置的情绪自动 fallback 到 SVG 动画
- `fps` = 每秒帧数，建议 idle 2-4，excited 8-12
- `frames` = 图中横向帧数，必须与实际 PNG 匹配
- PNG 背景必须透明
- 不同情绪的图可以有不同分辨率和帧数
# 当前基尔兽素材

`基尔兽表情像素动画图集.png` 已切分为 `idle.png`、`happy.png`、`curious.png`、`excited.png`、`sleepy.png`、`bored.png`。每张为透明背景的 **1024×128** 横向动画条，包含 **8 帧，每帧 128×128**。`index.html` 已启用六种表情，显示尺寸统一为 180×180。

保留原始图集。需要重新切分时，从 `desktop_app` 目录运行（会覆盖六张输出 PNG）：

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/split-guilmon-sprites.ps1
```

