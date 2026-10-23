use super::*;
use outfit_core::RecolorLayer;

pub(super) type RecolorFuture = Shared<LocalBoxFuture<'static, Result<Rc<String>, String>>>;

pub(super) struct RecolorUi {
    host: HtmlElement,
    pub(super) validation: HtmlElement,
    pub(super) help: HtmlElement,
    pub(super) actions: Vec<HtmlButtonElement>,
}

impl RecolorUi {
    pub(super) fn new(document: &Document) -> Result<Self, JsValue> {
        Ok(Self {
            host: element(document, "recolor-pairs")?,
            validation: element(document, "recolor-validation")?,
            help: element(document, "recolor-export-help")?,
            actions: ["recolor-generate", "recolor-copy", "recolor-download"]
                .iter()
                .map(|id| element(document, id))
                .collect::<Result<_, _>>()?,
        })
    }
}

pub(super) struct Pair {
    id: u64,
    root: HtmlElement,
    slots: [Slot; 2],
    dimensions: HtmlElement,
}

impl App {
    pub(super) fn recolor_ready(&self) -> bool {
        !self.recolor_pairs.is_empty()
            && self.recolor_pairs.iter().all(|pair| {
                match (&pair.slots[0].upload, &pair.slots[1].upload) {
                    (Some(source), Some(target)) => {
                        source.image.width == target.image.width
                            && source.image.height == target.image.height
                    }
                    _ => false,
                }
            })
    }
}

pub(super) fn add_pair(app: &Application) -> Result<(), JsValue> {
    let mut state = app.borrow_mut();
    state.next_pair_id += 1;
    let id = state.next_pair_id;
    let document = &state.ui.document;
    let root = document
        .create_element("section")?
        .dyn_into::<HtmlElement>()?;
    root.set_class_name("card recolor-pair");
    root.set_attribute("aria-labelledby", &format!("recolor-title-{id}"))?;
    root.set_inner_html(&format!(
        "<h3 id=\"recolor-title-{id}\">配对 #{id}</h3><p class=\"muted\">同一配对两图必须等尺寸、逐像素对齐；不同配对可以使用不同尺寸。</p><button type=\"button\" class=\"secondary small\" data-recolor-remove=\"{id}\" aria-label=\"删除配对 {id}\">删除此配对</button><div id=\"recolor-slots-{id}\" class=\"recolor-pair-grid\"></div><p id=\"recolor-dimensions-{id}\" class=\"file-status\" role=\"status\">请载入原图与目标图。</p>"
    ));
    state.ui.recolor.host.append_child(&root)?;
    let mut slots = Vec::with_capacity(2);
    for (side, label) in ["原图（实际原版基底）", "目标图（期望 RGBA）"]
        .iter()
        .enumerate()
    {
        let key = format!("recolor-{id}-{side}");
        let card = document
            .create_element("section")?
            .dyn_into::<HtmlElement>()?;
        card.set_id(&format!("drop-{key}"));
        card.set_class_name("upload-card");
        card.set_attribute("data-recolor-pair", &id.to_string())?;
        card.set_attribute("data-recolor-side", &side.to_string())?;
        card.set_inner_html(&format!(
            "<h4>{label}</h4><label for=\"input-{key}\">选择{label} PNG</label><input id=\"input-{key}\" type=\"file\" accept=\"image/png,.png\" aria-describedby=\"status-{key}\"><button id=\"clear-{key}\" type=\"button\" class=\"secondary small\" data-recolor-clear=\"true\">清除图片</button><p id=\"status-{key}\" class=\"file-status\" role=\"status\" aria-live=\"polite\">尚未选择 PNG；也可拖放文件到此处。</p><div class=\"preview\"><canvas id=\"preview-{key}\" hidden role=\"img\" aria-label=\"配对 {id} {label}预览\"></canvas></div>"
        ));
        element::<HtmlElement>(document, &format!("recolor-slots-{id}"))?.append_child(&card)?;
        slots.push(Slot {
            input: element(document, &format!("input-{key}"))?,
            clear: element(document, &format!("clear-{key}"))?,
            dropzone: card,
            canvas: element(document, &format!("preview-{key}"))?,
            status: element(document, &format!("status-{key}"))?,
            epoch: Epoch::default(),
            upload: None,
        });
    }
    let dimensions = element(document, &format!("recolor-dimensions-{id}"))?;
    let slots = slots
        .try_into()
        .map_err(|_| JsValue::from_str("无法初始化 PNG 配对。"))?;
    state.recolor_pairs.push(Pair {
        id,
        root,
        slots,
        dimensions,
    });
    state.invalidate();
    Ok(())
}

fn event_element(event: &Event) -> Option<web_sys::Element> {
    event.target()?.dyn_into().ok()
}

fn pair_location(target: &web_sys::Element) -> Option<(u64, usize)> {
    let card = target.closest("[data-recolor-pair]").ok()??;
    let id = card.get_attribute("data-recolor-pair")?.parse().ok()?;
    let side = card
        .get_attribute("data-recolor-side")?
        .parse::<usize>()
        .ok()?;
    (side < 2).then_some((id, side))
}

pub(super) fn attach_events(app: &Application) -> Result<(), JsValue> {
    let add: HtmlButtonElement = element(&app.borrow().ui.document, "recolor-add")?;
    let add_app = Rc::clone(app);
    listen(&add.into(), "click", move |_| {
        if let Err(error) = add_pair(&add_app) {
            add_app
                .borrow()
                .show_notice(&format!("无法增加配对：{}", js_error(error)), true);
        }
    })?;
    let host: EventTarget = app.borrow().ui.recolor.host.clone().into();
    // Delegated listeners survive pair edits without leaking per-pair closures.
    let change_app = Rc::clone(app);
    listen(&host, "change", move |event| {
        let Some(target) = event_element(&event) else {
            return;
        };
        let Some((id, side)) = pair_location(&target) else {
            return;
        };
        let Some(input) = target.dyn_ref::<HtmlInputElement>() else {
            return;
        };
        if let Some(file) = input.files().and_then(|files| files.get(0)) {
            upload_pair(&change_app, id, side, file);
        } else {
            clear_pair(&change_app, id, side);
        }
    })?;
    let click_app = Rc::clone(app);
    listen(&host, "click", move |event| {
        let Some(target) = event_element(&event) else {
            return;
        };
        if let Some(id) = target
            .get_attribute("data-recolor-remove")
            .and_then(|id| id.parse::<u64>().ok())
        {
            let mut state = click_app.borrow_mut();
            if let Some(index) = state.recolor_pairs.iter().position(|pair| pair.id == id) {
                state.recolor_pairs.remove(index).root.remove();
                state.invalidate();
            }
        } else if target.has_attribute("data-recolor-clear")
            && let Some((id, side)) = pair_location(&target)
        {
            clear_pair(&click_app, id, side);
        }
    })?;
    for event_name in ["dragover", "dragleave", "drop"] {
        let drop_app = Rc::clone(app);
        listen(&host, event_name, move |event| {
            let Some(target) = event_element(&event) else {
                return;
            };
            let Some((id, side)) = pair_location(&target) else {
                return;
            };
            event.prevent_default();
            if let Some(pair) = drop_app
                .borrow()
                .recolor_pairs
                .iter()
                .find(|pair| pair.id == id)
            {
                pair.slots[side]
                    .dropzone
                    .set_class_name(if event_name == "dragover" {
                        "upload-card drag-over"
                    } else {
                        "upload-card"
                    });
            }
            if event_name == "drop"
                && let Some(file) = event
                    .dyn_ref::<DragEvent>()
                    .and_then(|event| event.data_transfer())
                    .and_then(|transfer| transfer.files())
                    .and_then(|files| files.get(0))
            {
                upload_pair(&drop_app, id, side, file);
            }
        })?;
    }
    for (index, delivery) in [None, Some(true), Some(false)].into_iter().enumerate() {
        let target: EventTarget = app.borrow().ui.recolor.actions[index].clone().into();
        let app = Rc::clone(app);
        listen(&target, "click", move |_| generate(&app, delivery))?;
    }
    Ok(())
}

fn update_dimensions(pair: &Pair) {
    let (message, error) = match (&pair.slots[0].upload, &pair.slots[1].upload) {
        (Some(source), Some(target)) => {
            let source = &source.image;
            let target = &target.image;
            if source.width == target.width && source.height == target.height {
                (
                    format!(
                        "{} × {} · 尺寸一致；生成时联合检查所有配对的颜色映射。",
                        source.width, source.height
                    ),
                    false,
                )
            } else {
                (
                    format!(
                        "尺寸不一致：原图 {} × {}，目标 {} × {}。不自动裁切或缩放，请对齐后重新上传。",
                        source.width, source.height, target.width, target.height
                    ),
                    true,
                )
            }
        }
        _ => (
            "请为此配对载入原图与目标图；每个未完成的配对都会阻止生成。".to_owned(),
            false,
        ),
    };
    pair.dimensions.set_class_name(if error {
        "file-status error"
    } else {
        "file-status"
    });
    pair.dimensions.set_text_content(Some(&message));
}

fn clear_pair(app: &Application, id: u64, side: usize) {
    let mut state = app.borrow_mut();
    let Some(pair) = state.recolor_pairs.iter_mut().find(|pair| pair.id == id) else {
        return;
    };
    let slot = &mut pair.slots[side];
    slot.epoch.advance();
    slot.upload = None;
    slot.input.set_value("");
    slot.canvas.set_hidden(true);
    slot.status.set_class_name("file-status");
    slot.status
        .set_text_content(Some("尚未选择 PNG；也可拖放文件到此处。"));
    update_dimensions(pair);
    state.invalidate();
}

fn upload_pair(app: &Application, id: u64, side: usize, file: File) {
    let ticket = {
        let mut state = app.borrow_mut();
        let Some(pair) = state.recolor_pairs.iter_mut().find(|pair| pair.id == id) else {
            return;
        };
        let slot = &mut pair.slots[side];
        let ticket = slot.epoch.advance();
        slot.upload = None;
        slot.input.set_value("");
        slot.canvas.set_hidden(true);
        slot.status.set_class_name("file-status");
        slot.status
            .set_text_content(Some(&format!("正在本地 Rust 读取 {}…", file.name())));
        update_dimensions(pair);
        state.invalidate();
        ticket
    };
    let app = Rc::clone(app);
    spawn_local(async move {
        let buffer = JsFuture::from(file.array_buffer()).await;
        // Check before copying/decoding; deleted pairs never accept pending reads.
        let current = app
            .borrow()
            .recolor_pairs
            .iter()
            .any(|pair| pair.id == id && pair.slots[side].epoch.accepts(ticket));
        if !current {
            return;
        }
        let decoded = buffer
            .map_err(|error| format!("无法读取 PNG：{}", js_error(error)))
            .and_then(|buffer| {
                outfit_core::decode_png(&Uint8Array::new(&buffer).to_vec())
                    .map_err(|error| format!("无法解码 PNG：{error}"))
            });
        let mut state = app.borrow_mut();
        let Some(index) = state
            .recolor_pairs
            .iter()
            .position(|pair| pair.id == id && pair.slots[side].epoch.accepts(ticket))
        else {
            return;
        };
        let result = decoded.and_then(|image| {
            draw_preview(&state.recolor_pairs[index].slots[side].canvas, &image)?;
            Ok(image)
        });
        match result {
            Ok(image) => {
                state.next_image_id += 1;
                let image_id = state.next_image_id;
                let slot = &mut state.recolor_pairs[index].slots[side];
                slot.status.set_class_name("file-status");
                slot.status.set_text_content(Some(&format!(
                    "{} · {} × {} · 本地 Rust 已解码（含隐藏 RGB）",
                    file.name(),
                    image.width,
                    image.height
                )));
                slot.upload = Some(Upload {
                    id: image_id,
                    image: Rc::new(image),
                });
            }
            Err(error) => {
                let slot = &state.recolor_pairs[index].slots[side];
                slot.status.set_class_name("file-status error");
                slot.status.set_text_content(Some(&error));
            }
        }
        update_dimensions(&state.recolor_pairs[index]);
        state.invalidate();
    });
}

pub(super) fn generate(app: &Application, delivery: Option<bool>) {
    let (ticket, future, reuse, generation, pair_count) = {
        let mut state = app.borrow_mut();
        if state.mode != EditorMode::Recolor {
            return;
        }
        let ticket = state.request.advance();
        state.reset_result();
        if !state.recolor_ready() {
            state.show_notice(
                "请完成至少一组原图 / 目标图配对，并修正尺寸不一致的配对。",
                true,
            );
            return;
        }
        let mut identities = Vec::with_capacity(state.recolor_pairs.len());
        let mut images = Vec::with_capacity(state.recolor_pairs.len());
        for pair in &state.recolor_pairs {
            let source = pair.slots[0]
                .upload
                .as_ref()
                .expect("ready pair has source");
            let target = pair.slots[1]
                .upload
                .as_ref()
                .expect("ready pair has target");
            identities.push((source.id, target.id));
            images.push((Rc::clone(&source.image), Rc::clone(&target.image)));
        }
        let pair_count = images.len();
        let key = InputKey {
            source: InputSource::Recolor { pairs: identities },
            palette: false,
            strategy: Strategy::Text,
        };
        let (future, reuse, generation) = if let Some(found) = state.recolor_cache.get(&key) {
            found
        } else {
            let future = async move {
                let layers: Vec<_> = images
                    .iter()
                    .map(|(source, target)| RecolorLayer { source, target })
                    .collect();
                outfit_core::recolor_directives(&layers)
                    .map(Rc::new)
                    .map_err(|error| error.to_string())
            }
            .boxed_local()
            .shared();
            let (generation, reuse) = state.recolor_cache.insert(key, future.clone());
            (future, reuse, generation)
        };
        state.show_notice("正在联合检查全部配对的完整 RGBA 映射…", false);
        (ticket, future, reuse, generation, pair_count)
    };
    let app = Rc::clone(app);
    spawn_local(async move {
        let result = future.await;
        let text = {
            let mut state = app.borrow_mut();
            state.recolor_cache.finish(generation, result.is_ok());
            if !state.request.accepts(ticket) {
                return;
            }
            match result {
                Ok(text) => {
                    let status = if text.is_empty() {
                        "全部 RGBA 已一致，无需颜色指令；有效结果为空字符串，可复制或下载零字节文件。"
                    } else {
                        "联合颜色约束通过。仅覆盖所提供的有效帧 / 图层；请按下方适用说明自行应用 directives。"
                    };
                    state.ui.recolor.validation.set_text_content(Some(status));
                    let reused = match reuse {
                        Reuse::New => "新生成",
                        Reuse::Pending => "复用进行中结果",
                        Reuse::Completed => "复用已完成结果",
                    };
                    state.ui.stats.set_inner_html(&format!("<div><dt>类型</dt><dd>原版直接改色（纯 directives）</dd></div><div><dt>联合配对</dt><dd>{pair_count}</dd></div><div><dt>完整指令</dt><dd>{} B</dd></div><div><dt>生成来源</dt><dd>{reused}</dd></div><div><dt>状态</dt><dd>{}</dd></div>", text.len(), if text.is_empty() { "无需颜色指令（空字符串）" } else { "完整 RGBA 映射通过" }));
                    state.ui.output.set_value(&text);
                    state.ui.result.set_hidden(false);
                    state.ui.copy_last.set_disabled(false);
                    state.ui.download_last.set_disabled(false);
                    state.generated = Some(Generated {
                        text: Rc::clone(&text),
                        filename: "vanilla-recolor-directives.txt".to_owned(),
                        command: true,
                    });
                    state.show_notice(status, false);
                    Some(text)
                }
                Err(error) => {
                    let order = state
                        .recolor_pairs
                        .iter()
                        .enumerate()
                        .map(|(index, pair)| format!("层 {} = 配对 #{}", index + 1, pair.id))
                        .collect::<Vec<_>>()
                        .join("；");
                    let message = format!(
                        "拒绝生成：{error}\n{order}。坐标以 PNG 左上为 (0,0)。同一源 RGBA 必须在所有位置、所有配对中对应同一目标 RGBA（包括不变像素及 alpha=0 隐藏 RGB）；不会转用通用自绘编码。请统一目标色或选择原有重绘模式。"
                    );
                    state.ui.recolor.validation.set_text_content(Some(&message));
                    state.show_notice(&message, true);
                    None
                }
            }
        };
        if let (Some(text), Some(copy)) = (text, delivery) {
            deliver(
                &app,
                ticket,
                text,
                "vanilla-recolor-directives.txt".to_owned(),
                true,
                copy,
            )
            .await;
        }
    });
}
