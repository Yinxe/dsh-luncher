// 分享卡 PNG 导出：DOM → SVG <foreignObject> → Image → canvas → PNG，零依赖。
//
// 三条从 token-meter 插件踩过的坑（别改回去）：
//  1. 必须强制 `animation:none`——带入场动画（opacity 0 起步）的节点在 <img> 里
//     不会推进 CSS 动画，会以全透明被光栅化，导出图只剩页头；
//  2. 量尺寸前要等两帧，否则刚挂载的 DOM 宽度还是 0；
//  3. 内联 CSS 必须 XML 转义后再进 <style>——压缩后的 CSS 可能带裸 `<`（容器查询
//     范围语法 `(width<=300px)`），一个裸 `<` 就让整份 SVG 解析失败。
//  4. 内联 CSS 必须剥掉外部 `url()`（字体等）——见 stripExternalUrls；
//  5. `blob:` URL 在打包二进制里会污染画布（tauri:// 自定义协议下 WebKitGTK 把
//     blob: 图判为不透明源，toBlob 抛 SecurityError），必须留 base64 data: 退路。

/**
 * 剥掉 CSS 里的外部 `url()` 引用（@font-face 字体、外链图）。
 *
 * WebKitGTK 一旦发现作为 `<img>` 加载的 SVG 里含外部资源引用，就把整张图判为
 * **跨源**；`drawImage` 进 canvas 后画布被污染，随后 `canvas.toBlob()` 直接抛
 * `SecurityError: The operation is insecure`，导出/复制全挂（Linux 实测）。
 * 而字体在 `<img>` 承载的 SVG 里本就不会加载（导出图文字一直走系统回退字体），
 * 删掉这些 url() 零视觉损失，却让画布保持同源。`data:` URI 同源、不污染，保留。
 */
function stripExternalUrls(css: string): string {
  return css
    .replace(/@font-face\s*\{[^}]*\}/gi, "")
    .replace(/url\(\s*(['"]?)(?!data:)[^\s'"]*\1\s*\)/gi, "none");
}

/** XML 文本转义；`&` 必须最先换，否则会把刚生成的 `&lt;` 二次转义 */
function xmlEscapeText(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/**
 * 收集页面全部样式 + 当前主题的 CSS 变量计算值。
 * 主题类挂在 <html class="dark"> 上，foreignObject 里没有这个祖先，
 * 所以还要把 CSS 里引用到的 var(--x) 按**当前计算值**补一条 :root 规则。
 */
function collectCss(): string {
  let css = "*,:before,:after{animation:none!important;transition:none!important}\n";
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      for (const rule of Array.from(sheet.cssRules ?? [])) css += rule.cssText + "\n";
    } catch {
      // 跨域样式表读不到 cssRules：最多掉那份样式，不该让导出失败
    }
  }
  try {
    const used = new Set<string>();
    for (const m of css.matchAll(/var\(\s*(--[a-zA-Z0-9-]+)/g)) used.add(m[1]);
    const cs = getComputedStyle(document.documentElement);
    const decls: string[] = [];
    for (const name of used) {
      const v = cs.getPropertyValue(name).trim();
      if (v) decls.push(`${name}:${v}`);
    }
    if (decls.length > 0) css += `:root{${decls.join(";")}}\n`;
  } catch {
    // 取不到计算值：变量走各自 fallback
  }
  return stripExternalUrls(css);
}

/** 等两帧让布局稳定后再量（插件同款做法） */
function nextTwoFrames(): Promise<void> {
  return new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
}

/** Blob → base64 data URL（base64 不像 encodeURIComponent 那样让中文按 9 字符/字膨胀） */
function blobToDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => resolve(String(r.result));
    r.onerror = () => reject(new Error("SVG 编码为 data URL 失败"));
    r.readAsDataURL(blob);
  });
}

/** 加载 svgUrl 为图片 → 2× 画布 → PNG Blob */
async function rasterize(svgUrl: string, w: number, h: number, pw: number, ph: number, scale: number): Promise<Blob> {
  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.addEventListener("load", () => resolve(), { once: true });
    img.addEventListener("error", () => reject(new Error(`图片光栅化失败（SVG ${w}×${h}）`)), { once: true });
    img.src = svgUrl;
  });
  const canvas = document.createElement("canvas");
  canvas.width = pw;
  canvas.height = ph;
  const ctx = canvas.getContext("2d");
  if (ctx === null) throw new Error("canvas 不可用");
  // 先铺页面底色：foreignObject 边缘抗锯齿会透出透明像素
  ctx.fillStyle = getComputedStyle(document.body).backgroundColor || "#ffffff";
  ctx.fillRect(0, 0, pw, ph);
  ctx.scale(scale, scale);
  ctx.drawImage(img, 0, 0, w, h);
  return await new Promise<Blob>((resolve, reject) => {
    // 画布被污染时 some 引擎不抛异常而是 fire canvas error 事件，回调永远不来
    canvas.addEventListener("error", () => reject(new Error("SecurityError: 画布被污染，无法导出 PNG")), { once: true });
    canvas.toBlob((b) => (b === null ? reject(new Error("PNG 编码失败")) : resolve(b)), "image/png");
  });
}

/** blob: 在自定义协议下可能表现为 toBlob 抛 SecurityError，也可能表现为 img 静默加载失败 */
function isTaintError(e: unknown): boolean {
  return /insecure|SecurityError|taint|光栅化失败/i.test(String(e));
}

/** 把 DOM 节点按 2× 光栅化成 PNG Blob（比例跟随节点本身） */
export async function nodeToPngBlob(node: HTMLElement, scale = 2): Promise<Blob> {
  await nextTwoFrames();
  const w = node.offsetWidth;
  const h = node.offsetHeight;
  if (w === 0 || h === 0) throw new Error("分享面板尚未渲染完成，请重试");
  const pw = Math.max(1, Math.round(w * scale));
  const ph = Math.max(1, Math.round(h * scale));
  const clone = node.cloneNode(true) as HTMLElement;
  clone.style.transform = "none";
  clone.style.position = "static";
  const inner = new XMLSerializer().serializeToString(clone);
  // **SVG 的 width/height 必须是逻辑尺寸**（不是 2× 画布尺寸）：WebKitGTK 在把
  // 大尺寸 SVG 图像光栅化进 canvas 时会**静默丢掉**一部分 flex 布局节点
  // （实测：内联样式只有百分比高度/空 div 的柱状图与热力图格子在 2560 宽的
  // SVG 里整体消失，1280 宽则正常）。放大改在 canvas 侧做：ctx.scale(2) 后按
  // 逻辑尺寸 drawImage，矢量内容仍按 2× 重新排版，文字照样清晰。
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">` +
    `<foreignObject x="0" y="0" width="${w}" height="${h}">` +
    `<div xmlns="http://www.w3.org/1999/xhtml"><style>${xmlEscapeText(collectCss())}</style>${inner}</div>` +
    `</foreignObject></svg>`;
  const svgBlob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
  // 首选 blob:（几 MB 的 SVG  encodeURIComponent 后会撞 WebKit data URI 长度上限）。
  // 但打包二进制里页面源是 tauri://localhost 自定义协议，WebKitGTK 会把 blob: 图
  // 判为不透明源 → 画布污染 → toBlob 抛 SecurityError；此时退回 base64 data URL
  // （data: 在所有引擎都视为同源，base64 只膨胀 4/3，不会重演长度问题）。
  const svgUrl = URL.createObjectURL(svgBlob);
  try {
    return await rasterize(svgUrl, w, h, pw, ph, scale);
  } catch (e) {
    if (!isTaintError(e)) throw e;
    return await rasterize(await blobToDataUrl(svgBlob), w, h, pw, ph, scale);
  } finally {
    URL.revokeObjectURL(svgUrl);
  }
}

/** 触发一次浏览器下载 */
export function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 4000);
}

/** 复制到剪贴板（图片）；浏览器不支持或无焦点时返回 false，由调用方降级提示 */
export async function copyPng(blob: Blob): Promise<boolean> {
  try {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
    return true;
  } catch {
    return false;
  }
}
