# 图标

程序图标是可选的；把文件放进本目录后重新 `cargo build --release` 即可生效。

| 文件 | 用途 | 要求 |
| --- | --- | --- |
| `app.ico` | **exe 文件图标**（资源管理器、桌面快捷方式、任务栏） | ICO 格式，建议包含 16/24/32/48/64/128/256 多种尺寸 |
| `app.png` / `logo.png` | 窗口左上角 / 任务栏运行时图标（可选） | 正方形，建议 256×256 或 512×512 RGBA |

- 窗口图标优先用 `app.png`，没有再尝试 `logo.png`，最后回退到 ICO。
- 只有 `app.ico` 时也能构建，窗口图标会从 ICO 解码。
- 两个文件都没有时：exe 使用默认图标，构建不受影响。
- 构建脚本 `build.rs` 会自动调用 `tools/msys64/mingw64/bin/windres.exe`；
  该工具缺失时会在构建输出里给一条 warning，图标不嵌入，其余功能正常。

## 从 PNG 生成多尺寸 ICO

```python
from PIL import Image

img = Image.open("logo.png").convert("RGBA")
img.save(
    "assets/app.ico",
    sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
)
```

## 图标不更新？

Windows 会缓存 exe 图标。替换图标并重新构建后，如果资源管理器里还是旧图标：

- 把 exe 换个文件名，或复制到别的目录再看；
- 或重启资源管理器（任务管理器 → 重新启动「Windows 资源管理器」）。
