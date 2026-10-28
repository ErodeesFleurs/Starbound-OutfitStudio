# Starbound Outfit Studio

在浏览器里绘制、预览并导出 Starbound 服装、帽子和人物重绘指令。PNG 由 Rust / WebAssembly 在本地处理，不上传服务器；生成结果引用原版资源，其他玩家无需安装你的 PNG。

**[打开网页版](https://198.44.178.249:8443/)** · [本地运行](#本地运行) · [使用说明](#使用说明) · [命令行](#命令行) · [开发与验证](#开发与验证)

| 模式 | 输入 | 输出 |
| --- | --- | --- |
| 普通／隐藏身体裤装 | 裤装精灵表 | 物品 JSON 或 `/spawnitem` 命令 |
| 上衣与袖子 | 身体、前袖、后袖，可单独或组合使用 | 物品 JSON 或 `/spawnitem` 命令 |
| 背饰 | 背饰精灵表 | 物品 JSON 或 `/spawnitem` 命令 |
| 帽子 | 单张 PNG，可设置名称、稀有度和可见性 | 物品 JSON 或 `/spawnitem` 命令 |
| 人物重绘 | 身体、双臂、基础头、表情与头发 | identity 补丁、完整身份、备份或撤销内容 |
| 原版直接改色 | 一组或多组原图／目标图 | 纯 `?replace` 指令 |

> 人物重绘不是装备。修改已有角色前，先备份完整 `.player` 存档；不要把 identity 补丁直接当作完整身份或存档文件。

## 本地运行

在项目根目录执行。无需 Node.js 或前端包管理器。

### 使用 Nix

```sh
nix develop path:./nix
cargo xtask serve
```

打开 <http://127.0.0.1:4200>。开发环境提供 Rust 1.98.0、WASM 目标和 wasm-bindgen-cli 0.2.129。

### 使用 rustup

```sh
rustup toolchain install 1.98.0 --profile minimal \
  --component rustfmt --component clippy --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo xtask serve
```

工具链固定在 [rust-toolchain.toml](rust-toolchain.toml)；wasm-bindgen-cli 的版本须与工作区依赖匹配。

只构建静态站点，或服务已有构建：

```sh
cargo xtask build                       # 输出到 dist/
cargo xtask serve --no-build --port 4201 # 不重复构建
```

`dist/` 可直接部署到 HTTP(S) 静态托管，不需要 Rust 后端。请通过服务器打开，不要直接双击 HTML 文件；复制功能需要 localhost 或 HTTPS，以及浏览器剪贴板授权。

## 使用说明

网页采用浅色工作区与深色模式栏。桌面并排显示输入和预览，窄屏改为单列；主要按钮为下载 JSON，其余导出方式仍可直接使用。编码设置、模板、透明余边说明和页脚许可按需展开，人物存档安全及改色约束保持可见。支持键盘操作、200% 字号、强制高对比度和减少动态效果。

### 服装

1. 在对应模式下载 PNG／PDN 模板，按原帧位置绘制，保持透明背景、43 × 43 帧格和左上原点不变。
2. 选择或拖入 PNG。上衣的身体、前袖、后袖均可单独提供，也可组合成一件物品；组合顺序为身体 → 前袖 → 后袖。
3. 检查预览，复制或下载物品 JSON、`/spawnitem` 命令。普通裤装和隐藏身体裤装共用同一张输入图。

| 输入部位 | 有效尺寸 |
| --- | --- |
| 裤装 | 387 × 258 |
| 上衣身体 | 86 × 258 |
| 前袖、后袖、背饰 | 387 × 301 |

执行服务器内的 `/spawnitem` 仍需管理员权限；能穿戴物品不代表有生成物品的权限。剪贴板被拒绝时，可下载结果或在只读结果框中手动复制。

### 帽子

- 接受宽、高各 **1–256 像素**的 PNG。输入恰为 **86 × 215** 时，只取右上角 `(43, 0)` 起的 **43 × 43** `normal` 帧；其他尺寸使用整张图片并居中定位。
- 可设置名称、描述、五档稀有度，以及去除头发、隐藏身体。普通帽子使用原版 `eyepatchhead`，隐藏身体使用 `frogghead`。
- 原图、编码帧和人物单帧试戴均在本地显示。试戴只用于检查固定站姿与对齐，不模拟所有种族、动画或完整游戏渲染；预览人物图片不会进入物品。
- 修改名称、描述、稀有度或可见性开关不重新编码 PNG。帽子不使用服装辅助图配置。

### 人物重绘

基础头只需画一次；程序将表情叠加到基础头，再生成完整头部动画。头发是独立图层。

| 输入部位 | 有效尺寸 | 要求 |
| --- | --- | --- |
| 身体与腿 | 387 × 258 | 与双臂、基础头、表情一起提供 |
| 前臂、后臂 | 各 387 × 301 | 独立运动和旋转层 |
| 基础头 | 43 × 43，或原版 86 × 215 | 原版图只取 `(43, 0)` 起的 `normal` 帧 |
| 表情叠加层 | 387 × 344 | 只画眼睛、嘴等表情差异 |
| 头发 | 43 × 43 | 可单独编辑，也可随全身提供 |

**准备图像**

- 下载空白 PNG，在对应 `*-layout.svg` 标出的有效格内绘制。另有五张 `*-reference.svg` 原版像素参考，仅用于对照；不要把整张 SVG 转成 PNG 上传，标签和网格不能进入画作。
- `head-reference.svg` 展示的是合成后的完整头部，不是表情输入。透明表情保留基础头，不透明像素覆盖基础头，半透明像素按 RGBA 合成。
- 身体 44 格、前臂 34 格、表情 29 格可独立绘制；真正的原版帧别名必须保持相同画面。后臂 `walk.1/2/3`、`run.1/2` 也共享同图约束，未使用格不能绘制。
- 完整重绘需一起提供身体、前臂、后臂、基础头、表情五张图，缺失不会补空。若要隐藏某个关联部位，提供对应尺寸的透明图；隐藏整个头部需基础头与表情都透明。
- 仅编辑头发时，原 identity 必须包含非空的有效发型基底。

**读取身份并应用结果**

1. 备份完整存档，读取角色的 **identity JSON 对象**。OpenStarbound 可用 `/identity get`；离线编辑时先退出游戏，再用存档工具导出。不要粘贴 `.player` 二进制或完整存档。
2. 用真实 identity 替换网页中的示例。示例不是你的角色，基于它导出的备份或撤销内容不能恢复真实身份。
3. 导入 PNG，检查逐帧预览与分层站姿组合。完整重绘使用固定 Floran 图源，但保留 species、gender、名称等身份属性，以及未编辑的发型和面部图层。
4. 默认导出 identity patch，**将变化字段合并到原 identity**；也可导出合并后的完整对象、原身份备份或撤销内容。Undo 中的 `null` 表示删除原先不存在的字段。

**不要执行 `/identity set PATCH`**：不完整对象会重置遗漏字段。命令导出使用 OpenStarbound 客户端本地 `/run`，读取当前身份后合并变化；`/run` 和 `/identity` 都不是原版服务器命令。离线工具应修改存档内的 identity 字段，不能用 JSON 覆盖整个 `.player` 文件。

应用后保存、重新进入并核对外观；恢复时使用真实备份或对应的撤销内容。全身图层共享 `bodyDirectives`，不要把单个关联图层的指令拆出来随意应用。默认保留未编辑的胡须、喙等面部层；若需去掉它们，先在角色编辑工具中清除相应字段，再重新读取 identity。

### 原版直接改色

提供实际使用的原版基底 PNG 与目标 PNG。每对图片必须等尺寸，不同配对可用不同尺寸；所有配对联合生成一段同时映射的 `?replace`。

- 同一个源 **完整 RGBA** 在所有配对中必须始终对应同一个目标 RGBA。一个源色需要变成多个目标色时，会报告配对和左上零基坐标并拒绝生成，不回退到自绘模式。
- 包括未修改像素、alpha=1、半透明颜色和 alpha=0 的隐藏 RGB。编辑器不能在重存原图时清零隐藏 RGB；改变透明度可能同时显露所有同源透明像素。
- 原图须覆盖实际受影响的全部图层和有效动画帧，包括 `climb` 等动作。必要时按 `.frames` 裁取有效帧再配对；工具不会推断或忽略精灵表无效格，也不会验证未提供的帧。
- 带标注或复用姿势的参考 SVG 不是原版完整精灵表，不能当作严格改色基底。输出须应用到提供的原图所对应的处理阶段，不能盲目接在已改变源色的指令后面。
- 支持两色交换、多源色合并。两图完全一致时，空字符串是有效结果：网页复制会清空剪贴板，下载得到零字节文件。

本模式只输出纯指令，不生成物品、`/spawnitem` 或 identity 命令，也不是任意位置自绘工具。

### PNG 与透明余边

PNG 由 Rust 解码，保留精确 8-bit RGBA、半透明和 alpha=0 的隐藏 RGB；Canvas 仅用于预览，不参与像素读回编码。

服装和人物的固定尺寸输入可带右侧、底部全透明余边：

- 只保留从 `(0, 0)` 开始的模板窗口。例如，人物身体 PNG 为 `387 × 301`、最后 43 行全透明时，会归一化为 `387 × 258`。
- 余边须全部 **alpha=0**，隐藏 RGB 可非零。存在任何可见像素时拒绝处理并报告坐标；小于模板的图不会拉伸或补空。
- 不缩放、不移动帧、不删除内部空白格，也不自动裁掉顶部或左侧边距。有效窗口内的 RGBA 不变，原文件不被改写。
- 原版 `86 × 215` 基础头保留裁取 `normal` 帧的规则；其他头部尺寸先尝试单帧窗口，再尝试原版窗口。帽子和直接改色不参与这项余边处理。

网页显示原始和处理后的尺寸，预览与导出使用处理后的像素。底层编码 API 仍要求标准尺寸，调用方应显式使用 `normalize_sheet` 或 `normalize_character_head`。

## 编码设置

默认使用 `profile=auto`、`strategy=text` 和无损调色板，通常无需更改。

| 目标 | 行为 |
| --- | --- |
| `text`（默认） | 选择完整像素指令文本最短的候选 |
| `compressed` | 按完整指令的 Deflate 压缩估算选择候选 |
| `passes` | 禁用调色板别名，每个导出指令字段仅保留一个像素赋色阶段；固定模板自身操作不变 |

服装的 `auto` 比较 `original`、`conservative`、`compact` 三种辅助图配置；明确指定某一配置时不跨配置搜索。`conservative` 使用较小的竖采样辅助图，`compact` 进一步缩小横采样辅助图。固定采样依赖原版资源，资源替换包可能破坏它们；`auto` 按当前目标评分，不代表游戏内存最少。

搜索有界，不承诺全局最优；不减色、不合并动画帧。帽子和人物使用各自的编码路径，直接改色不使用辅助图、编码目标或调色板别名。

浏览器使用 `CompressionStream('deflate')`，CLI 使用 flate2 zlib level 6，两者可能选出不同但像素等价的候选。统计中的压缩值只针对完整 UTF-8 directives，**不是物品 JSON、实际网络包或 FPS**。

修改图片或编码设置会清除旧结果；切换导出形式、复制和下载可复用最近一次编码。

## 命令行

以下命令在项目根目录运行；使用 Nix 时先进入开发环境。将示例中的 `art/` 和身份文件路径替换为自己的文件。

```sh
# 裤装：物品 JSON；--stats 将统计写到 stderr
cargo run -p outfit-cli -- encode --garment pants --input art/pants.png \
  --format json --out outfit.json --stats

# 上衣：三个部位组合，各部位也可单独提供
cargo run -p outfit-cli -- encode --garment chest \
  --body art/body.png --front-sleeves art/front.png --back-sleeves art/back.png \
  --profile compact --strategy compressed --format command --out outfit.txt

# 帽子：不能指定服装 --profile
cargo run -p outfit-cli -- encode --garment hat --input art/hat.png \
  --name '^orange;Custom Hat' --description 'My hat' --rarity rare \
  --remove-hair --hide-body --format command --out hat.txt

# 人物：原 identity 必填；备份文件必须不存在
cargo run -p outfit-cli -- character-encode \
  --identity identity.json \
  --body art/body.png --front-arm art/front-arm.png --back-arm art/back-arm.png \
  --head art/head.png --emotes art/emotes.png --hair art/hair.png \
  --backup-out identity.backup.json --format json --out identity.patch.json

# 仅改头发，也必须提供原 identity
cargo run -p outfit-cli -- character-encode \
  --identity identity.json --hair art/hair.png --format command --out apply-character.txt

# 直接改色：第 n 个 source 与第 n 个 target 配对，联合检查
cargo run -p outfit-cli -- recolor \
  --source art/body-original.png --target art/body-edited.png \
  --source art/arm-original.png --target art/arm-edited.png \
  --out recolor-directives.txt --stats
```

服装类型为 `pants`、`hidingPants`、`chest`、`back`、`hat`。通用导出支持 `json`、`command`、`directives`；可用 `--palette off` 关闭无损调色板。人物另支持 `--format merged`，其 `directives` 输出仅包含指令字段，可能缺少应用所需的图源和重定位字段，不能当作独立身份补丁。

人物的 `--backup-out` 以私有权限新建文件，逐字节保存原 identity 并拒绝覆盖；它不能代替完整角色存档备份。直接改色无变化时，CLI 的 stdout 和 `--out` 文件均只含一个结尾换行；网页下载则为零字节。

完整参数与人物帧坐标：

```sh
cargo run -p outfit-cli -- --help
cargo run -p outfit-cli -- character-encode --help
cargo run -p outfit-cli -- recolor --help
cargo run -p outfit-cli -- native-verify --help
```

## 兼容性与限制

- 兼容基准为 Starbound 1.4.4，已有原版与 OpenStarbound 0.1.15.1 的历史原生验证。固定采样仍可能受引擎构建、第三方资源包或图源替换影响，不能据此保证所有客户端与动画组合。
- 生成结果只引用原版资源，不新增游戏图像或物品定义。但人物应用工具、服务器权限、资源兼容性与服务器规则是不同问题。
- 网页的逐帧检查、分层站姿和帽子试戴不是完整游戏预览；不模拟姿态偏移、装备遮罩、所有种族或场景。装备仍可能遮挡重绘图层，指令不能新增骨骼、动画状态或碰撞几何。
- **旧物品和旧 identity 不会自动升级。** 修复后的指令需重新导出并替换或应用；仅刷新网页不能更新已保存的角色与物品。
- 原版引擎验证使用精确颜色足迹和透明 RGB 探针，OpenSB 可进行全 RGBA 比较；独立模型、网页预览、引擎检查和联网观察各有边界，不互相替代。

遇到异常时，请附上模式、输入尺寸、编码设置、引擎版本和错误信息；分享角色身份或存档前注意移除私人信息。

## 开发与验证

| 路径 | 职责 |
| --- | --- |
| [crates/outfit-core](crates/outfit-core) | PNG 解码、帧布局、像素编码、颜色映射和物品／身份导出 |
| [crates/outfit-web](crates/outfit-web) | Rust DOM 交互、上传、预览、剪贴板和下载 |
| [crates/outfit-cli](crates/outfit-cli) | 原生命令行、只读资产解析和游戏引擎验证 |
| [web](web) | HTML 和 CSS |
| [public/assets](public/assets) | 模板、布局、参考图和字体 |
| [xtask](xtask) | WASM 构建、静态打包与本地服务器 |
| [tools](tools) | 游戏 Lua 环境中使用的原生图像探针 |
| [deploy](deploy) | Caddy 与 systemd 部署示例 |

与 [CI](.github/workflows/verify.yml) 一致的检查，在开发环境中执行：

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p outfit-web --target wasm32-unknown-unknown --locked -- -D warnings
cargo xtask build
```

CI 不需要商业游戏资产。浏览器行为应另用真实上传、预览、复制和下载检查；涉及采样或画布链的改动还需原生引擎验证，不能只靠单元测试。

<details>
<summary>使用合法游戏资产进行原生验证（Linux）</summary>

游戏资产与二进制由使用者自行提供。不要把 `packed.pak`、私人存档或验证夹具加入仓库、Nix store、CI 或网站。

```sh
# 只读资产校验，并在项目外的新目录生成私有引擎夹具
cargo run -p outfit-cli -- verify-assets --pak /path/to/packed.pak \
  --out /tmp/outfit-assets-report.json --engine-dir /tmp/outfit-private-fixture

# 必须使用隔离环境，不能指向现有世界或角色存档
cargo run -p outfit-cli -- native-verify --mode vanilla \
  --server /path/to/starbound_server --pak /path/to/packed.pak \
  --fixture /tmp/outfit-private-fixture --run-dir /tmp/outfit-private-run --measure-images
```

`--engine-dir` 必须是项目外的新目录或空目录，运行目录也须新建；夹具包含游戏图像，使用后妥善清理。可用 `--hats-only` 或 `--characters-only` 只生成对应验证集。OpenSB 使用 `--mode opensb`，还需 `--opensb-assets /path/to/opensb`。

启动器使用私有存储、loopback RCON 和随机密码，保留游戏的脚本限制。只有案例计数完整、全部 PASS、无 FAIL 且正常退出才写成功报告。`--measure-images` 的计时含 IPC 与共享缓存，不是冷解码或游戏帧率。

</details>

## 部署

运行 `cargo xtask build` 后只部署 `dist/`，保留其中的 `pkg/`、`assets/` 和许可文件。不要上传项目根目录、游戏资产包或角色存档。页面没有编码后端，也不需要 PNG 上传接口。

现有 VPS 的 Caddy 和 systemd 服务由 NixOS 配置声明，不能用本仓库的独立部署示例覆盖生成的 unit 或解密配置。日常发布只新增静态版本并切换 `current`，不修改端口、证书或系统配置；保留上一版本以便回滚。

<details>
<summary>Caddy / systemd 示例</summary>

[deploy/Caddyfile](deploy/Caddyfile) 与 [deploy/outfit-studio.service](deploy/outfit-studio.service) 使用以下布局；复用前须按自己的主机修改地址、路径，准备低权限用户与 Caddy 运行时：

| 路径 | 用途 |
| --- | --- |
| `/srv/outfit-studio/releases/` | 每次上传独立的完整静态版本 |
| `/srv/outfit-studio/current` | 指向当前版本的符号链接 |
| `/etc/outfit-studio/Caddyfile` | 静态托管与 HTTPS 配置 |
| `/var/lib/outfit-studio/` | ACME 账户、证书与状态，不应删除 |
| `/nix/var/nix/profiles/outfit-caddy` | 服务引用的 Caddy 运行时，需保留 GC root |

示例为 IP HTTPS，网页使用 TCP/UDP 8443，HTTP-01 证书验证需要公网 TCP 80；已禁用依赖 443 的 TLS-ALPN 验证。使用支持 IP 短期证书的 Caddy／ACME 服务，并保持自动续签所需的端口可达。静态文件使用 `no-cache` 重新验证，避免固定名称的 JS/WASM 长期缓存为旧版。

更新时，将完整 `dist/` 上传到新的版本目录后，再原子切换 `current`；仅更新静态文件无需重启服务。回滚时切回保留的旧版本，避免逐文件覆盖正在使用的目录。

在服务器上查看状态，或在修改 Caddyfile 后校验并重启：

```sh
systemctl status outfit-studio.service
journalctl -u outfit-studio.service -n 50 --no-pager
/nix/var/nix/profiles/outfit-caddy/bin/caddy adapt \
  --config /etc/outfit-studio/Caddyfile --adapter caddyfile --validate
systemctl restart outfit-studio.service
```

</details>

## 来源与许可

- 服装生成器基于 [Silverfeelin/Starbound-NgOutfitGenerator](https://github.com/Silverfeelin/Starbound-NgOutfitGenerator)（`d5bed9f65791b5ddc8368541ac2bc25da8ef8f91`），保留上游 MIT 许可、模板和 Nettle Boy 的指令方案。本项目独立于 OpenStarbound 源码仓库。
- 帽子及复用的坐标画布算法改编自 [Silverfeelin/Starbound-Hatter](https://github.com/Silverfeelin/Starbound-Hatter)（`8a8d47314c639f84c50936bda61dfc198c39e49c`），相关部分采用 **CC BY 3.0，不是 MIT**；修改包括 Rust/WASM 移植、精确 RGBA 解码、稳定采样与无损编码。
- [Silkscreen](https://github.com/googlefonts/silkscreen) 字体采用 **SIL OFL 1.1**，完整许可随[字体资源](public/assets/fonts/OFL.txt)分发。
- Hatter 人物／头发单帧与参考 SVG 中的 Starbound 美术仍归 **Chucklefish／原权利方**。本项目的 MIT 与算法的 CC BY 3.0 不会重新授权这些美术，也不授予额外使用或分发权。独立生成的空白 PNG 和纯标签布局与游戏美术分别适用各自的权利边界。
- 人物方案参考了社区 [Drawable Generator](https://github.com/FezzedOne/FezzedOne-Drawable-Generator/tree/fa9c0450b33b5634e53d133a25dc70dfb54e4a41) 及其[指南](https://steamcommunity.com/sharedfiles/filedetails/?id=3071843411)。未复制该仓库源码或人物模板，也未将其视为 MIT 授权。

完整声明与第三方来源见 [LICENSE](LICENSE)。
