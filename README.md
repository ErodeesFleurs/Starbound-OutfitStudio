# Starbound Outfit Studio

使用 **Rust / WebAssembly** 在浏览器本地生成原版兼容的 Starbound 动画服装、帽子及角色身份重绘指令，同时提供原生 Rust 命令行和资产验证工具。没有 PNG 上传服务，不需要 Node、Bun、Angular 或远程解码脚本。

基于 [Silverfeelin/Starbound-NgOutfitGenerator](https://github.com/Silverfeelin/Starbound-NgOutfitGenerator) 的 `d5bed9f65791b5ddc8368541ac2bc25da8ef8f91`，保留 MIT 许可、上游模板和 Nettle Boy 的指令方案。本项目独立于 OpenStarbound 源码仓库。

## 运行

### Nix

```sh
cd ~/Code/Starbound-OutfitStudio
nix develop path:./nix
cargo xtask serve
```

打开 http://127.0.0.1:4200 。开发环境固定 Rust 1.98.0、`wasm32-unknown-unknown` 和 wasm-bindgen-cli 0.2.129；Nix 配置单独放在 `nix/`，避免进入开发环境时把整个 `target/` 复制进 Nix store。

### 已有 Rust 的环境

```sh
rustup toolchain install 1.98.0 --profile minimal \
  --component rustfmt --component clippy --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo xtask serve
```

`rust-toolchain.toml` 固定工具链；wasm-bindgen-cli 必须匹配工作区锁定的 wasm-bindgen 版本。原生引擎验证仅支持 Linux；其他平台没有在本次验证中实际运行。

```sh
cargo xtask build                       # 完整静态站点：dist/
cargo xtask serve --no-build --port 4201 # 服务已构建的站点，不重复构建
```

`dist/` 可部署到普通 HTTP(S) 静态托管；不需要 Rust 后端。JavaScript 仅用于 WASM 启动、启动失败提示和 wasm-bindgen 自动生成的浏览器绑定，界面交互与业务逻辑均由 Rust 实现。构建失败不会先删除上一份可用站点。

### VPS 部署

公网地址：**https://198.44.178.249:8443/**。VPS 使用独立的 `outfit-studio.service` 和低权限 `outfit-studio` 用户运行 Caddy 2.11.7，仅托管 `dist/`，不运行编码后端，不接收 PNG 上传。网页使用 TCP/UDP 8443，已释放 TCP/UDP 443；80 端口保留用于 ACME 验证和跳转到新 HTTPS 地址。JS/WASM 使用 `no-cache` 重新验证，避免固定文件名被长期缓存为旧版本。

配置源文件为 `deploy/Caddyfile` 和 `deploy/outfit-studio.service`。服务器布局：

| 路径 | 用途 |
| --- | --- |
| `/srv/outfit-studio/releases/20261020T071547Z-garment-fingerprints/` | 原版／OpenSB共同衣物帧识别，修复Floran jump.2及同类漏识别 |
| `/srv/outfit-studio/current` | 指向当前版本的符号链接 |
| `/etc/outfit-studio/Caddyfile` | HTTPS 与静态托管配置 |
| `/etc/systemd/system/outfit-studio.service` | 已启用开机启动的服务 |
| `/var/lib/outfit-studio/` | 私有 ACME 账户、证书与配置状态，勿删除 |
| `/nix/var/nix/profiles/outfit-caddy` | 固定 Caddy 运行时及其 GC root |

Caddy 来自 Nixpkgs `fe13242281a67641c0102cdb1087f0ebe586c7df`，运行时已复制到 VPS 并固定，不依赖本地电脑在线。公网 IP 使用 Let's Encrypt 正式 ACME 服务的 `shortlived` 证书，有效期 160 小时；Caddy 负责自动续签。配置禁用需要公网 443 的 TLS-ALPN 验证，改用 HTTP-01：必须保持服务运行、公网 TCP 80 可达，以及网页 TCP 8443 可达。若将来需要释放 80，须为 HTTP-01 保留转发或重新设计证书验证方案，不能只关闭端口。不使用自签证书或第三方动态 DNS；剪贴板仍需要浏览器授权，端口变更后可能需要重新授权。

在 VPS 上查看运行状态、日志；修改 Caddyfile 后校验并重启：

```sh
systemctl status outfit-studio.service
journalctl -u outfit-studio.service -n 50 --no-pager
/nix/var/nix/profiles/outfit-caddy/bin/caddy adapt \
  --config /etc/outfit-studio/Caddyfile --adapter caddyfile --validate
systemctl restart outfit-studio.service
```

更新网页时，在本地重新构建，然后只传输 `dist/` 到新版本目录；传输完成后原子切换链接，不必重启服务：

```sh
cd ~/Code/Starbound-OutfitStudio &&
nix develop path:./nix --command cargo xtask build &&
release=$(date -u +%Y%m%dT%H%M%SZ) &&
ssh -i ~/.ssh/openstarbound-builder root@198.44.178.249 \
  "test ! -e /srv/outfit-studio/releases/$release" &&
scp -r -i ~/.ssh/openstarbound-builder dist \
  "root@198.44.178.249:/srv/outfit-studio/releases/$release" &&
ssh -i ~/.ssh/openstarbound-builder root@198.44.178.249 \
  "chmod -R a=rX,u+w /srv/outfit-studio/releases/$release && \
   ln -s releases/$release /srv/outfit-studio/.current-$release && \
   mv -Tf /srv/outfit-studio/.current-$release /srv/outfit-studio/current"
```

不要上传项目根目录、`packed.pak`、角色存档或原生验证夹具。旧版本保留在 `releases/`；例如在 VPS 上回滚到首次部署、不含本地帽子模式的版本：

```sh
ln -s releases/20261019T075747Z /srv/outfit-studio/.current-rollback &&
mv -Tf /srv/outfit-studio/.current-rollback /srv/outfit-studio/current
```

## 使用

1. 下载对应 PNG/PDN 模板，在模板上绘制服装，保持每格 43×43、整张精灵表尺寸和帧位置不变。
2. 选择或拖放 PNG：裤装 387×258；上衣身体 86×258；前袖、后袖、背饰均为 387×301。
3. 普通裤装与隐藏身体模式共用输入；上衣身体、前袖、后袖均可单独使用，也可按身体→前袖→后袖组合导出。
4. 复制或下载物品 JSON、`/spawnitem` 命令。剪贴板需要 localhost/HTTPS 和浏览器授权；拒绝授权后生成结果仍可下载或手动复制。

PNG 由 Rust 解码，保留精确 8-bit RGBA、半透明和 alpha-zero 隐藏 RGB，不经 Canvas 反读或预乘转换后再编码；Canvas 只显示预览。能穿戴物品不等于拥有服务器 `/spawnitem` 权限。

### 像素工作台

- 视觉采用星舰开发终端风格，同时保留 Starbound 的像素标题、装备 SVG 图标与像素预览：近黑海军底色 `#0b1118`、扁平面板 `#111c27`、细线边框 `#2b414e`，正文 `#edf3f5`、次要文字 `#b9cbd3`，青绿 `#67e8c1` 强调选中与操作，琥珀仅用于兼容性警告。使用克制的静态网格，不加入动画、扫描线或假遥测；未提取游戏原始 UI 贴图或游戏字体。
- 浏览器标签页使用 `public/assets/favicon.svg`：沿用页眉像素徽标与青绿色，透明背景；通过相对路径本地加载，随静态站点构建发布，无外部图标服务。
- 顶部路径栏与 `BROWSER LOCAL`、`PNG LOCAL`、`RUST + WASM`、`VANILLA ASSETS` 标签描述浏览器本地编码与原版资源引用架构，不是连接、部署或游戏兼容性状态。静态三步说明区分物品与人物 identity 补丁，文件输入、像素预览与导出标题带技术标记。
- 桌面为左侧模式栏、右侧编辑与导出区；手机将六种服装／人物模式收成紧凑网格，并提供独立的原版直接改色入口，按中文词组换行。编码设置、模板下载与技术说明保留原生折叠区域；人物图像参考保持可聚焦的独立滚动，不将原尺寸参考缩成不可读的缩略图。默认编码设置不变。
- 英文标题保留自托管的 [Silkscreen](https://github.com/googlefonts/silkscreen) Regular 像素字体，原始 TTF 为 32,220 B，采用 **SIL OFL 1.1，不是 MIT**；完整许可随 `public/assets/fonts/OFL.txt` 一起发布。中文正文、下拉框和普通表单使用系统无衬线字体，技术标签、状态信息和代码文本保留等宽字体，不请求字体 CDN，不新增 JavaScript 依赖。
- 保留现有 DOM ID、ARIA、键盘焦点、跳到编辑器、原生按钮/表单语义与至少 44px 的主要操作目标；支持强制高对比度和减少动态效果。采用固定深色终端界面，系统浅色偏好不会改变配色；本次风格调整不改变 Rust 业务或资产。
- 终端风格初次上线时，实际浏览器检查六模式 × 五种宽度共 30 状态，含展开的人物参考图和完整长指令，无整页横向溢出；保留全部 136 个控件 ID。12 组代表文字的最低实测对比度约 7.64:1（非全站 WCAG 认证）；键盘模式切换、编码设置、帽子上传／可见性开关、强制高对比度与减少动态模式通过。公网头部参考图可见、人物命令全文与已验证版本逐字节一致，HTTPS CSS 校验通过。仅 HTML/CSS 改动，正式构建通过，WASM SHA-256 仍为 `d7d50b620e315f8bb8635bda92a5f7a70f18d6712b09be32dcdf012f678e9e32`，未重跑游戏引擎门禁。记录在 `~/.cache/outfit-geek-design/`。
- 后续文字可读性优化：主要说明和表单字号 16px，手机模式名 14px、技术标签至少 12px，增加行高并提亮普通、禁用和占位文字。真实浏览器再次通过六模式 × 五宽度，以及 320／375／1280px 下根字号 200% 的布局检查；流程条按字体大小自动换行，修复放大文字后的溢出。10 组代表文字对比度均超过 8.5:1，不代表全站认证。公网实际字号、人物命令逐字节不变、HTTPS CSS 哈希及零运行错误已验证，编码 WASM 不变；记录在 `~/.cache/outfit-readable/`。
- 历史像素工作台版本曾以真实 Chromium 155 验证 320/375/768/1024/1440px × 五种装备模式无横向溢出；同时检查展开模板、含完整长指令的 320px 页面、键盘切换与设置展开、PNG 拖放、错误尺寸恢复、真实剪贴板与文件下载、拒绝剪贴板后的下载回退和帽子自动裁帧。此记录不是本次终端风格的验证结果。
- 像素工作台换肤当时仅改 HTML/CSS 与字体资源，编码 WASM 的 SHA-256 保持 `5dca928ce26e2d63576d3278eefad4dff746d3ba543ef9723864583deec771e8`；之后的人物单帧预览更新了 WASM，见下节。帽子与裤装实际导出仍匹配原有指令哈希；未重跑或冒称新的游戏引擎门禁。换肤验证记录在 `~/.cache/outfit-pixel-design/`。


### 帽子：本地 Hatter

帽子已经整合到同一 Rust/WASM 编辑器，不再跳转或嵌入外部 Hatter 页面：

- 接受宽、高各 **1–256 像素 PNG**；若输入恰为 **86×215**，自动选取右上 **43×43** 帧（PNG 坐标 x=43、y=0）。其它尺寸使用完整图片。
- 支持名称、描述、五档稀有度、去除头发、隐藏身体；隐藏身体使用原版 `frogghead`，普通帽子使用 `eyepatchhead`。两种基底都只引用原版资源，不安装新物品定义或上传 PNG。
- 帽子只有一条通用画布路径：引用原版 `/interface/ai/xsmall.png;0;3`，生成 0/255 角点，先双线性放大 512 倍，再裁剪并以精确可表示的 `255/512` 最近邻采样。采样落在颜色量化区间内部，避免旧链在 256 边界依赖精确除法；最大中间图为 1024×1024，最终尺寸不变。这不是游戏内存或 FPS 优化声明。复用无损调色板、三种目标和缓存，保留半透明 RGB、alpha=1 和未赋色透明地址；旧 Hatter 会省略 alpha≤1 且通过 Canvas 读取像素。
- 原图、实际裁帧和单帧试戴均在本地显示。试戴使用 Hatter 同款两张 **43×43 人物／头发 PNG**，不再使用几何人形，不要求导入 `packed.pak`。隐藏身体同时隐藏人物与头发；去除头发只隐藏头发层，帽子始终在最上层。只预览固定站姿，不模拟所有种族、动画或完整游戏渲染；人物图片不进入导出物品。
- 名称、描述和稀有度只更新描述器；头发/身体开关还更新单帧试戴，但都不驱逐已有编码。图片、调色板和目标等编码设置变化使缓存失效。辅助图配置仅用于其它服装，在帽子模式禁用。

帽子算法改编自 [Silverfeelin/Starbound-Hatter](https://github.com/Silverfeelin/Starbound-Hatter) 的 `8a8d47314c639f84c50936bda61dfc198c39e49c`，算法许可证为 **CC BY 3.0，而非 MIT**。两张人物单帧也来自该固定提交的 `imgs/hatterCharacter.png` 与 `imgs/hatterHair.png`，原始字节不变、启动时各解码一次并复用离屏画布；人物美术版权归原权利方，**不因算法许可或本站 MIT 许可而重新授权**。来源和修改说明保留在页面与 `LICENSE`，没有引入上游 jQuery、FileSaver、Canvas 编码器或帽子示例图库。

单帧预览使用两倍逻辑尺寸的画布，使 43px 人物与奇偶尺寸帽子的半像素居中都落在整数画布坐标，关闭图像平滑。最初引入单帧预览时，真实 Chromium 的独立 PNG 合成对照覆盖 **36 种画面状态**：空帽、细行／列、最大 256、86×215 自动裁帧及四种身体／头发开关组合，RGBA 逐字节一致。该阶段实际帽子／裤装指令哈希未变，切换人物开关仍复用编码；WASM Clippy 与正式构建通过，当时未重跑游戏引擎门禁。历史记录在 `~/.cache/outfit-character-preview/`；后续帽子辅助图及指令变化见下方颜色方案验收。

### 原版直接改色：严格原图／目标图配对

这是独立于通用自绘的第七种模式。上传实际原版基底 PNG 和期望目标 PNG，可增加多组配对；同一组必须等尺寸，不同组可以不同尺寸。所有配对联合生成单阶段 `?replace`，只提供生成、复制、下载纯 directives，不自动推断装备、生成 `/spawnitem` 或 identity 命令。

- 同一个源 **完整 RGBA** 在任何位置、任何配对中都必须对应同一个目标 RGBA，包括未修改像素、alpha=0 的隐藏 RGB、alpha=1 和半透明。源色分裂会拒绝并报告配对、层及左上零基坐标，不回退成通用自绘。
- 原图应覆盖实际受该指令影响的全部图层和动画帧，包括原版 `climb` 等状态。必要时按 `.frames` 裁取有效帧后分别配对；不同层共享一个 directives 字段时必须一起检查。精灵表无效格须共同处理，不能只改目标的空格颜色。
- 图像参考 SVG、带网格标注的图，以及按共享 ID 复制 canonical 姿势的参考图，不是原版精灵表逐格原样数据。仅提供一帧不能证明其它未提供帧也符合映射。网页不读取或上传整份 PAK。
- 一段 `replace` 是同时映射，支持两色交换和多源色合并。若一整类源色一致改变 alpha 或轮廓，数学上可能可表达，但会影响它在其它位置的所有出现；不要把局部新增图案误当整体改色。
- 全部像素已一致时，空字符串是有效成功结果：复制会清空剪贴板，下载为零字节文件；不是透明人物或失败占位。
- 输出必须应用到你实际提供的原始基底；不要盲目接在已经改变源色的其它指令后面。直改色保留目标透明 RGB，而坐标画布自绘保留隐藏地址 RGB，两者是不同契约，不能称为可随意替换的严格无损优化。

### 人物重绘：角色 identity，不是装备

人物模式分开编码身体／腿、前后手臂、头部／表情与发型，不把整个人物复制到每一个图层，也不生成 `/spawnitem` 物品。

| 输入 | PNG 尺寸 | 约束 |
| --- | --- | --- |
| 身体与腿 | 387 × 258 | 与双臂、头部一起提供 |
| 前臂 | 387 × 301 | 独立运动和旋转层 |
| 后臂 | 387 × 301 | 独立运动和旋转层 |
| 头部与表情 | 387 × 344 | 每个使用的表情格绘制完整头部，不是 86 × 215 头部原图 |
| 头发 | 43 × 43 | 单帧；可单独编辑，也可随全身一起提供 |

1. 在“人物重绘”下载空白 PNG 和对应 `*-layout.svg` 纯标签布局；两者独立生成、不含游戏人物美术。另提供 `body-reference.svg`、`front-arm-reference.svg`、`back-arm-reference.svg`、`head-reference.svg`、`hair-reference.svg` 五张图像参考，内嵌原版人物像素并标注帧名／alias，网页可逐部分折叠、滚动预览或查看原图和下载。**参考 SVG 不是可直接上传的模板**：在空白 PNG 对应有效格绘画，不要将整张参考 SVG 转成 PNG 上传，标签与网格不能进入绘画像素层；以当前布局的 ID 和共享标识为准。
2. 先备份完整角色存档，再导入原角色的 **identity JSON 对象**。OpenSB 可用 `/identity get` 读取；离线编辑时应先退出游戏，并由存档工具导出 identity。不要上传或粘贴 `.player` 二进制，也不要把完整存档当成 identity。
   文本框预填可编辑的人类男性示例 JSON，便于查看格式；它不是你的角色身份。用于已有角色前必须替换为真实 identity，基于示例导出的备份／撤销不能恢复真实角色。
3. 按布局绘制。身体 **44 格**、前臂 **34 格**、头部 **29 格**可分别独立绘制。真正指向同一原版帧的别名，以及后臂原本完全相同的空帧组（`walk.1/2/3`、`run.1/2`），仍必须使用相同的可见 RGBA（含半透明与 alpha=1）；不同的透明 RGB 不算冲突。未使用格子不能绘制。工具仍拒绝真实共享冲突，不悄悄覆盖画作。
4. 完整重绘必须同时提供身体、前臂、后臂、头部四张表；要隐藏其中一层，提供对应尺寸的透明 PNG。也可以只提供头发，但原 identity 必须有非空的有效发型基底。
5. 默认导出 **identity patch**，只合并变化字段；另可导出合并后的完整 identity、原 identity 备份，或恢复变化字段的 undo patch／本地命令。恢复时也只合并相应字段，原来不存在的 `imagePath` 会被移除。

身体、头部及双臂共享 `bodyDirectives`，因此全身编码先识别图层／动作，再清掉原头部，通过 `emoteDirectives` 重绘完整头部；发型使用独立 `hairDirectives`。全身重绘把 `imagePath` 设为固定的 Floran 图源，**不改变 species、gender 或其他身份属性**；切换图源时，未编辑的发型、胡须、面具通过原版绝对资源引用和原有指令保留，不依赖 `../` 路径穿越。若已使用同一图源，则保持这些图层原样，不在每次编辑时重复包装。

默认保留未编辑的胡须、喙等面部层。若要把这些也纳入新头部，先在本地角色编辑工具中显式清除对应 `facialHairType`／`facialMaskType`，再重新读取 identity；应用命令所用的基线应与当前角色一致。

**不要执行 `/identity set PATCH`**：把不完整对象当成完整身份会重置遗漏字段。命令导出使用 OpenSB 的本地 `/run`，先读取当前身份、合并变化，再一次调用 `player.setHumanoidIdentity`。`/run` 与 `/identity` 不是原版服务器命令；离线工具也可以把合并后的对象写入存档的 identity 字段，**不能直接用 JSON 覆盖 `.player` 二进制文件**。

生成结果仅引用游戏自带资源，不要求服务器或其他玩家安装输入 PNG。编辑工具、服务器权限和服规仍是不同问题；第三方资源包替换图源可能破坏固定指纹。网页提供逐帧像素检查和固定站姿分层合成，不模拟游戏姿态偏移、装备遮罩、骨骼或完整场景；衣服、帽子仍可能遮挡新身体和头发。不能靠图像指令增加动画状态或碰撞几何。

```sh
cargo run -p outfit-cli -- character-encode \
  --identity /private/identity.json \
  --body /private/body.png --front-arm /private/front-arm.png \
  --back-arm /private/back-arm.png --head /private/head.png \
  --hair /private/hair.png \
  --backup-out /private/identity.backup.json \
  --format json --out /private/identity.patch.json --stats

# 只改发型；也必须提供原 identity
cargo run -p outfit-cli -- character-encode \
  --identity /private/identity.json --hair /private/hair.png \
  --format command --out /private/apply-character.txt
```

`--format` 支持 `json`（patch）、`merged`（完整 identity）、`command`（本地原子合并命令）和 `directives`（仅指令字段，可能不足以单独应用）。`--backup-out` 以私有权限新建文件，逐字节保留输入 identity JSON，并拒绝覆盖；它不代替完整角色存档备份。网页与 CLI 共用三种优化目标；身份文本及导出形式变化复用已有像素编码，压缩估算仍不代表真实网络流量。

人物实现参考了社区的 [Drawable Generator 方案](https://github.com/FezzedOne/FezzedOne-Drawable-Generator/tree/fa9c0450b33b5634e53d133a25dc70dfb54e4a41)及其[使用指南](https://steamcommunity.com/sharedfiles/filedetails/?id=3071843411)。该仓库未见明确许可，本项目未复制其源码或人物模板；帧布局、采样常量和共享组从本地合法资产独立推导。素材与软件许可边界见 `LICENSE`。

五张 `*-reference.svg` 的原版像素取自用户明确允许使用的本地合法游戏资产，仅供帧布局对照；这些图像仍归 **Chucklefish／原权利方**，不因本项目 MIT 或 Hatter 算法 CC BY 3.0 许可而重新授权，不授予额外分发或使用权。独立生成的空白 PNG／纯标签布局与其中的原版美术应分别看待。它们也不同于帽子试戴原有的两张 Hatter 单帧预览：后者来源与用途仍按上节及 `LICENSE` 的独立说明保留。

参考示例固定为 **human／male／hair male1**。头部将原版静态头与对应表情叠加；身体、双臂、头发各展示对应原版图层。相同编码 ID 复用该组首个 canonical 源帧，因此不是原版精灵表逐格复刻；人类原图中真实透明的动作标注 `empty`，不补造像素。

图像参考更新已用实际 Chromium 验证：五张参考覆盖 **143 个目标帧格、1,057,628 次渲染通道比较**；五个真实 SVG 下载与发布源 SHA-256 一致，320／375／768／1024／1440px 无整页横向溢出，预览区域可键盘滚动。原人物导出命令全文字节不变，原生／WASM Clippy 与正式构建通过；开发服务器补齐 `image/svg+xml`，修复 SVG 被当作二进制而无法预览。公网 HTTPS、图片加载和下载通过。本次未改编码，也未重跑游戏引擎门禁。


## 结构

| 模块 | 职责 |
| --- | --- |
| `crates/outfit-core` | 借用 RGBA 的编码、严格多层原图／目标图改色、同时替换语义、无损调色板、固定模板与人物帧指纹、候选选择、PNG／物品／身份导出 |
| `crates/outfit-web` | Rust DOM 界面、本地上传/拖放/预览、浏览器压缩估算、导出与最新输入缓存 |
| `crates/outfit-cli` | 原生 PNG 编码、只读 SBAsset6 解析、独立资产模型验证、原版/OpenSB 原生引擎验证 |
| `xtask` | WASM 构建、绑定生成、静态资源打包和仅绑定 loopback 的本地服务器 |

`outfit-core/data/legacy.json` 保存从原实现提取的固定指令、帧布局和生产物品描述器，仍是服装编码必需数据，与已移除的帽子旧分块模式无关。它不依赖旧 TypeScript，也不包含商业游戏 PNG。旧 Angular、TypeScript、Bun 配置及 Python 验证器已移除。两个 `tools/*-verify.lua` 保留：它们在真正的游戏 Lua 环境调用原生图像 API，不是旧应用运行时或编码器替代品。

## 编码与复用契约

| 目标 | 选择规则 |
| --- | --- |
| `text`，默认 | 衣物按完整 directives 原长选择辅助图与调色板方案，上衣身体／双袖联合比较；帽子与人物保持原有独立语义。只压缩最终赢家一次供统计 |
| `compressed` | 衣物保留旧候选，追加有界调色板别名分配／子集与安全排序；每固定 profile 最多 80 个，auto 最多 240 个，实际去重可更少。逐完整指令按真实 Deflate 估算、原长、稳定顺序择优。帽子／人物仍最多16个（palette off为8） |
| `passes` | 禁用别名调色板；每个导出指令字段仅一个像素赋色阶段，固定模板自身操作不变。衣物跨允许的辅助图按完整原长择优，只压缩赢家一次供统计 |

只在同一个 `replace` 内重排唯一源 RGBA 映射；重复源键阶段保持顺序。保留服装的 JS 整数属性键顺序、重复源键优先级、帧编号、图层覆盖顺序和透明地址 RGB，不修改服装原始浮点指纹链。帽子使用 `[x,0,y,0]` 地址，人物使用 `[x,frameID,y,0]`，不减色；人物共享帧限制见布局参考。

帽子与人物在 `text`／`passes` 下使用最短合法地址颜色写法，例如 `11002200` 与 `1020` 表示完全相同 RGBA；不截断通道，也不合并帧 ID。`compressed` 保留原固定八位地址候选，因为较短的文本不一定更容易压缩。衣物仍使用原有颜色规范写法；新衣物 API 在一次共享多层扫描后搜索辅助图与严格等价的调色板分配，保留旧候选并稳定处理平局，无收益不强行换方案。不改变帧识别或浮点链；这是有界候选最优，不是所有编码的全局最优。独立直接改色没有坐标画布、辅助图、调色板别名或压缩目标设置。

`text` 下 RGB3 别名长度固定，逐色收益可直接相加并选取最大收益分配；不会为等长的别名排列反复构造大指令。`passes` 同样只比较每种辅助图的直接编码。只有 `compressed` 才展开额外分配、子集和排序，因为 Deflate 大小不能按逐色收益相加。

浏览器使用真正的 `CompressionStream('deflate')`，不支持时明确报错；原生 CLI 使用 flate2 的 zlib level 6。不同压缩实现可能选择不同候选，二者均须严格 RGBA 等价。估算对象只是完整 UTF-8 directives，**不是物品 JSON、实际装备包、双向流量或 FPS**。

衣物默认 `profile=auto`，联合比较以下三个合法固定模板；明确选择 `original`／`conservative`／`compact` 时严格限制在该模板。相同评分和原长保留稳定候选顺序，auto 首先保留原默认 conservative 的旧候选。网页结果显示真实选中 profile、实际比较候选数与搜索上界；CLI `--stats` 的 `profile` 为选中值，`profileChoice` 为请求值。图片缓存键区分 auto 与所有固定设置，JSON／命令／复制／下载复用同一赢家；帽子、人物和直接改色不参与此搜索。

auto 只按当前选择的目标择优，不把辅助图解码内存、网络包或帧时间混成一个分数；压缩目标可能为了更小的 Deflate 估计选中较大的 original 辅助图。需要固定小图资源成本时，明确选择 conservative／compact，而不是把 auto 当作最低内存模式。

| 辅助图配置 | 行为 | 两张辅助图完整解码后的 RGBA 缓冲合计 |
| --- | --- | ---: |
| `original` | 上游 apeboss + key | 9,026,816 B |
| `conservative` | 竖采样换成 RGBA 完全相同的 glitchT5blocks 采样 | 985,720 B |
| `compact` | 横采样再换成 translocator 小图 | 38,008 B |

这些是辅助图片缓冲量，不是游戏进程总内存。compact 的中间 alpha 不同，等价性依赖正常指纹和后续透明度处理；资源包替换采样像素或底衣可能破坏任一固定模板。生成的装备仍只引用原版自带资源，不需要输出专用 PNG 或新物品定义。

只缓存最近一组不可变输入；键包含有序图片身份、布局、服装类型、辅助图、调色板和策略，不扫描或复制像素计算缓存键。进行中和已完成结果共享；输入/设置变化失效，失败可重试。上传、导出和剪贴板均防止旧请求完成后覆盖新状态；剪贴板写入串行化。界面显示真实的新计算/进行中复用/已完成复用状态。

人物完整身份合并只复制最终保留的字段，不先复制即将被替换的大段旧指令；未修改的 null 元数据保持存在，只有明确的 `imagePath: null` patch 才删除该可选字段。重复编辑固定图源不会不断增长未修改发型或面部的指令。本地原子命令直接写入单一缓冲，避免大字段的临时转义字符串和命令列表拼接；CLI 与网页仍保持相同输出。

## 原生 CLI

```sh
# 与网页相同的默认 auto/text，写出物品 JSON 和统计
cargo run -p outfit-cli -- encode --garment pants --input /path/pants.png \
  --format json --out /tmp/outfit.json --stats

# 三个部位组合；各部位也可单独提供
cargo run -p outfit-cli -- encode --garment chest \
  --body /path/body.png --front-sleeves /path/front.png --back-sleeves /path/back.png \
  --profile compact --strategy compressed --format command --out /tmp/outfit.txt

# 帽子；默认 text/auto，不使用服装的 --profile
cargo run -p outfit-cli -- encode --garment hat --input /path/hat.png \
  --name '^orange;Custom Hat' --description 'My hat' --rarity rare \
  --remove-hair --hide-body --strategy compressed --format command \
  --out /tmp/hat.txt --stats

# 直接改色；第 n 个 source 与第 n 个 target 配对，所有图层联合检查
cargo run -p outfit-cli -- recolor \
  --source /path/body-original.png --target /path/body-edited.png \
  --source /path/arm-original.png --target /path/arm-edited.png \
  --out /tmp/recolor-directives.txt --stats

cargo run -p outfit-cli -- --help
cargo run -p outfit-cli -- character-encode --help
cargo run -p outfit-cli -- native-verify --help
cargo run -p outfit-cli -- recolor --help
```

类型为 `pants`、`hidingPants`、`chest`、`back`、`hat`。可用 `--palette off` 禁用调色板，`--format directives` 只输出指令。帽子只有一个编码路径，不再接受 `--hat-mode` 参数。帽子专用参数不能混用于其它服装，服装层与 `--profile` 也不能混用于帽子。JSON 保存实际名称/描述；命令导出再处理原版聊天解析所需的反斜杠、双引号、单引号，不预先污染描述器字符串。

### 只读真实资产与原版引擎

游戏资产和原版二进制由使用者合法提供；不把 `packed.pak`、解码图像或私人角色加入项目、Nix store、网站或 CI。

```sh
cargo run -p outfit-cli -- verify-assets --pak /absolute/path/packed.pak \
  --out /tmp/outfit-assets-report.json --engine-dir /tmp/new-private-fixture

# 仅验证帽子；覆盖稳定画布、尺寸边界与自动裁帧
cargo run -p outfit-cli -- verify-assets --hats-only --pak /absolute/path/packed.pak \
  --out /tmp/hat-assets-report.json --engine-dir /tmp/new-private-hat-fixture

# 仅验证人物；涵盖男女图源、双臂、表情、发型、别名与原头部清除
cargo run -p outfit-cli -- verify-assets --characters-only --pak /absolute/path/packed.pak \
  --out /tmp/character-assets-report.json --engine-dir /tmp/new-private-character-fixture

# 必须在隔离的 Linux 游戏运行环境执行，不能指向现有世界/存档
cargo run -p outfit-cli -- native-verify --mode vanilla \
  --server /absolute/path/starbound_server --pak /absolute/path/packed.pak \
  --fixture /tmp/new-private-fixture --run-dir /tmp/new-private-run --measure-images
```

`--engine-dir` 仅接受项目外的新目录或空目录；它包含编码游戏图像，必须私有，使用后清理。资产解析器只读 SBAsset6 索引和指定条目，处理大小写、索引边界、重叠、实际 `frameGrid`/`frameList` 和 PNG；不会解包整份游戏。不会因袖子 `.frames` 声明了未使用的空行而误要求 PNG 也包含该空行。

Rust 原生启动器使用新私有存储、loopback 临时端口、随机 RCON 密码和两个允许的 CPU；保持原版 safeScripts 与 Lua 指令限制。它实现真实 RCON 分帧/认证/多包响应和完整进程关闭；只有全部 PASS、无 FAIL、计数完整且正常退出才写成功报告，不以 exit 0 代替验证。OpenSB 模式还需要 `--opensb-assets /path/to/opensb`。

## 人物重绘验证

- **80 项 Rust 测试**、原生与 WASM Clippy `-D warnings` 通过。原有帽子／裤装的 **137 组完整 CLI 输出逐字节不变**，网页旧模式也与同参数 CLI 对齐。
- 真正原版 **1.4.4**：**216 个源帧／别名、648 批、3,816 次精确颜色足迹、1,944 次透明 RGB 探针**全部通过，进程正常退出，控制台和引擎日志无 `[Error]` 或 `FAIL`。首轮曾触及私有测试单元的 16 GiB 内存／4 GiB swap 上限而 OOM，未计通过；保留失败日志，仅将资源上限调整为 22 GiB／12 GiB 后完整重跑，没有缩小矩阵、修改期望图像或跳过源帧。
- **OpenStarbound 0.1.15.1**：同样 **216 个源帧／别名、648 组变体、1,198,152 个像素、4,792,608 次通道比较**全部通过并正常退出。原版与 OpenSB 的 190 个原始帧指纹对照发现 16 行差异；新增 12 个不冲突的指纹键，统一映射为相同帧 ID，不需要选择引擎或导出两套人物指令。
- 两个实际 **OpenSB 非管理员客户端，强制 legacy 协议，连接只加载原版 `base.pak` 的真正原版服务器**：另一客户端实际观察到完整身体、双臂、头部／表情和头发；六个原版资源引用及完整 directives 与导出逐项一致。撤销命令恢复原始身份，正常保存退出后重入仍保留新身份。**这不是两个原版客户端的实测**，也不代表验证了所有显卡、资源替换包或动画配置。
- 实际 Chromium 验证 **143 个帧预览的完整 RGBA**、独立图层合成、六种模式五种视口宽度、真实剪贴板和下载；覆盖四种身份 JSON 导出、合并／备份仅开放 JSON、撤销、缺失身份、冲突帧、仅头发、拒绝剪贴板后的下载兜底，以及约 237 KB 完整身份的再次编辑。空白 PNG 模板仍为透明图；新增人物图像参考的用途与资产权利见上节。私有存档和原生探针包不公开。
- 人物功能验证阶段没有修复旧帽子 256 边界或旧衣物 Floran `jump.2` 差异；该阶段失败记录保留。后续修复分别见“帽子 256 跨引擎采样修复”和“衣物跨引擎帧识别修复”。

## 头部表情误合并修复

`sad.2/sad.3` 与 `neutral.2/oooh.1` 原来分别共享 ID9、ID13，原因是旧头部采样指纹碰撞，不是这些原版帧完全相同。只读合法 PAK 确认两组原始 RGBA 分别有 2、4 个像素不同；用户提供的 `deepseek_emo.png` 对应有 2、34 个可见像素不同，尺寸与有效格位置正确，不能靠删除校验或覆盖其中一帧处理。

头部改为统一 `?scale=0.48?scale=0.61?scale=0.56?crop;4;4;5;5` 采样，29 个 canonical 源可分别识别。保留原有 27 个 ID，仅将 `sad.3` 分配 ID28、`oooh.1` 分配 ID29；其余布局位置、sourcePaths、6 个真正的原版别名及 replace 后画布链不变。两实际引擎的全部 29+6 个源帧／别名采样完全相同，新识别表只保存这些实测键，不把 CPU 近似值伪记为原生观测。

新旧都只引用游戏原有资源，无额外游戏资产或客户端／服务器脚本。该头部修复阶段未修改身体、双臂、头发、衣物、帽子和直接改色的编码链；身体与前臂的后续拆分见下节。真正的帧别名仍受同图约束；这是增加对既有不同原帧的区分，不是添加动画状态。旧 identity 内的旧指令不会自动变更，需要重新导出并应用完整关联重绘。

头部标签布局、图像参考和透明 PNG 模板同步更新：两张新独立参考分别由真实人类男性头与对应表情合成，原有 27 张内嵌 PNG 字节不变；透明模板仅迁移两格共 3,698 个隐藏绿色通道，红、蓝、alpha 和格外像素不变。其它四类 12 个参考／模板文件字节不变，原美术许可边界保持。

该用户原 PNG 未修改，已实际通过 CLI 和浏览器上传／完整 JSON 导出；4,811 个可见像素、54 色保留。四个冲突帧的浏览器预览与独立 PNG 裁片逐 RGBA 相等；压缩／最少遍数输出分别检查全部 29 帧的 53,621 个地址，含未赋色透明地址。合成 identity 与透明身体／双臂仅用于隔离验证，不是替用户生成可直接应用的完整角色。

`idle.1` 格（PNG 左上坐标 x=301、y=43）在该输入中完全透明，站姿组合预览的头层因此为空；这属于有效的透明画作，未自动补画或覆盖。若希望该站姿显示头部，需要用户在此格绘制。

- **114 项 Rust 行为测试**、原生／WASM Clippy `-D warnings` 和正式构建通过。独立头部回归按格子位置而不是指纹 ID 生成不同画作，覆盖全部 29 格、三种策略、两种调色板状态、alpha1／128／254／255 与未赋色地址；保留当时其它部位的共享拒绝校验，不重 pin 旧头部取样数值。
- 完整保留 **216 个原有案例**，追加该用户原图的 4 个冲突帧，共 **220 案例／660 变体**。同一固定 OpenSB 完成 **1,220,340 像素／4,881,360 通道**全 RGBA 比较；真正原版完成 **4,215 次精确颜色足迹／1,980 次透明地址探针**，所有进程正常退出、无 `[Error]`／FAIL。原版仍不是全 RGBA 引擎 readback，也不是新双客户端联机证明。
- 原版首次单进程运行完成195个案例后达到2400秒私有单元上限，峰22 GiB／swap4.2 GiB，仅记部分失败运行。随后按最多32例分为7个独立进程、两条受限队列，完整执行同样220例／660变体；逐项确认案例对象、变体组和预期 PNG SHA 不变，不删帧、不改图、不提高 Lua 限额。最终峰值最高6.9 GiB、swap0，属于密集诊断缓存，不是单个角色的内存。
- 已发布至 **https://198.44.178.249:8443/**，版本 `20261021T011344Z-head-poses`。公网原始 PNG 上传、完整 JSON 与最终 CLI 一致；真实剪贴板和 **67,023 字节 JSON 下载** SHA 一致，实际375px表情画面可见且无横溢、无应用运行错误，HTML／WASM／三张头部模板与构建哈希一致且正式 TLS 校验0。未改 Caddy／端口／证书，旧 release 保留。
- 证据在 `~/.cache/outfit-head-fingerprint-fix/` 的原图冲突／源帧比较、双引擎采样、分片等价证明、全部引擎日志、浏览器与发布报告。用户原图 SHA-256 始终为 `3e6494124f27ffd94526b1cee3533ea1f145f41078d80f2860287e8991b9a25d`；用户画作和商业源帧不纳入仓库内的测试素材、公开资产或发布包。

## 身体与前臂误合并修复

用户 `身体.png` 的 `walk.3/walk.5` 被旧采样合并为 ID10，但原版男性与女性身体源帧均有 **69 个原始 RGBA 像素不同**，用户画作有 63 个可见像素不同。完整检查发现其余身体共享组和前臂 `walk.2/jump.4` 也都是弱采样误合并，不能把它们当成不可突破的原版同帧限制。

身体、双臂和待抑制的原头统一改用 `?scale=0.61;0.42?scale=0.4;0.6?scale=0.75;0.6?scale=0.47;0.77?scale=0.61;0.71?crop;1;1;2;2`，随后沿用原画布链。两实际引擎各完整捕获 **179 个源帧／别名**；155 个键各自有效，实际联合 193 个键无跨 ID 碰撞。原版通过候选完整 RGBA 的正／负精确替换唯一确认，OpenSB 实际读取像素；只保存原生实测值，不把 CPU 近似值伪记为原生来源。

保留原先每组首帧 ID，只将身体 `walk.5/6/7` 分别分配99/100/101、`run.7`→102、`fall.1/2`→103/104、`climb.5/6/7`→105/106/107、`swim.4`→108，以及前臂 `jump.4`→109。身体44格、前臂34格均可独立绘制；后臂 `walk.1/2/3`、`run.1/2` 的完整原始 RGBA 确实完全相同，继续共享 ID74，不删除真实冲突校验。真正的 `.frames` 别名、坐标、sourcePaths、旧头部29格识别和其它模式不变。

身体／前臂的六张模板与参考同步迁移。11个新参考来自对应的人类男性原版帧，原有67张内嵌 PNG 字节不变；78格参考逐 RGBA 校验时显式沿用既有的透明像素 RGB 清零约定，不冒充原游戏隐藏 RGB 原样副本。透明上传模板仅更改11格共20,339个隐藏绿色 ID 字节，R/B/alpha 与格外像素不变。其它九个部位资产与既有头部修复保持不变。

用户原 PNG 未修改，**5,051 个可见像素、9 色**均保留。全部44格浏览器预览与独立原图裁片一致；三种策略／两种调色板状态均验证身体和双臂208,937个完整地址像素、头部53,621个地址像素。完整 JSON 与 CLI 相同，真实剪贴板和133,640字节实际下载一致。此验证使用合成 identity、透明双臂和先前的表情输入隔离问题，不提供这种会隐藏双臂的测试补丁作为用户角色成品。

该身体图的 **`climb.1` 至 `climb.8` 全透明**；攀爬时该身体层将为空。透明输入是有效画作，未擅自补画、复制其它姿势或覆盖原图。若希望攀爬时身体可见，需要在对应八格绘制。

本次114项 Rust 行为测试、原生／WASM Clippy `-D warnings` 与正式构建通过。新的身体／双臂回归按部位和格子位置生成独立画作，不按 ID 自证合并；覆盖全体格子、三策略、调色板两状态、alpha1／128／254／255、透明地址及真正共享空帧的拒绝校验。旧身体采样值实现断言删除，不重新 pin 新数值。

- 完整保留216个原有源帧／别名案例，追加真实身体原图全部44格（各验证男性、女性两个源）和前次表情原图的4个冲突帧，共 **264案例／924帧变体**。同一固定 OpenSB 完成 **1,708,476像素／6,833,904通道**的全 RGBA 比较；真正原版完成 **5,715次精确颜色足迹／2,772次透明地址探针**。全部进程正常退出、无 `[Error]`／FAIL；这不是原版全 RGBA readback 或新的双客户端联机证明。
- 原版按最多24例分为11个独立进程、两条受限队列；最终夹具内所有案例对象、变体组、期望 PNG SHA 在分片前后完全一致，不删帧、不修改期望、不提高默认 Lua 限额。两队列均峰值5.9 GiB，属于密集诊断缓存，不是单个角色的内存。
- 已发布 **https://198.44.178.249:8443/**，版本 `20261021T025310Z-body-poses`。公网直接上传未改原图、完整 JSON／真实剪贴板／133,640字节实际下载均一致；375px实际画面显示独立的 `walk.5` ID63（十六进制），无横溢、无应用错误。本地七模式五宽度与手机200%文字共37种布局通过。HTML／WASM及身体、前臂、头部模板等14个静态文件 HTTP200、TLS校验0、哈希与正式构建一致；未改端口／Caddy／证书，旧 release 保留。
- 证据在 `~/.cache/outfit-body-fingerprint-fix/` 的原图完整源帧比较、双引擎采样、分片等价证明、全部引擎日志和浏览器／发布报告。原身体 PNG SHA-256 始终为 `15a91c81959740228418d6ea8003053cc039d489e6f41e31002f5493af591fcc`；用户画作不进入仓库测试素材或公开发布包。旧网页需强制刷新并重新上传原 PNG，旧 identity 指令需重新导出并应用，不会自动修正。

## 帽子 256 跨引擎采样修复

旧链的首次分歧是双线性 `scale=256;256`，不是 `xsmall.png` 颜色短写、PNG 解码或最终坐标错误。同一个 `moderncleanup-vps` OpenSB 二进制的实际指令是 `rcpps` 近似倒数加商残差修正，不是正确舍入的 IEEE 除法；原生观测 `(256,0)` 为 `[254,0,0,0]`，原版为 `[255,0,0,0]`。略低于 1 的商乘以 255 后向整数截断，合并了最后一列／行的颜色地址。仅用精确除法及正确舍入倒数乘法的旧 CPU 模型漏掉了这个执行方式。

新链对所有尺寸统一使用双线性 512 倍放大、动态裁剪和 `scalenearest=0.498046875`（精确的 `255/512`）。每轴宽度 `W` 的裁剪长度为 `round(512W/255)`，从坐标 1 开始；最终像素 `x` 对应放大图中的 `1+round(512x/255)`。内部采样离颜色量化边界约有四分之一通道值的余量，最后一格自然进入端点钳制区；不是对坐标 255 打补丁。最终仍为原尺寸，未赋色像素仍保留 `[x,0,y,0]` 地址。

不修改游戏、不加入新资产，不要求选择原版／OpenSB，不恢复 Legacy 路径；alpha1、半透明、纯白、原有三种策略和 86×215 自动裁帧语义不变。代价是增加一次最近邻操作，最大双线性中间图由 512×512 增至 1024×1024（RGBA8 约 4 MiB），不宣称压缩率、FPS 或游戏总内存改善。

**已生成的旧帽子不会自动更新**：需要用新版网页／CLI重新生成并替换物品实例内的旧指令。衣物帧识别、人物重绘及直接改色不使用这条帽子画布链。

- **113 项 Rust 行为测试**、原生／WASM Clippy `-D warnings`、正式构建通过。独立解释器现在解析并执行 helper、blend、replace、双线性、裁剪及最近邻，不再由前缀直接假定理想地址；回归明确区分旧链精确除法的 255 与近似商修正的 254。全尺寸精度回归覆盖 1–256 的细行／列、最大正方形、自动裁帧，以及 byte／normalized、四种除法与五种 lerp（含两向 FMA）。
- 原有 **11 个案例的预期 PNG 字节及 SHA-256 完全不变**，追加最大细行、细列、四边条与全透明 4 个案例。独立真实资产 CPU 检查通过 **15 cases／156 个不同候选／2,475,509 坐标**；24 组旧／新 CLI 输出的 797,532 个赋色地址语义一致，text／passes 的 16 组像素赋色后缀逐字节不变。
- 曾出错的同一个 **OpenSB 0.1.15.1 / moderncleanup-vps** 构建完整通过 **15 cases／94 输出变体／1,546,408 像素／6,185,632 通道**全 RGBA 比较；真正原版 **1.4.4** 完整通过 **94 批／510 次精确颜色足迹／300 次透明地址探针**。两基底及三种策略、关闭调色板都保留，等价输出去重；默认 Lua 限额不变，日志无 `[Error]`／FAIL，正常停止。原版仍不是全 RGBA 引擎 readback。OpenSB／原版门禁分别约 22.6 秒／145.1 秒、峰值 463 MiB／3 GiB、swap0，属于密集诊断，不是单件物品内存。
- 实际 Chromium 的 7 类上传 × text／passes 共 **14 份完整 JSON**与最终 CLI 一致；最大图两种调色板状态的 compressed 输出逐 RGBA 等价，独立真实 Deflate 与页面计分一致（浏览器和原生压缩器可选不同赢家）。隐藏身体／去头发、元数据修改复用编码，真实剪贴板读回与 **1,026,595 字节命令下载**和同参数 CLI 完全一致；七模式五宽度及手机双倍字号无横溢，零运行错误。
- 证据在 `~/.cache/outfit-hat256-fix/`。旧链原版阶段对照完成 8,289 坐标后达到诊断单元时间／内存限额，只记部分调查，不计完整门禁；原始失败及超时日志保留。上面的完整通过来自最终 CLI 重新生成的全部案例，不是缩小旧矩阵、改预期图、重 pin 或修改游戏引擎。
- 已发布至 **https://198.44.178.249:8443/**，版本 `20261020T091242Z-hat256-sampling`。公网实际最大帽子 JSON／命令与最终 CLI 一致，真实剪贴板及 **851,491 字节命令下载** SHA 一致；375px 无横溢，HTML／CSS／WASM 均 HTTP200、正式 TLS 校验0且哈希与构建相同。旧 release 保留，Caddy、证书和端口未改。

## 衣物跨引擎帧识别修复

Floran 裤装 `jump.2` 的首次分歧发生在第二遍双线性 `scale=0.7`。原始像素和前一遍 `scale=0.4` 对下一遍有贡献的四个像素均相同；不是帧 ID、上传 PNG 或最终画布坐标错误。

| 阶段 | 原版 1.4.4 | 已测 OpenSB 0.1.15.1 |
| --- | --- | --- |
| 采样 RGBA | `2b243668` | `2c243668` |
| 旧识别表处理后 | `ffd2ff00` | 未命中，保留原采样 |
| 新识别表处理后 | `ffd2ff00` | `ffd2ff00` |

旧输出中的 green=36 是未识别采样本身的绿色通道，不是正确识别到“第36帧”。修复在生成器中进行，不修改 OpenSB 或添加游戏资产：保留原采样、旧键、帧 ID、辅助图和画布数学，将两引擎原生观测到的等价指纹映射到同一既有 ID。同一个输出同时支持两引擎，不需要选择引擎，也没有针对 `jump.2` 的条件分支。

`crates/outfit-core/data/garment-fingerprints.json` 保存196个实际源的双引擎数值观测，覆盖全部344个映射帧去重后的源集；共新增12个不冲突键，包含普通裤装1个、隐藏身体裤装1个和上衣10个。初始化依据原布局单元和固定源族校验完整覆盖、重复、ID、采样 stem、目标 seed 及跨引擎碰撞，随后释放元数据，仅保留共享指令串。资产路径按游戏规则忽略ASCII大小写，例如实际 `pantsM.png` 与规范索引的 `pantsm.png`。

实际 OpenSB 解析后的14个相关 PNG／`.frames` 原始字节 SHA 与合法 PAK 一致；仅 `lay.1` 别名补丁不影响这些映射单元。Floran／Frogg 未映射的16个 climb 源另做双引擎32样本检查，新增键命中0，不改变原有处理，也不宣称新增了 climb 编辑支持。

**已经生成的物品不会自动更新**：指令保存在物品实例内。使用修复后的网页／CLI重新生成并替换旧物品；`original` 仅指辅助图配置，也使用修复后的共同识别表，不是保留旧缺陷的兼容模式。衣物修复阶段未修改帽子边界；后续帽子修复见前节。

- **112 项行为测试**、原生／WASM Clippy `-D warnings`、正式构建通过。回归把全部原生观测输入实际识别表，再经过固定画布与真实像素编码检查可见 RGBA、alpha1 和未着色透明地址；另独立固定 `jump.2` 的两种真实采样和既有 `d2` 契约。16组自有图像的四类衣物／四种配置，像素赋色后缀全部逐字节不变，仅在识别阶段扩展必要键。
- 真实资产独立 CPU 模型继续通过9 fixtures／344映射帧、1992候选／141,618,608坐标RGBA及旧162测量／11,449,008坐标检查。实际网页四类衣物完整JSON与最终CLI一致；固定配置的三策略保留共同指纹，真实剪贴板／命令下载字节一致，导出复用编码；七模式五宽度及大字号无横溢。
- 曾失败的同一个 **OpenSB 0.1.15.1 / moderncleanup-vps** 构建已实际通过全部衣物：9 fixtures、344映射帧、每帧21变体，共7224帧变体、13,357,176像素、53,428,704通道比较；包括原始画布和baseline的额外全RGBA断言，日志无 `[Error]`／FAIL且正常停止。按fixture独立进程保留全部输入和变体，不是只测 `jump.2`；未执行无关帽子／人物门禁。
- 真正原版 **1.4.4** 同样完整通过9 fixtures／344映射帧、8变体各344帧、688批／2752帧变体、27968精确颜色足迹／7824透明地址探针；Floran `jump.2` 保持原帧ID210。默认Lua限额不变、无 `[Error]`／FAIL、正常关闭。SSH监督连接曾断开，但独立unit继续运行并以退出事件恢复监督，没有重跑或用exit0代替PASS；原版仍不宣称全RGBA引擎readback。
- 公网默认auto生成的完整裤装JSON、命令与最终CLI／本地验证一致，真实Clipboard读回和419471字节命令下载SHA一致；375px无横溢、无运行错误，WASM 200／TLS校验0且与构建哈希一致。证据在 `~/.cache/outfit-jump2-fix/` 的阶段采样、资源身份、CPU、`vanilla-fixed/`、`opensb-fixed/` 和浏览器／发布报告；原始失败日志保留，私有商业夹具和运行存储不发布。


## 衣物自动辅助图与调色板搜索验收

- **105 项 Rust 行为测试**、原生／WASM Clippy `-D warnings`、正式 WASM 与优化版 CLI 构建通过。逐色独立 RGBA 模型覆盖新别名分配、颜色子集、最终颜色碰撞、alpha1、透明地址和 RGB3 别名空间耗尽；保留全部旧候选，不改变帽子／人物／直接改色。
- 自有两组完整精灵表覆盖四种衣物与完整三层上衣：**144 组固定配置均不劣于旧 CLI，48 组 auto 都达到相同目标下三种新固定配置中的最优值**。固定配置的 text／passes 共96组完整输出逐字节不变。旧、新 CLI 的帽子、完整人物身份和直接改色输出另逐字节一致。大矩阵使用 release CLI；debug 版300秒超时不计通过。
- 实际 Chromium 对四种衣物 × 三策略共12组输出独立比较 **1,053,930 个完整 RGBA 地址**，包括未赋色透明地址；七模式五宽度35状态及320／375px根字号200%无横溢。text 实际只调用一次 CompressionStream；上衣 compressed 实际调用156次与显示的156候选一致。切换 auto／固定配置失效，JSON／命令／真实下载复用赢家，真实 Clipboard 读回正确。
- 真实资产独立 CPU 检查 **9 fixtures、216 组合、1992 个完整候选、141,618,608 个坐标 RGBA**，并保留旧162测量／11,449,008坐标检查。真正原版 **1.4.4** 的8个变体各覆盖全部344个映射帧：**688批、2752帧变体、27968颜色足迹、7824透明地址探针**通过；含三种新 auto 赢家和五种旧变体。默认Lua限额不变，日志无 `[Error]`／FAIL，正常停止；14m9.826s、峰8.7G／swap0属于密集诊断缓存，不是单件衣物内存。
- 公网实际三层上衣压缩输出与本地验证结果逐字节相同，真实 Clipboard 读回／861844字节JSON下载 SHA 一致；375px无溢出、无运行错误，正式 HTTPS WASM 200／TLS校验0且与本地哈希一致。该轮原版检查不是引擎全RGBA readback或新双客户端联网门禁，当时没有重跑／修复旧 OpenSB 256 和 Floran `jump.2` 差异；后续衣物修复见上节。

同一组自有五色样本，Chromium 独立 Deflate：

| 衣物 | 旧可用候选最小 | 新搜索 |
| --- | ---: | ---: |
| 普通裤装 | 81646 B | 80902 B |
| 隐藏身体裤装 | 81663 B | 80920 B |
| 完整三层上衣 | 152711 B | 152455 B |
| 背饰 | 98292 B | 97768 B |

旧可用候选取旧 CLI 各固定配置／palette 的压缩赢家，再在同一浏览器重新计分；不是穷举所有旧浏览器候选、实际网络包或FPS。完整证据 `~/.cache/outfit-garment-search/`。

## 颜色短写、帽子辅助图与直接改色验收

- **96 项 Rust 行为测试**、原生与 WASM Clippy `-D warnings`、正式构建通过。旧四种服装 × 三种 profile × 三种策略 × 两种 palette 的 **72 组完整 CLI stdout 逐字节一致**。短写回归检查隐藏地址、alpha1、全部旧压缩候选仍在，像素语义不变。
- 真正原版 **1.4.4** 帽子完整门禁：**11 case、两种原版基底、66 批、358 次精确颜色足迹、192 次透明 RGB 探针**通过，包含最大256、纯白、alpha1、自动裁帧。xsmall 替代只改取色基底及同义颜色，未改256缩放／裁剪数学。最终 CLI 重新生成的全部15个夹具文件与实际运行输入逐字节一致。
- 真正原版直接改色 primitive：**5 批、13 次颜色足迹、7 次透明 RGBA 探针**通过，覆盖四色同时映射、alpha0隐藏RGB变化、alpha1/128/255、两色交换、真正空串，以及原版 eyepatch 头部 normal 帧的全部隐藏色。两套引擎日志均无 `[Error]` 或 `FAIL`，正常关闭；这是精确足迹与透明样本检查，不是引擎全像素四通道读回。
- 实际 Chromium 检查 **七模式 × 五宽度共35状态**无整页横向溢出；原图／目标图上传和真实拖放、与CLI的62字节结果一致、跨配对冲突和恢复、真实空字符串复制与零字节下载、延迟旧上传／删除配对不回写均通过。
- 公网独立改色已实际核对与原版测试过的 CLI 完全一致：62字节输出真实剪贴板读回／下载 SHA-256 一致，原图与目标一致时复制真正空串、下载真正0字节；375px无溢出、无运行错误，HTTPS WASM 200／TLS校验0且与本地构建哈希一致。此前六模式的可读性设计保留，直接改色也通过320／375px根字号200%布局检查。
- 自有76着色像素帽子样本：文本目标原1253B／Deflate334B，新1175B／296B；压缩目标原1456B／330B，新1378B／292B。目标分别择优，非同一指令同时达到两个最小值，不代表实际网络包或FPS。
- 颜色优化阶段没有重跑或修复旧 OpenSB 256 边界失败、旧衣物 Floran `jump.2` 差异，也不宣称重新执行完整人物原生矩阵。历史证据在 `~/.cache/outfit-color-implementation/`；后续衣物修复见“衣物跨引擎帧识别修复”。私有原生夹具不进入站点。

## Hatter 稳定画布验证（旧 beamend 阶段历史）

- 移除旧分块编码、模式类型、CLI 参数和网页开关后，**65 项 Rust 行为测试**、原生与 WASM Clippy `-D warnings` 通过。画布回归独立模拟 byte/normalized RGBA、两种 lerp 与除法/倒数乘法，检查 43×43 和 256×256 的每个像素，防止边界地址合并。
- 清理前后实际 CLI 的 **128 组完整 stdout 字节数与 SHA-256 一致**：21 组输入覆盖 3 种策略 × 2 种调色板设置，另有带引号、多语言元数据的 JSON/命令导出。旧 `--hat-mode modern` 和 `--hat-mode legacy` 均拒绝，且不创建输出文件；没有保留兼容别名。
- 联合资产检查仍包含原来的 9 个服装样本、162 组、11,449,008 个坐标；另有 **11 个帽子 case、50 个不同候选、665,065 个 RGBA 坐标检查**。
- 真正原版 **1.4.4**：两种基底上的 **11 个帽子 case、66 批、358 次精确颜色足迹、192 次透明 RGB 样本检查通过**，服务器正常关闭。覆盖最大 256、细行/列、自动裁帧、半透明、alpha=1 与纯白色。不是重新执行的双客户端联网抓包。
- 未修改的上游画布曾在原版 43×43 的 `(42,1)` 生成前一列地址；该失败日志保留。当前修复版通过上述完整原版门禁。
- **该历史阶段尚未解决 OpenSB 256 边界**：旧报告 `maximum-256-corners-modern` 的 `(255,0)`，red 应为 255、实际为 254。当时没有修改渲染链，也没有跳过最大尺寸、改预期 PNG 或修改 OpenSB 来隐藏差异。后续统一采样链修复与同一故障构建的完整通过见“帽子 256 跨引擎采样修复”；旧失败记录保留。
- 实际 Chromium 155 验证帽子上传、86×215 裁帧、真实剪贴板和命令下载、三种策略、调色板禁用/恢复、375px 暗色布局及原服装输出哈希不变。计数包装器仍调用真正的 `CompressionStream`：首次文本编码 1 次，修改名称/描述/稀有度/头发与身体开关并再次导出仍为 1 次，切换压缩目标后为 7 次、最少遍数后为 8 次；网页不再存在旧模式控件。

下面是同一张自有 43×43、六色、含半透明和 alpha=1 的合成图在 Chromium 155 的实测。基线同样使用修正画布及精确 RGBA；不是与旧 Hatter 丢弃低 alpha 后的结果混比，也不是网络包或 FPS 测量。

| 编码策略 | 完整 directives 文本 | 浏览器 Deflate 估算 |
| --- | ---: | ---: |
| 直接编码、关闭调色板 | 24,276 B | 4,311 B |
| 默认文本最短、无损调色板 | 20,886 B | 3,856 B |
| Deflate 估算最小 | 20,886 B | 3,419 B |

移除帽子旧模式阶段的证据位于 `~/.cache/outfit-legacy-removal/`，包含完整输出差分、联合/帽子独立模型、原版成功日志及实际浏览器记录；较早的上游差分和 OpenSB 失败证据仍保留在 `~/.cache/outfit-hatter-verification/`。那些验证阶段尚未内置人物素材；后续两张单帧试戴素材的归属与验证见“帽子：本地 Hatter”。游戏资产包、私人角色和引擎夹具不发布到网站。


## Rust 迁移时的验证（帽子整合前）

已在 NixOS x86_64 完成：

- **45 项 Rust 行为测试通过**；原生和 WASM 目标均通过 Clippy `-D warnings`，生产静态站点构建通过。
- 旧 TypeScript→Rust 的 **90 组差分**：模板、完整指令和统计逐字一致；**381 个候选字符串及顺序逐字一致**。不强求不同 Deflate 实现选出同一压缩候选。
- 真实资产：9 个样本（7 个单层、2 个完整上衣），162 组，对 **11,449,008 个映射坐标**执行独立同时替换 RGBA 模型，包含未触及的 alpha-zero 地址 RGB。
- 隔离 VPS 上由 **Rust 验证器驱动真正原版 1.4.4 Linux**：344 帧/344 批、**17,480 次逐色检查、4,890 次透明地址检查**通过，正常关闭；另外完成 45 组原生图像请求测量。
- 实际 Chromium 155：PNG 上传和拖放、四类装备、单袖/完整上衣、策略/辅助图切换、精确导出、真实剪贴板读回、真实文件下载、缓存复用、剪贴板拒绝后下载恢复、尺寸错误及恢复、375px 暗色手机布局。没有浏览器应用异常。

原版 source identifier 为 `8cbe6faf22282659828a194e06a08999f213769e`，资产包 SHA-256 为 `325d7d4210667b2a4738d805e858b80d7d55e8f834da773984ca6edb993577fe`。原版图像探针通过 `root.imageSize`、`root.imageSpaces` 和真实指令逐色检测，并检查透明地址样本；隐藏 RGB 的穷举证据来自独立模型，不冒充原版逐像素四通道读回。

引擎报告中的首次/重复 `imageSpaces` 计时包含 RCON、IPC 和调度，并共享辅助图与同路径缓存；不是 OS 冷缓存、客户端纹理上传或 FPS。完整探针的图像缓存也不等于生成服装的稳态内存需求。没有仅凭换成 Rust 或文本变短宣称游戏帧率提升。

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p outfit-web --target wasm32-unknown-unknown --locked -- -D warnings
cargo xtask build
```

CI 使用 Cargo.lock 与固定 Nix 开发环境，不需要商业资产。私有数值/原生日志/截图报告保存在 `~/.cache/outfit-rust-migration/`；临时编码资产、上传二进制和隔离存档在验证后清理，不进入网站或仓库。

## 迁移前的联网证据与保留限制

上一轮使用两个 OpenStarbound `0.1.15.1 / moderncleanup-vps` 客户端、强制原版 747 协议，连接只加载 `base.pak` 的真正原版服务器。双方非管理员，16 套描述器经历 64 次转换和 64 次远端检查，在每阶段第 6 秒全部正确；另有真实背包穿脱两轮。原先第 3 秒的空闲/卸装检查有 26 次未收敛，这些失败保留，不解释成瞬时同步保证。

那轮完整捕获 301,348 条记录、内核丢包 0。下表是同一条纹/半透明样本的两次穿戴期间，A→服务器及服务器→B 中含完整 directives 的 EntityUpdate 帧字节合计相对 original/off 的变化。帧还含其他状态，不是精确剥离的装备字节。

| 装备 | conservative/text | conservative/compressed |
| --- | ---: | ---: |
| Floran 裤装 | 减少 3.69% | 增加 2.77% |
| Frogg 裤装 | 减少 3.68% | 增加 2.69% |
| Avian 上衣 | 减少 5.19% | 减少 7.61% |
| Tiger 背饰 | 减少 5.59% | 增加 4.67% |

因此保留 text 默认，不把独立 Deflate 更小等同于双向网络更小，也不按四个样本硬编码服装类型特例。旧联网证据位于 `~/.cache/outfit-network-evidence/`；**本次 Rust 迁移没有把它冒充重新执行的联网抓包**。

历史 OpenSB 严格图像诊断在原始 original/off 前缀的 Floran `jump.2` 上发现 `(0,0)` 的 green 应为210、实际36。原始失败记录保留，没有 suppression 或单帧特判；后续通用双引擎指纹修复见“衣物跨引擎帧识别修复”。网页成功或静止联网成功仍不等于所有动作、种族、染色及第三方资源包的完整兼容证明。
