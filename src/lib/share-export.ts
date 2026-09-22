// 分享卡 PNG 导出：DOM → SVG <foreignObject> → Image → canvas → PNG，零依赖。
//
// 三条从 token-meter 插件踩过的坑（别改回去）：
//  1. 必须强制 `animation:none`——带入场动画（opacity 0 起步）的节点在 <img> 里
//     不会推进 CSS 动画，会以全透明被光栅化，导出图只剩页头；
//  2. 量尺寸前要等两帧，否则刚挂载的 DOM 宽度还是 0；
//  3. 内联 CSS 必须 XML 转义后再进 <style>——压缩后的 CSS 可能带裸 `<`（容器查询
//     范围语法 `(width<=300px)`），一个裸 `<` 就让整份 SVG 解析失败。

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
  return css;
}

/** 等两帧让布局稳定后再量（插件同款做法） */
function nextTwoFrames(): Promise<void> {
  return new Promise((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
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
  const img = new Image();
  await new Promise<void>((resolve, reject) => {
    img.addEventListener("load", () => resolve(), { once: true });
    img.addEventListener("error", () => reject(new Error(`图片光栅化失败（SVG ${svg.length} 字符 / ${pw}×${ph}）`)), { once: true });
    img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
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
  return await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob((b) => (b === null ? reject(new Error("PNG 编码失败")) : resolve(b)), "image/png"),
  );
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
