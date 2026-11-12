# Rust 单 EXE 版

应用逻辑集中在 [rust/codex_pet.rs](../rust/codex_pet.rs)，Cargo 清单与锁文件记录编译依赖。发行物为单个 Windows x64 EXE，含全部 92 个原始动画，无需 Electron、Node.js、WebView 或额外资源文件。

程序图标取自白龙“元气挥手”动作第 48 帧的笑脸头像，保留透明背景，内嵌 16–256 像素的十种尺寸。来源和裁剪记录见 [assets/ICON_SOURCE.json](../assets/ICON_SOURCE.json)，图标不需要作为外部文件放在 EXE 旁边。

当前预览版：**[v0.5.0-preview.4](https://github.com/Miraclemarvel/codex-desktop-pet/releases/tag/v0.5.0-preview.4)** · [下载 EXE](https://github.com/Miraclemarvel/codex-desktop-pet/releases/download/v0.5.0-preview.4/CodexPet-Rust-Windows-x64-v0.5.0-preview.4.exe) · [SHA-256 校验文件](https://github.com/Miraclemarvel/codex-desktop-pet/releases/download/v0.5.0-preview.4/CodexPet-Rust-Windows-x64-v0.5.0-preview.4.exe.sha256)

## 使用

把 EXE 放在固定文件夹，双击运行。默认启用当前用户登录启动，后台轻量监听 Codex；Codex 打开后显示桌宠，主进程退出后桌宠结束。监听程序和桌宠进程均由同一个 EXE 运行，不显示托盘或任务栏图标。

从 Electron 版切换时，先在旧版右键关闭“随 Codex 启停”并退出旧桌宠，再运行 Rust 版。两个版本使用各自的设置目录和启动项，便于回退。

Rust 版升级时，先在旧版右键选择“退出并关闭后台监听”，再双击新版 EXE；原有偏好与位置会保留。

- 点击角色或状态条展开额度卡片；支持多额度窗口、重置时间、手动刷新和缓存提示。
- 按住拖动，右键恢复到屏幕内。窗口按 Windows 原生屏幕坐标移动，静止长按不会反复写入位置。
- 右键调整待机动态、画面效果、大小、暂停播放，或从 92 个动作中选择预览。
- 右键“退出桌宠（本次 Codex）”后，本次 Codex 会话内不会自动重新出现。
- 右键关闭“随 Codex 启停”取消登录启动和后台监听；“退出并关闭后台监听”只结束当前后台监听。
- 设置与运行状态保存在 `%APPDATA%/Codex Desktop Pet Rust/`。动画直接从 EXE 解码，不释放外部素材。

默认使用安静待机、清晰画面效果。素材中开头的浅色过渡帧会跳过，新动画解码时保留当前画面。

0.5.0-preview.2 调整文字：数字与西文使用 Segoe UI，中文使用微软雅黑 UI；降低字重并按实际 DPI 做三倍采样的灰度抗锯齿。额度数字由 27 缩小到 20 逻辑像素，百分号独立使用 11 像素辅助字号。字形覆盖缓存有固定上限，不随动画帧反复光栅化。

0.5.0-preview.3 在额度卡片左下角以 10 像素小字显示“可重置 N 次”，读取接口的实际可用重置次数。接口未返回次数时显示“重置次数 —”，不把未知值当作零；底栏固定在卡片下方，滚动额度内容时仍可见。

0.5.0-preview.4 将笑脸头像图标内嵌到 EXE，并供程序窗口使用；构建脚本自动验证十种尺寸的资源，确认程序代码区未被图标写入改变。GitHub Windows CI 构建后保存 EXE 与校验文件，发布使用同一份已验证产物。

## 无损封装

92 个原始 WebP 文件共 136,977,916 字节，逐文件 zlib 封装后为 135,042,242 字节，缩小约 **1.41%**。每个文件都验证解包后与原始字节完全一致，不重新编码图像。EXE 的主要体积来自原始动画，移除 Electron 是更主要的程序体积收益。

素材作者为 [hunmer](https://github.com/hunmer)，来源为 [aboutme-pets/pets/gpt](https://github.com/hunmer/aboutme-pets/tree/main/pets/gpt)。来源清单、上游 README 与第三方许可文本嵌入 EXE；右键可打开来源或查看署名，完整文本可用 `--licenses 文件路径` 导出。素材不因 Rust 重写而获得额外许可。

## 编译

需要 Windows x64、Rust stable（MSVC）、Visual Studio C++ 编译工具与 Windows SDK、Python 3。

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/build-rust.ps1
```

脚本按 Cargo.lock 获取依赖，收集第三方许可证，生成内嵌素材包，运行 Rust 测试，然后静态链接 C 运行库并输出单个 EXE 与 SHA-256 校验文件。应用只有一个 Rust 源文件；素材仍保留为独立的上游文件供构建使用。`outputs/` 与 `rust/target/` 不提交到 Git。

编译时会将项目、Cargo 与 Rust 工具链的本机目录映射为通用路径，避免在发布 EXE 中保留本机编译目录。可用 `-OutputDirectory 目录` 将发行物写入独立目录。

也可向脚本传入 `-Offline` 使用已缓存的依赖。Rust 单文件不是零依赖的 `rustc 文件.rs` 项目；Cargo 清单和构建时的素材包仍是编译输入。

## 接入与验证

使用与 Electron 版相同的 Codex 桌面本地协议，仅调用 `list_threads` 与 `get_usage_limits` 两种只读工具，并持续处理任务状态订阅。对话正文、工具参数与待确认请求内容不保存在状态镜像中。当前适配仍以 Codex Windows 26.1106.6628.0 为验证对象。

16 项 Rust 测试覆盖原始素材字节一致性、消息分帧、只读工具限制、状态优先级、额度缺失与边界值、正文丢弃、待机动态、稳定颜色帧、静止长按、拖动坐标和透明合成。原生窗口测试另行验证长按 128 次采样不写入位置；独立模拟宿主验证随宿主启动、退出，以及手动退出后本会话不重开。真实 Codex 未在测试中关闭，Windows 注销和其他电脑的验收仍需另行进行。

桌面自动化在隔离的 `--ui-test` 窗口中实际点击打开额度卡片，并拖动角色，核对位置写入设置文件、窗口尺寸保持 650×775 物理像素。正常模式的透明绘制和隐藏任务栏样式由原生窗口检查覆盖。真实 Rust 管道检查读到了当前工作任务及账号额度；界面交互检查使用演示额度，与真实账号数据验证分开记录。

开发检查参数：`--probe 输出.json` 进行真实状态/额度只读检查；`--owner-probe 输出.json` 检查 Codex 主进程识别；`--demo --profile 目录 --smoke 输出目录` 运行隔离窗口检查；`--ui-test` 仅用于桌面自动化发现窗口，会临时显示任务栏入口，正常运行不使用此参数。
