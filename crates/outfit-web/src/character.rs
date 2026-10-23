use super::*;
use outfit_core::{CharacterLayer, CharacterPart};
use serde_json::Value;

pub(super) const FIRST_SLOT: usize = 6;
const PARTS: [CharacterPart; 5] = [
    CharacterPart::Body,
    CharacterPart::FrontArm,
    CharacterPart::BackArm,
    CharacterPart::Head,
    CharacterPart::Hair,
];
pub(super) type CharacterFuture =
    Shared<LocalBoxFuture<'static, Result<Rc<CharacterEncoding>, String>>>;

pub(super) struct CharacterEncoding {
    groups: Vec<(CharacterPart, OptimizedEncoding)>,
}

pub(super) struct CharacterUi {
    part: HtmlSelectElement,
    frame: HtmlSelectElement,
    frame_canvas: HtmlCanvasElement,
    composite: HtmlCanvasElement,
    info: HtmlElement,
    identity: HtmlTextAreaElement,
    kind: HtmlSelectElement,
    validation: HtmlElement,
    pub(super) help: HtmlElement,
    pub(super) gear_help: HtmlElement,
}

impl CharacterUi {
    pub(super) fn new(document: &Document) -> Result<Self, JsValue> {
        let ui = Self {
            part: element(document, "character-part")?,
            frame: element(document, "character-frame")?,
            frame_canvas: element(document, "character-frame-canvas")?,
            composite: element(document, "character-composite")?,
            info: element(document, "character-frame-info")?,
            identity: element(document, "character-identity")?,
            kind: element(document, "character-export-kind")?,
            validation: element(document, "character-validation")?,
            help: element(document, "character-export-help")?,
            gear_help: element(document, "gear-export-help")?,
        };
        ui.populate_frames(document)?;
        for (file, label) in [
            ("body", "身体与腿"),
            ("front-arm", "前臂"),
            ("back-arm", "后臂"),
            ("head", "头部与表情"),
            ("hair", "头发"),
        ] {
            let card = document.create_element("article")?;
            card.set_class_name("template-card");
            card.set_inner_html(&format!(
                "<h4>{label}</h4><p class=\"muted\">空白 PNG 用于绘制；纯标签布局与原版人物图像参考用于对照。</p><details class=\"character-reference\"><summary>展开{label}图像参考</summary><p class=\"muted\">可横向滚动查看帧名与别名。参考 SVG 不可直接上传；请在空白 PNG 的对应有效格绘制，不要将网格或标注带入编码。</p><div class=\"character-reference-scroll\" tabindex=\"0\" role=\"region\" aria-label=\"{label}图像参考，可横向滚动\"><img src=\"assets/character/{file}-reference.svg\" alt=\"{label}原版像素图与帧名、别名布局参考\" loading=\"lazy\" decoding=\"async\"></div><p><a href=\"assets/character/{file}-reference.svg\" target=\"_blank\" rel=\"noopener noreferrer\">在新标签页查看原图 ↗</a></p></details><div class=\"actions\"><a class=\"button\" href=\"assets/character/{file}.png\" download=\"character-{file}.png\">下载空白 PNG</a><a class=\"button secondary\" href=\"assets/character/{file}-layout.svg\" download=\"character-{file}-layout.svg\">下载帧布局 SVG</a><a class=\"button secondary\" href=\"assets/character/{file}-reference.svg\" download=\"{file}-reference.svg\">下载图像参考 SVG</a></div>"
            ));
            element::<HtmlElement>(document, "character-templates")?.append_child(&card)?;
        }
        Ok(ui)
    }

    pub(super) fn allows_command_export(&self) -> bool {
        matches!(self.kind.value().as_str(), "patch" | "undo")
    }

    fn part_index(&self) -> usize {
        self.part
            .value()
            .parse::<usize>()
            .unwrap_or(0)
            .min(PARTS.len() - 1)
    }

    fn populate_frames(&self, document: &Document) -> Result<(), JsValue> {
        self.frame.set_inner_html("");
        for (index, frame) in outfit_core::character_layout(PARTS[self.part_index()])
            .frames
            .iter()
            .enumerate()
        {
            let option = document.create_element("option")?;
            option.set_attribute("value", &index.to_string())?;
            let label = if frame.aliases.is_empty() {
                frame.name.clone()
            } else {
                format!("{} (= {})", frame.name, frame.aliases.join(", "))
            };
            option.set_text_content(Some(&label));
            self.frame.append_child(&option)?;
        }
        Ok(())
    }
}

pub(super) fn validate_image(index: usize, image: &RgbaImage) -> Result<(), String> {
    outfit_core::validate_character_layer(CharacterLayer {
        part: PARTS[index - FIRST_SLOT],
        image,
    })
    .map_err(|error| format!("人物 PNG：{error}"))
}

pub(super) fn attach_events(app: &Application) -> Result<(), JsValue> {
    for id in [
        "character-part",
        "character-frame",
        "character-previous",
        "character-next",
    ] {
        let target: EventTarget = app
            .borrow()
            .ui
            .document
            .get_element_by_id(id)
            .ok_or_else(|| JsValue::from_str("缺少人物预览控件。"))?
            .into();
        let app = Rc::clone(app);
        let event = if id.ends_with("previous") || id.ends_with("next") {
            "click"
        } else {
            "change"
        };
        listen(&target, event, move |_| {
            let state = app.borrow();
            let ui = &state.ui.character;
            if id == "character-part" {
                if let Err(error) = ui.populate_frames(&state.ui.document) {
                    state.show_notice(&js_error(error), true);
                    return;
                }
            } else if id == "character-previous" || id == "character-next" {
                let count = outfit_core::character_layout(PARTS[ui.part_index()])
                    .frames
                    .len();
                let index = ui.frame.value().parse::<usize>().unwrap_or(0);
                if count > 0 {
                    let next = if id == "character-next" {
                        (index + 1) % count
                    } else {
                        (index + count - 1) % count
                    };
                    ui.frame.set_value(&next.to_string());
                }
            }
            if let Err(error) = state.draw_character_preview() {
                state.show_notice(&error, true);
            }
        })?;
    }
    for id in ["character-identity", "character-export-kind"] {
        let target: EventTarget = app
            .borrow()
            .ui
            .document
            .get_element_by_id(id)
            .ok_or_else(|| JsValue::from_str("缺少人物导出控件。"))?
            .into();
        let app = Rc::clone(app);
        listen(
            &target,
            if id == "character-identity" {
                "input"
            } else {
                "change"
            },
            move |_| {
                let mut state = app.borrow_mut();
                // Baseline and export selection are metadata, never a pixel cache key.
                state.request.advance();
                state.reset_result();
                state.refresh_actions();
            },
        )?;
    }
    Ok(())
}

impl App {
    pub(super) fn character_ready(&self) -> bool {
        let count = (FIRST_SLOT..FIRST_SLOT + 4)
            .filter(|&index| self.slots[index].upload.is_some())
            .count();
        let hair = self.slots[FIRST_SLOT + 4].upload.is_some();
        let ready = (count == 0 || count == 4) && (count == 4 || hair);
        self.ui
            .character
            .validation
            .set_class_name(if count > 0 && count < 4 {
                "file-status error"
            } else {
                "file-status"
            });
        self.ui
            .character
            .validation
            .set_text_content(Some(if count > 0 && count < 4 {
                "身体 / 前臂 / 后臂 / 头部必须全部上传；不会自动清空缺失部位。"
            } else if ready {
                "像素输入已齐全。所有人物导出均需原 identity；请先备份，再选择导出方式。"
            } else {
                "上传四张完整人物图层，或只上传头发。服装与帽子输入不会被改动。"
            }));
        ready
    }

    pub(super) fn draw_character_preview(&self) -> Result<(), String> {
        let draw = || -> Result<(), JsValue> {
            let ui = &self.ui.character;
            let part_index = ui.part_index();
            let layout = outfit_core::character_layout(PARTS[part_index]);
            let index = ui.frame.value().parse::<usize>().unwrap_or(0);
            let frame = layout
                .frames
                .get(index)
                .ok_or_else(|| JsValue::from_str("人物帧布局为空。"))?;
            let context = canvas_context(&ui.frame_canvas)?;
            context.clear_rect(0.0, 0.0, 43.0, 43.0);
            context.set_image_smoothing_enabled(false);
            if self.slots[FIRST_SLOT + part_index].upload.is_some() {
                draw_frame(
                    &context,
                    &self.slots[FIRST_SLOT + part_index].canvas,
                    frame.x,
                    frame.y,
                )?;
            }
            ui.info.set_text_content(Some(&format!(
                "{} · 格子原点 ({}, {}) · ID {:02x}{}{}",
                frame.name,
                frame.x,
                frame.y,
                frame.id,
                if frame.aliases.is_empty() {
                    ""
                } else {
                    " · 共享别名："
                },
                frame.aliases.join(", ")
            )));
            let context = canvas_context(&ui.composite)?;
            context.clear_rect(0.0, 0.0, 43.0, 43.0);
            context.set_image_smoothing_enabled(false);
            for part_index in [2, 0, 3, 4, 1] {
                if self.slots[FIRST_SLOT + part_index].upload.is_none() {
                    continue;
                }
                let layout = outfit_core::character_layout(PARTS[part_index]);
                let frame = layout
                    .frames
                    .iter()
                    .find(|frame| {
                        frame.name == "idle.1"
                            || frame.name == "neutral.1"
                            || frame.name == "normal"
                    })
                    .or_else(|| layout.frames.first())
                    .ok_or_else(|| JsValue::from_str("人物帧布局为空。"))?;
                draw_frame(
                    &context,
                    &self.slots[FIRST_SLOT + part_index].canvas,
                    frame.x,
                    frame.y,
                )?;
            }
            Ok(())
        };
        draw().map_err(|error| format!("无法绘制人物预览：{}", js_error(error)))
    }
}

fn draw_frame(
    context: &CanvasRenderingContext2d,
    source: &HtmlCanvasElement,
    x: usize,
    y: usize,
) -> Result<(), JsValue> {
    context.draw_image_with_html_canvas_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        source, x as f64, y as f64, 43.0, 43.0, 0.0, 0.0, 43.0, 43.0,
    )
}

fn baseline(state: &App) -> Result<Value, String> {
    let text = state.ui.character.identity.value();
    if text.trim().is_empty() {
        return Err("人物重绘需要原角色 identity JSON，以确定真实图像资源并保留种族、未编辑图层与其他字段。请先备份并粘贴原身份。".into());
    }
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("原 identity JSON 无效：{error}"))?;
    if !value.is_object() {
        return Err("原 identity 必须是 JSON 对象。".into());
    }
    Ok(value)
}

pub(super) fn export(app: &Application, command: bool, copy: bool) {
    let prepared = (|| -> Result<_, String> {
        let mut state = app.borrow_mut();
        let ticket = state.request.advance();
        state.reset_result();
        if !state.character_ready() {
            return Err("请先补齐人物图层，或仅选择头发。".into());
        }
        let baseline = baseline(&state)?;
        let kind = state.ui.character.kind.value();
        if command && matches!(kind.as_str(), "merged" | "backup") {
            return Err(
                "完整 identity 与原身份备份只能导出 JSON；应用命令请选择变化字段或恢复字段。"
                    .into(),
            );
        }
        let mut identities = Vec::new();
        let mut images = Vec::new();
        for (index, part) in PARTS.iter().enumerate() {
            if let Some(upload) = &state.slots[FIRST_SLOT + index].upload {
                identities.push((upload.id, *part));
                images.push((*part, Rc::clone(&upload.image)));
            }
        }
        let key = InputKey {
            source: InputSource::Character { layers: identities },
            palette: state.palette,
            strategy: state.strategy,
        };
        let (future, reuse, generation) = if let Some(found) = state.character_cache.get(&key) {
            found
        } else {
            let palette = key.palette;
            let strategy = key.strategy;
            let future = async move {
                let mut groups = Vec::new();
                for selected in [
                    CharacterPart::Body,
                    CharacterPart::Head,
                    CharacterPart::Hair,
                ] {
                    let layers: Vec<_> = images
                        .iter()
                        .filter(|(part, _)| match selected {
                            CharacterPart::Body => matches!(
                                part,
                                CharacterPart::Body
                                    | CharacterPart::FrontArm
                                    | CharacterPart::BackArm
                            ),
                            _ => *part == selected,
                        })
                        .map(|(part, image)| CharacterLayer {
                            part: *part,
                            image: image.as_ref(),
                        })
                        .collect();
                    if layers.is_empty() {
                        continue;
                    }
                    let candidates = outfit_core::character_candidates(&layers, palette, strategy)
                        .map_err(|error| error.to_string())?;
                    let mut estimates = Vec::with_capacity(candidates.len());
                    for candidate in &candidates {
                        estimates.push(deflate_size(&candidate.directives).await?);
                    }
                    let result = outfit_core::select(candidates, &estimates, strategy)
                        .map_err(|error| error.to_string())?;
                    groups.push((selected, result));
                }
                Ok(Rc::new(CharacterEncoding { groups }))
            }
            .boxed_local()
            .shared();
            let (generation, reuse) = state.character_cache.insert(key, future.clone());
            (future, reuse, generation)
        };
        state.show_notice(
            match reuse {
                Reuse::New => "正在编码人物各组并比较浏览器 Deflate 候选…",
                Reuse::Pending => "正在复用进行中的人物编码…",
                Reuse::Completed => "正在复用已完成的人物编码；重新生成身份补丁…",
            },
            false,
        );
        Ok((ticket, baseline, kind, future, reuse, generation))
    })();
    let (ticket, baseline, kind, future, reuse, generation) = match prepared {
        Ok(value) => value,
        Err(error) => {
            app.borrow().show_notice(&error, true);
            return;
        }
    };
    let app = Rc::clone(app);
    spawn_local(async move {
        let result = future.await;
        let generated = {
            let mut state = app.borrow_mut();
            state.character_cache.finish(generation, result.is_ok());
            if !state.request.accepts(ticket) {
                return;
            }
            match result.and_then(|encoding| {
                let directive = |part| {
                    encoding
                        .groups
                        .iter()
                        .find(|(group, _)| *group == part)
                        .map(|(_, result)| result.encoding.directives.as_str())
                };
                let patch = outfit_core::identity_patch(
                    &baseline,
                    directive(CharacterPart::Body),
                    directive(CharacterPart::Head),
                    directive(CharacterPart::Hair),
                )
                .map_err(|error| error.to_string())?;
                let value = match kind.as_str() {
                    "merged" => outfit_core::apply_identity_patch(&baseline, &patch)
                        .map_err(|error| error.to_string())?,
                    "backup" => baseline.clone(),
                    "undo" => Value::Object(
                        patch
                            .as_object()
                            .ok_or("Identity patch 必须是对象。")?
                            .keys()
                            .map(|key| {
                                (
                                    key.clone(),
                                    baseline.get(key).cloned().unwrap_or(Value::Null),
                                )
                            })
                            .collect(),
                    ),
                    _ => patch,
                };
                let text = if command {
                    outfit_core::identity_command(&value).map_err(|error| error.to_string())?
                } else {
                    serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
                };
                Ok((encoding, text))
            }) {
                Ok((encoding, text)) => {
                    render_result(&state.ui, &encoding, reuse, text.len());
                    state.ui.output.set_value(&text);
                    state.ui.result.set_hidden(false);
                    state.ui.download_last.set_disabled(false);
                    state.ui.copy_last.set_disabled(false);
                    let filename = format!(
                        "character-identity-{kind}.{}",
                        if command { "txt" } else { "json" }
                    );
                    let text = Rc::new(text);
                    state.generated = Some(Generated {
                        text: Rc::clone(&text),
                        filename: filename.clone(),
                        command,
                    });
                    Some((text, filename))
                }
                Err(error) => {
                    state.show_notice(&error, true);
                    None
                }
            }
        };
        if let Some((text, filename)) = generated {
            deliver(&app, ticket, text, filename, command, copy).await;
        }
    });
}

fn render_result(ui: &Ui, result: &CharacterEncoding, reuse: Reuse, bytes: usize) {
    let source = match reuse {
        Reuse::New => "新编码",
        Reuse::Pending => "复用进行中编码",
        Reuse::Completed => "复用已完成编码",
    };
    let mut html = format!(
        "<div><dt>类型</dt><dd>人物 Identity（非装备）</dd></div><div><dt>编码来源</dt><dd>{source}</dd></div><div><dt>完整导出文本</dt><dd>{bytes} B</dd></div>"
    );
    for (part, result) in &result.groups {
        let label = match part {
            CharacterPart::Body => "身体与双臂",
            CharacterPart::Head => "头部与表情",
            CharacterPart::Hair => "头发",
            _ => unreachable!("only complete groups are encoded"),
        };
        html.push_str(&format!("<div><dt>{label}指令</dt><dd>{} B / {} 像素</dd></div><div><dt>{label} Deflate 估算</dt><dd>{} B / {} 候选</dd></div>", result.encoding.output_bytes, result.encoding.pixel_count, result.estimated_deflate_bytes, result.candidate_count));
    }
    ui.stats.set_inner_html(&html);
}
