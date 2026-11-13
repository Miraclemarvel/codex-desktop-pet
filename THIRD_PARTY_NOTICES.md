# 素材及运行时来源

## GPT 白龙娘动画素材

- 作者：[hunmer](https://github.com/hunmer)
- 原仓库：[hunmer/aboutme-pets](https://github.com/hunmer/aboutme-pets)
- 原始目录：[pets/gpt](https://github.com/hunmer/aboutme-pets/tree/main/pets/gpt)
- 核对版本：[4f8e9e327bad8c137d98d8ec591712af920de8b7](https://github.com/hunmer/aboutme-pets/commit/4f8e9e327bad8c137d98d8ec591712af920de8b7)

上游 README 将 GPT 动作包的 92 个动作标为作者生成。本项目保留原始 WebP 精灵图、动作名称及清单，并保存原始仓库说明为 `assets/UPSTREAM-README.md`；逐文件校验记录位于 `assets/SOURCE.json`。Windows 发行包中的相应文件位于 `resources/app/assets/`。预览图由这些素材的动画帧展示而来。

Rust EXE 图标由该动作包“元气挥手”的第 48 帧头像裁剪、缩放而成，保留原图颜色与透明背景；源文件校验值、帧号及裁剪范围记录于 `assets/ICON_SOURCE.json`。

Rust 0.5.0-preview.5 的侧边探头素材与闭眼帧，由 OpenAI 内置 imagegen 依据该角色画面参考、多轮用户指导辅助生成，属于本项目新增的参考衍生图，并非上游原始动作。半边头部、手、翅膀与尾巴使用该新增图；眨眼只合成闭眼帧的眼睛区域。PNG 原图、逐像素无损的 WebP、定稿要求和生成记录分别保存在 `assets/edge-peek*.png`、`assets/edge-peek*.webp`、`assets/edge-peek-prompt.txt` 和 `assets/EDGE_SOURCE.json`。原始 92 张 WebP 没有修改。发饰使用 ChatGPT 结形标志，相关标志归其权利人所有。

角色与动画的权利归原权利人所有，本项目不把它们声明为原创，也不另行授予素材许可。截至 2026-11-12，上游根仓库没有统一的 LICENSE 文件，GPT 目录也未单列许可文件。其他角色包的素材限制或上游工具的代码 MIT 许可不自动适用于 GPT 素材；使用素材时请核对原作者的最新说明。

0.5.0-preview.9 依据用户提供的角色参考，通过同一内置 imagegen 修正发饰下方的细链、珠坠与白色流苏。生成原图与无损 WebP 保存为 `assets/edge-peek-tassel.*`，提示词为 `assets/edge-peek-tassel-prompt.txt`；运行时仅取该局部饰物，保留原先定稿的其余姿势和颜色，裁剪坐标及校验值见 `assets/EDGE_SOURCE.json`。

## Electron

Electron 版使用 Electron 40.10.6，采用 MIT 许可。完整运行时及 Chromium 第三方许可证随 Electron 版 Windows 发行包中的 `LICENSE`、`LICENSES.chromium.html` 一并提供。Rust 单 EXE 版不包含或使用 Electron。

## Rust 单 EXE 版

Rust 版使用 Windows 自带的 Win32 API，以及 `windows-sys`、`serde`、`serde_json`、`image-webp`、`miniz_oxide` 等依赖。各依赖的实际版本、许可标识及完整许可文本在打包时从锁定依赖中收集，并嵌入 EXE；运行 `CodexPetRust.exe --licenses 输出文件路径` 可导出。软件依赖的许可不覆盖角色和动画素材。

素材只经过可逆的 zlib 封装，解压后恢复原始 WebP 文件字节，不重新编码或降低动画画质。运行时直接从 EXE 读取素材，不释放外部图片、DLL、Electron 或 WebView 文件。

## Codex 与 OpenAI

Codex 和 OpenAI 为其各自权利人的名称。本项目为独立社区伴侣程序，与 OpenAI 无隶属关系。

本地接口根据桌面应用的 app tools、状态订阅协议及 [App Server 文档](https://learn.chatgpt.com/docs/app-server) 进行适配。本项目不分发 Codex 程序代码或登录凭据。
