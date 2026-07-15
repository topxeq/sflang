//! builtins_image.rs — 图像处理与画布绘图
//!
//! 设计要点：
//!   - 基于 image crate 实现图片加载/保存/变换（支持 PNG/JPEG/GIF/BMP/WebP）
//!   - 基于 rusttype 实现 TrueType 字体文字渲染
//!   - Image 类型封装 DynamicImage，用于图片操作
//!   - Canvas 类型封装 RgbaImage，用于画布绘图
//!   - Font 类型封装 rusttype::Font，用于文字渲染
//!   - 颜色用 [r, g, b, a] 数组表示（每个 0-255，a 默认 255）
//!   - 几何绘图算法纯标准库实现（Bresenham 画线/中点画圆）
//!
//! 函数列表：
//!   图片加载/保存：
//!     imageLoad(path) → image
//!     imageLoadFromBytes(bytes, format?) → image
//!     imageSave(img, path, format?)
//!     imageSaveToBytes(img, format) → bytes
//!
//!   图片基本操作：
//!     imageNew(width, height, bgColor?) → image
//!     imageGetWidth(img) → int
//!     imageGetHeight(img) → int
//!     imageGetPixel(img, x, y) → [r,g,b,a]
//!     imageSetPixel(img, x, y, color)
//!     imageFill(img, color)
//!
//!   图片变换：
//!     imageResize(img, width, height, filter?) → image
//!     imageCrop(img, x, y, width, height) → image
//!     imageRotate(img, degrees) → image  (仅支持 90/180/270)
//!     imageFlipH(img) → image
//!     imageFlipV(img) → image
//!
//!   颜色滤镜：
//!     imageGray(img) → image
//!     imageInvert(img) → image
//!     imageBrightness(img, factor) → image
//!     imageContrast(img, factor) → image
//!
//!   Canvas 画布：
//!     canvasNew(width, height, bgColor?) → canvas
//!     canvasFromImage(img) → canvas
//!     canvasToImage(canvas) → image
//!     canvasGetWidth(canvas) → int
//!     canvasGetHeight(canvas) → int
//!     canvasGetPixel(canvas, x, y) → [r,g,b,a]
//!     canvasSetPixel(canvas, x, y, color)
//!     canvasFill(canvas, color)
//!
//!   Canvas 绘图：
//!     canvasDrawLine(canvas, x1, y1, x2, y2, color)
//!     canvasDrawRect(canvas, x, y, width, height, color)
//!     canvasFillRect(canvas, x, y, width, height, color)
//!     canvasDrawCircle(canvas, cx, cy, radius, color)
//!     canvasFillCircle(canvas, cx, cy, radius, color)
//!     canvasDrawText(canvas, x, y, text, color, fontSize?, font?)
//!     canvasDrawImage(canvas, img, x, y)
//!
//!   颜色与字体：
//!     colorNew(r, g, b, a?) → [r,g,b,a]
//!     colorFromHex(hex) → [r,g,b,a]
//!     colorToHex(color) → string
//!     loadFont(fontPath) → font

use std::sync::{Arc, Mutex};

use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba, RgbaImage};
use rusttype::{Font, Point, Scale};

use crate::builtins_helpers as bh;
use crate::value::{error_value, Value};
use crate::vm::VM;
use crate::function::BuiltinDoc;

// ===================== 图片加载/保存 =====================

static DOC_IMAGE_LOAD: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageLoad(path) -> image",
    summary: "从文件加载图片（自动识别格式）。",
    params: &[("path", "图片文件路径（支持 png/jpg/gif/bmp/webp）")],
    returns: "image 图片对象",
    examples: &["img := imageLoad(\"photo.png\")"],
    errors: &["文件不存在或不是支持的图片格式"],
};

static DOC_IMAGE_LOAD_FROM_BYTES: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageLoadFromBytes(bytes, format?) -> image",
    summary: "从字节数据加载图片。",
    params: &[
        ("bytes", "图片字节数据（bytes/byteArray）"),
        ("format", "可选格式 \"png\"/\"jpg\"/\"gif\"/\"bmp\"/\"webp\"，缺省自动识别"),
    ],
    returns: "image 图片对象",
    examples: &[
        "var data = readFileBin(\"a.png\")",
        "img := imageLoadFromBytes(data)",
        "img := imageLoadFromBytes(data, \"png\")  // 指定格式更快",
    ],
    errors: &["数据损坏、格式不匹配或不支持"],
};

static DOC_IMAGE_SAVE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageSave(img, path, format) -> undefined",
    summary: "保存图片到文件。",
    params: &[
        ("img", "image 图片对象"),
        ("path", "输出文件路径"),
        ("format", "格式 \"png\"/\"jpg\"/\"gif\"/\"bmp\"/\"webp\""),
    ],
    returns: "undefined",
    examples: &["imageSave(img, \"out.png\", \"png\")"],
    errors: &["img 不是 image 对象", "路径不可写或磁盘空间不足", "格式拼写错误"],
};

static DOC_IMAGE_SAVE_TO_BYTES: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageSaveToBytes(img, format) -> bytes",
    summary: "将图片编码为字节。",
    params: &[
        ("img", "image 图片对象"),
        ("format", "格式 \"png\"/\"jpg\"/\"gif\"/\"bmp\"/\"webp\""),
    ],
    returns: "bytes 编码后的图片字节",
    examples: &[
        "var b = imageSaveToBytes(img, \"png\")",
        "writeFileBin(\"out.png\", b)",
    ],
    errors: &["img 不是 image 对象", "图片数据无效", "格式拼写错误"],
};

// ===================== 图片基本操作 =====================

static DOC_IMAGE_NEW: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageNew(width, height, bgColor?) -> image",
    summary: "创建新图片（纯色填充）。",
    params: &[
        ("width", "宽度（像素，>0）"),
        ("height", "高度（像素，>0）"),
        ("bgColor", "可选背景色 [r,g,b,a]，缺省透明 [0,0,0,0]"),
    ],
    returns: "image 新建图片对象",
    examples: &[
        "img := imageNew(100, 100)  // 透明",
        "img := imageNew(100, 100, [255,0,0,255])  // 红色背景",
    ],
    errors: &["width/height 不能为 0"],
};

static DOC_IMAGE_GET_WIDTH: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageGetWidth(img) -> int",
    summary: "获取图片宽度。",
    params: &[("img", "image 图片对象")],
    returns: "int 像素宽度",
    examples: &["var w = imageGetWidth(img)"],
    errors: &["img 不是 image 对象（应先用 imageLoad/imageNew 创建）"],
};

static DOC_IMAGE_GET_HEIGHT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageGetHeight(img) -> int",
    summary: "获取图片高度。",
    params: &[("img", "image 图片对象")],
    returns: "int 像素高度",
    examples: &["var h = imageGetHeight(img)"],
    errors: &["img 不是 image 对象（应先用 imageLoad/imageNew 创建）"],
};

static DOC_IMAGE_GET_PIXEL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageGetPixel(img, x, y) -> [r,g,b,a]",
    summary: "获取指定坐标的像素颜色。",
    params: &[
        ("img", "image 图片对象"),
        ("x", "X 坐标（从 0 开始）"),
        ("y", "Y 坐标（从 0 开始）"),
    ],
    returns: "[r,g,b,a] 颜色数组（每分量 0-255）",
    examples: &["var px = imageGetPixel(img, 10, 20)  // 如 [255,0,0,255]"],
    errors: &["img 不是 image 对象", "坐标超出图片范围"],
};

static DOC_IMAGE_SET_PIXEL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageSetPixel(img, x, y, color) -> undefined",
    summary: "设置指定坐标的像素颜色。",
    params: &[
        ("img", "image 图片对象"),
        ("x", "X 坐标（从 0 开始）"),
        ("y", "Y 坐标（从 0 开始）"),
        ("color", "颜色 [r,g,b] 或 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["imageSetPixel(img, 10, 20, [255,255,255,255])"],
    errors: &["img 不是 image 对象", "坐标超出图片范围", "color 不是数组或长度不足 3"],
};

static DOC_IMAGE_FILL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageFill(img, color) -> undefined",
    summary: "用指定颜色填充整个图片。",
    params: &[
        ("img", "image 图片对象"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["imageFill(img, [0,0,0,255])"],
    errors: &["img 不是 image 对象", "color 格式错误"],
};

static DOC_IMAGE_CLONE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageClone(img) -> image",
    summary: "深拷贝图片（返回独立副本）。",
    params: &[("img", "image 图片对象")],
    returns: "image 与原图互不影响的新图片",
    examples: &["var copy = imageClone(img)"],
    errors: &["img 不是 image 对象"],
};

// ===================== 图片变换 =====================

static DOC_IMAGE_RESIZE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageResize(img, width, height, filter?) -> image",
    summary: "缩放图片。",
    params: &[
        ("img", "image 图片对象"),
        ("width", "目标宽度（>0）"),
        ("height", "目标高度（>0）"),
        ("filter", "可选采样滤镜 \"nearest\"/\"triangle\"/\"catmullrom\"/\"gaussian\"/\"lanczos3\"，默认 lanczos3"),
    ],
    returns: "image 缩放后的新图片",
    examples: &[
        "small := imageResize(img, 50, 50)",
        "small := imageResize(img, 50, 50, \"nearest\")  // 像素风",
    ],
    errors: &["img 不是 image 对象", "目标尺寸不能为 0"],
};

static DOC_IMAGE_CROP: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageCrop(img, x, y, width, height) -> image",
    summary: "裁剪图片。",
    params: &[
        ("img", "image 图片对象"),
        ("x", "裁剪区左上角 X"),
        ("y", "裁剪区左上角 Y"),
        ("width", "裁剪区宽度"),
        ("height", "裁剪区高度"),
    ],
    returns: "image 裁剪后的新图片",
    examples: &["sub := imageCrop(img, 10, 10, 100, 100)"],
    errors: &["img 不是 image 对象", "裁剪区域超出图片范围"],
};

static DOC_IMAGE_ROTATE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageRotate(img, degrees) -> image",
    summary: "旋转图片（仅支持 90/180/270 度，正值顺时针）。",
    params: &[
        ("img", "image 图片对象"),
        ("degrees", "角度 90/180/270（正值顺时针，负值逆时针）"),
    ],
    returns: "image 旋转后的新图片",
    examples: &["rotated := imageRotate(img, 90)"],
    errors: &["img 不是 image 对象", "仅支持 90/180/270 度，任意角度请用 imageRotateFree"],
};

static DOC_IMAGE_ROTATE_FREE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageRotateFree(img, degrees, bgColor?) -> image",
    summary: "任意角度旋转图片（正值逆时针）。",
    params: &[
        ("img", "image 图片对象"),
        ("degrees", "任意角度（float，正值逆时针）"),
        ("bgColor", "可选空白区域填充色，默认透明 [0,0,0,0]"),
    ],
    returns: "image 旋转后的新图片（画布自动扩大以容纳全部内容）",
    examples: &[
        "rotated := imageRotateFree(img, 45)",
        "rotated := imageRotateFree(img, 30, [255,255,255,255])  // 白底",
    ],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_FLIP_H: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageFlipH(img) -> image",
    summary: "水平翻转图片（左右镜像）。",
    params: &[("img", "image 图片对象")],
    returns: "image 翻转后的新图片",
    examples: &["mirrored := imageFlipH(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_FLIP_V: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageFlipV(img) -> image",
    summary: "垂直翻转图片（上下镜像）。",
    params: &[("img", "image 图片对象")],
    returns: "image 翻转后的新图片",
    examples: &["mirrored := imageFlipV(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_BLEND: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageBlend(baseImg, overlayImg, x, y) -> image",
    summary: "将叠加图绘制到基础图上（alpha 混合）。",
    params: &[
        ("baseImg", "基础 image 对象"),
        ("overlayImg", "叠加 image 对象"),
        ("x", "叠加左上角 X"),
        ("y", "叠加左上角 Y"),
    ],
    returns: "image 混合后的新图片（半透明像素自动 alpha 混合）",
    examples: &["result := imageBlend(bg, logo, 10, 10)"],
    errors: &["参数不是 image 对象"],
};

// ===================== 颜色滤镜 =====================

static DOC_IMAGE_GRAY: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageGray(img) -> image",
    summary: "灰度化图片。",
    params: &[("img", "image 图片对象")],
    returns: "image 灰度图片",
    examples: &["gray := imageGray(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_INVERT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageInvert(img) -> undefined",
    summary: "反色图片（就地修改）。",
    params: &[("img", "image 图片对象")],
    returns: "undefined（就地修改 img）",
    examples: &["imageInvert(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_BRIGHTNESS: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageBrightness(img, factor) -> image",
    summary: "调整亮度。",
    params: &[
        ("img", "image 图片对象"),
        ("factor", "亮度增量（-255 到 255，正值变亮，负值变暗）"),
    ],
    returns: "image 调整后的新图片",
    examples: &[
        "bright := imageBrightness(img, 50)",
        "dark := imageBrightness(img, -50)",
    ],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_CONTRAST: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageContrast(img, factor) -> image",
    summary: "调整对比度。",
    params: &[
        ("img", "image 图片对象"),
        ("factor", "对比度系数（float，正值增加对比度，负值降低）"),
    ],
    returns: "image 调整后的新图片",
    examples: &["result := imageContrast(img, 1.5)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_BLUR: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageBlur(img, sigma) -> image",
    summary: "高斯模糊。",
    params: &[
        ("img", "image 图片对象"),
        ("sigma", "模糊半径，越大越模糊（通常 1.0-10.0，>=0）"),
    ],
    returns: "image 模糊后的新图片",
    examples: &["blurry := imageBlur(img, 3.0)"],
    errors: &["img 不是 image 对象", "sigma 不能为负数"],
};

static DOC_IMAGE_SHARPEN: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageSharpen(img) -> image",
    summary: "锐化图片（拉普拉斯卷积核）。",
    params: &[("img", "image 图片对象")],
    returns: "image 锐化后的新图片",
    examples: &["sharp := imageSharpen(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_GAMMA: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageGamma(img, gamma) -> image",
    summary: "伽马校正。",
    params: &[
        ("img", "image 图片对象"),
        ("gamma", "伽马值（>0，>1 变亮，<1 变暗，1 不变）"),
    ],
    returns: "image 校正后的新图片",
    examples: &[
        "bright := imageGamma(img, 2.0)",
        "dark := imageGamma(img, 0.5)",
    ],
    errors: &["img 不是 image 对象", "gamma 必须 > 0"],
};

static DOC_IMAGE_SEPIA: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageSepia(img) -> image",
    summary: "棕褐色滤镜（复古效果）。",
    params: &[("img", "image 图片对象")],
    returns: "image 棕褐色复古风格的新图片",
    examples: &["vintage := imageSepia(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_THRESHOLD: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageThreshold(img, threshold) -> image",
    summary: "二值化阈值（灰度 > threshold 变白，否则变黑）。",
    params: &[
        ("img", "image 图片对象"),
        ("threshold", "阈值 0-255"),
    ],
    returns: "image 黑白二值图片",
    examples: &["bw := imageThreshold(img, 128)"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_TINT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageTint(img, color) -> image",
    summary: "用指定颜色着色（保留亮度，替换色调）。",
    params: &[
        ("img", "image 图片对象"),
        ("color", "着色目标颜色 [r,g,b,a]"),
    ],
    returns: "image 着色后的新图片",
    examples: &["tinted := imageTint(img, [255,0,0,255])  // 红色调"],
    errors: &["img 不是 image 对象", "color 格式错误"],
};

static DOC_IMAGE_OPACITY: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageOpacity(img, factor) -> image",
    summary: "调整图片透明度。",
    params: &[
        ("img", "image 图片对象"),
        ("factor", "透明度系数（0.0 全透明，1.0 不变，>1 增强，按 0-255 截断）"),
    ],
    returns: "image 调整 alpha 后的新图片",
    examples: &["semi := imageOpacity(img, 0.5)  // 半透明"],
    errors: &["img 不是 image 对象"],
};

static DOC_IMAGE_EDGE_DETECT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageEdgeDetect(img) -> image",
    summary: "边缘检测（Sobel 算子）。",
    params: &[("img", "image 图片对象")],
    returns: "image 边缘高亮的灰度图片",
    examples: &["edges := imageEdgeDetect(img)"],
    errors: &["img 不是 image 对象", "图片小于 3x3 时返回灰度原图"],
};

static DOC_IMAGE_CONVOLVE3X3: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageConvolve3x3(img, kernel) -> image",
    summary: "自定义 3x3 卷积核滤镜。",
    params: &[
        ("img", "image 图片对象"),
        ("kernel", "9 个数字的数组 [k0..k8]，对应 3x3 矩阵，自动归一化"),
    ],
    returns: "image 卷积后的新图片",
    examples: &[
        "// 模糊核",
        "blurry := imageConvolve3x3(img, [1,1,1, 1,1,1, 1,1,1])",
    ],
    errors: &["img 不是 image 对象", "kernel 必须为恰好 9 元素的数字数组"],
};

// ===================== 图片信息 =====================

static DOC_IMAGE_HISTOGRAM: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "imageHistogram(img) -> [rHist, gHist, bHist, aHist]",
    summary: "获取图片直方图（RGBA 四通道）。",
    params: &[("img", "image 图片对象")],
    returns: "4 个数组的数组，每个是 256 元素的该亮度像素计数",
    examples: &[
        "var h = imageHistogram(img)",
        "var redHist = h[0]  // redHist[i] = 红色分量为 i 的像素数",
    ],
    errors: &["img 不是 image 对象"],
};

// ===================== Canvas 画布 =====================

static DOC_CANVAS_NEW: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasNew(width, height, bgColor?) -> canvas",
    summary: "创建新画布（纯色填充）。",
    params: &[
        ("width", "宽度（像素，>0）"),
        ("height", "高度（像素，>0）"),
        ("bgColor", "可选背景色 [r,g,b,a]，缺省透明"),
    ],
    returns: "canvas 画布对象（用于绘图）",
    examples: &[
        "c := canvasNew(200, 200, [0,0,0,255])",
    ],
    errors: &["width/height 不能为 0"],
};

static DOC_CANVAS_FROM_IMAGE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFromImage(img) -> canvas",
    summary: "从图片创建画布（拷贝像素数据以便绘图）。",
    params: &[("img", "image 图片对象")],
    returns: "canvas 与原图相同像素的画布",
    examples: &["c := canvasFromImage(img)"],
    errors: &["img 不是 image 对象"],
};

static DOC_CANVAS_TO_IMAGE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasToImage(canvas) -> image",
    summary: "画布转图片（用于保存/滤镜）。",
    params: &[("canvas", "canvas 画布对象")],
    returns: "image 与画布相同像素的图片",
    examples: &["img := canvasToImage(c)"],
    errors: &["canvas 不是画布对象"],
};

static DOC_CANVAS_GET_WIDTH: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasGetWidth(canvas) -> int",
    summary: "获取画布宽度。",
    params: &[("canvas", "canvas 画布对象")],
    returns: "int 像素宽度",
    examples: &["var w = canvasGetWidth(c)"],
    errors: &["canvas 不是画布对象"],
};

static DOC_CANVAS_GET_HEIGHT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasGetHeight(canvas) -> int",
    summary: "获取画布高度。",
    params: &[("canvas", "canvas 画布对象")],
    returns: "int 像素高度",
    examples: &["var h = canvasGetHeight(c)"],
    errors: &["canvas 不是画布对象"],
};

static DOC_CANVAS_GET_PIXEL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasGetPixel(canvas, x, y) -> [r,g,b,a]",
    summary: "获取画布指定坐标的像素颜色。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x", "X 坐标（从 0 开始）"),
        ("y", "Y 坐标（从 0 开始）"),
    ],
    returns: "[r,g,b,a] 颜色数组",
    examples: &["var px = canvasGetPixel(c, 5, 5)"],
    errors: &["canvas 不是画布对象", "坐标超出画布范围"],
};

static DOC_CANVAS_SET_PIXEL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasSetPixel(canvas, x, y, color) -> undefined",
    summary: "设置画布指定坐标的像素颜色。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x", "X 坐标（从 0 开始）"),
        ("y", "Y 坐标（从 0 开始）"),
        ("color", "颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasSetPixel(c, 5, 5, [255,0,0,255])"],
    errors: &["canvas 不是画布对象", "坐标超出画布范围", "color 格式错误"],
};

static DOC_CANVAS_FILL: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFill(canvas, color) -> undefined",
    summary: "用指定颜色填充整个画布。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFill(c, [255,255,255,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_CLEAR: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasClear(canvas, color?) -> undefined",
    summary: "清空画布（默认清为透明，可指定颜色）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("color", "可选清空颜色，缺省透明 [0,0,0,0]"),
    ],
    returns: "undefined",
    examples: &[
        "canvasClear(c)  // 透明",
        "canvasClear(c, [255,255,255,255])  // 白色",
    ],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

// ===================== Canvas 绘图 =====================

static DOC_CANVAS_DRAW_LINE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawLine(canvas, x1, y1, x2, y2, color) -> undefined",
    summary: "画线段（Bresenham 算法，单像素宽）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x1, y1", "起点坐标"),
        ("x2, y2", "终点坐标"),
        ("color", "线条颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawLine(c, 0, 0, 100, 100, [255,0,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误", "越界坐标自动跳过"],
};

static DOC_CANVAS_DRAW_LINE_W: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawLineW(canvas, x1, y1, x2, y2, width, color) -> undefined",
    summary: "画带宽度的线段（圆刷模拟笔触）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x1, y1", "起点坐标"),
        ("x2, y2", "终点坐标"),
        ("width", "线条宽度（像素）"),
        ("color", "线条颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawLineW(c, 0, 0, 100, 100, 3, [0,0,255,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_DRAW_RECT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawRect(canvas, x, y, width, height, color) -> undefined",
    summary: "画矩形边框。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "左上角坐标"),
        ("width", "矩形宽度"),
        ("height", "矩形高度"),
        ("color", "边框颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawRect(c, 10, 10, 80, 40, [255,0,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_FILL_RECT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFillRect(canvas, x, y, width, height, color) -> undefined",
    summary: "填充矩形。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "左上角坐标"),
        ("width", "矩形宽度"),
        ("height", "矩形高度"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFillRect(c, 10, 10, 80, 40, [0,255,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_DRAW_ROUND_RECT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawRoundRect(canvas, x, y, width, height, radius, color) -> undefined",
    summary: "画圆角矩形边框。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "左上角坐标"),
        ("width", "矩形宽度"),
        ("height", "矩形高度"),
        ("radius", "圆角半径（自动限制为不超过宽高的一半）"),
        ("color", "边框颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawRoundRect(c, 10, 10, 80, 40, 8, [0,0,255,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_FILL_ROUND_RECT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFillRoundRect(canvas, x, y, width, height, radius, color) -> undefined",
    summary: "填充圆角矩形。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "左上角坐标"),
        ("width", "矩形宽度"),
        ("height", "矩形高度"),
        ("radius", "圆角半径（自动限制为不超过宽高的一半）"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFillRoundRect(c, 10, 10, 80, 40, 8, [255,128,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_DRAW_CIRCLE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawCircle(canvas, cx, cy, radius, color) -> undefined",
    summary: "画圆边框（中点画圆算法）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("cx, cy", "圆心坐标"),
        ("radius", "半径"),
        ("color", "边框颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawCircle(c, 50, 50, 30, [255,0,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_FILL_CIRCLE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFillCircle(canvas, cx, cy, radius, color) -> undefined",
    summary: "填充实心圆。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("cx, cy", "圆心坐标"),
        ("radius", "半径"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFillCircle(c, 50, 50, 30, [0,255,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_DRAW_ELLIPSE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawEllipse(canvas, cx, cy, rx, ry, color) -> undefined",
    summary: "画椭圆边框。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("cx, cy", "中心坐标"),
        ("rx", "水平半径"),
        ("ry", "垂直半径"),
        ("color", "边框颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawEllipse(c, 50, 50, 40, 20, [0,0,255,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误", "rx/ry <= 0 时不绘制"],
};

static DOC_CANVAS_FILL_ELLIPSE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFillEllipse(canvas, cx, cy, rx, ry, color) -> undefined",
    summary: "填充实心椭圆。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("cx, cy", "中心坐标"),
        ("rx", "水平半径"),
        ("ry", "垂直半径"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFillEllipse(c, 50, 50, 40, 20, [255,0,255,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误", "rx/ry <= 0 时不绘制"],
};

static DOC_CANVAS_DRAW_TRIANGLE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawTriangle(canvas, x1, y1, x2, y2, x3, y3, color) -> undefined",
    summary: "画三角形边框（连接三个顶点）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x1, y1", "顶点 1"),
        ("x2, y2", "顶点 2"),
        ("x3, y3", "顶点 3"),
        ("color", "边框颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasDrawTriangle(c, 50,0, 0,100, 100,100, [255,0,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误"],
};

static DOC_CANVAS_FILL_TRIANGLE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasFillTriangle(canvas, x1, y1, x2, y2, x3, y3, color) -> undefined",
    summary: "填充三角形（重心坐标法）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x1, y1", "顶点 1"),
        ("x2, y2", "顶点 2"),
        ("x3, y3", "顶点 3"),
        ("color", "填充颜色 [r,g,b,a]"),
    ],
    returns: "undefined",
    examples: &["canvasFillTriangle(c, 50,0, 0,100, 100,100, [0,255,0,255])"],
    errors: &["canvas 不是画布对象", "color 格式错误", "三点共线时不绘制"],
};

static DOC_CANVAS_DRAW_TEXT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawText(canvas, x, y, text, color, fontSize?, font?) -> undefined",
    summary: "在画布上绘制文字（抗锯齿）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "文字基线左上角坐标"),
        ("text", "要绘制的字符串"),
        ("color", "文字颜色 [r,g,b,a]"),
        ("fontSize", "可选字号（默认 16）"),
        ("font", "可选 loadFont 返回的字体对象，缺省用系统字体"),
    ],
    returns: "undefined",
    examples: &[
        "canvasDrawText(c, 10, 10, \"Hello\", [0,0,0,255])",
        "canvasDrawText(c, 10, 10, \"大字\", [255,0,0,255], 32)",
        "var f = loadFont(\"arial.ttf\")",
        "canvasDrawText(c, 10, 10, \"Hi\", [0,0,0,255], 20, f)",
    ],
    errors: &[
        "canvas 不是画布对象",
        "未找到系统默认字体时需用 loadFont 加载后作为第 7 参数传入",
        "字体文件不存在或损坏",
    ],
};

static DOC_CANVAS_DRAW_IMAGE: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawImage(canvas, img, x, y) -> undefined",
    summary: "在画布上绘制图片（透明像素跳过）。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("img", "要绘制的 image 对象"),
        ("x, y", "绘制左上角坐标"),
    ],
    returns: "undefined",
    examples: &["canvasDrawImage(c, sprite, 10, 10)"],
    errors: &["canvas 不是画布对象", "img 不是 image 对象"],
};

static DOC_CANVAS_DRAW_GRADIENT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasDrawGradient(canvas, x, y, w, h, color1, color2, direction) -> undefined",
    summary: "画线性渐变矩形。",
    params: &[
        ("canvas", "canvas 画布对象"),
        ("x, y", "左上角坐标"),
        ("w", "宽度"),
        ("h", "高度"),
        ("color1", "起始颜色 [r,g,b,a]"),
        ("color2", "结束颜色 [r,g,b,a]"),
        ("direction", "\"h\" 水平渐变 或 \"v\" 垂直渐变"),
    ],
    returns: "undefined",
    examples: &[
        "canvasDrawGradient(c, 0, 0, 100, 100, [255,0,0,255], [0,0,255,255], \"h\")",
    ],
    errors: &["canvas 不是画布对象", "direction 必须是 \"h\" 或 \"v\""],
};

static DOC_CANVAS_MEASURE_TEXT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "canvasMeasureText(text, fontSize?, font?) -> [width, height]",
    summary: "测量文字尺寸（不实际绘制）。",
    params: &[
        ("text", "要测量的字符串"),
        ("fontSize", "可选字号（默认 16）"),
        ("font", "可选 loadFont 返回的字体对象，缺省用系统字体"),
    ],
    returns: "[width, height] 文字宽高（像素）",
    examples: &[
        "var size = canvasMeasureText(\"Hello\", 20)",
        "var w = size[0]  // 文字宽度",
    ],
    errors: &[
        "未找到系统默认字体时需用 loadFont 加载后作为第 3 参数传入",
        "字体文件不存在或损坏",
    ],
};

// ===================== 颜色与字体 =====================

static DOC_COLOR_NEW: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "colorNew(r, g, b, a?) -> [r,g,b,a]",
    summary: "创建颜色数组（各分量限制到 0-255）。",
    params: &[
        ("r", "红色 0-255"),
        ("g", "绿色 0-255"),
        ("b", "蓝色 0-255"),
        ("a", "可选 alpha 0-255（默认 255）"),
    ],
    returns: "[r,g,b,a] 颜色数组",
    examples: &[
        "colorNew(255, 0, 0)  // [255,0,0,255]",
        "colorNew(255, 0, 0, 128)  // 半透明红",
    ],
    errors: &["r/g/b/a 应为数字（超出范围自动截断）"],
};

static DOC_COLOR_FROM_HEX: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "colorFromHex(hex) -> [r,g,b,a]",
    summary: "从十六进制字符串创建颜色。",
    params: &[("hex", "十六进制颜色字符串，支持 \"#rrggbb\"/\"rrggbb\"/\"#rrggbbaa\"")],
    returns: "[r,g,b,a] 颜色数组",
    examples: &[
        "colorFromHex(\"#ff0000\")  // [255,0,0,255]",
        "colorFromHex(\"ff0000\")  // 同上，# 可省",
        "colorFromHex(\"#ff000080\")  // [255,0,0,128] 半透明",
    ],
    errors: &["格式必须为 #rrggbb 或 #rrggbbaa（6 或 8 位十六进制）"],
};

static DOC_COLOR_TO_HEX: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "colorToHex(color) -> string",
    summary: "颜色转十六进制字符串。",
    params: &[("color", "颜色 [r,g,b] 或 [r,g,b,a]")],
    returns: "string 形如 \"#rrggbbaa\" 的十六进制字符串",
    examples: &["var s = colorToHex([255,0,0,255])  // \"#ff0000ff\""],
    errors: &["color 不是数组或长度不足 3"],
};

static DOC_COLOR_BLEND: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "colorBlend(color1, color2, ratio) -> [r,g,b,a]",
    summary: "按比例混合两种颜色。",
    params: &[
        ("color1", "第一个颜色 [r,g,b,a]"),
        ("color2", "第二个颜色 [r,g,b,a]"),
        ("ratio", "混合比例（0.0 完全用 color1，1.0 完全用 color2）"),
    ],
    returns: "[r,g,b,a] 混合后的颜色",
    examples: &[
        "colorBlend([0,0,0,255], [255,255,255,255], 0.5)  // 中灰色",
    ],
    errors: &["color1/color2 不是颜色数组", "ratio 超出 [0,1] 自动截断"],
};

static DOC_LOAD_FONT: BuiltinDoc = BuiltinDoc {
    category: "image",
    signature: "loadFont(fontPath) -> font",
    summary: "从文件加载 TrueType/OpenType 字体。",
    params: &[("fontPath", "字体文件路径（.ttf/.otf/.ttc）")],
    returns: "font 字体对象，用于 canvasDrawText/canvasMeasureText",
    examples: &[
        "var f = loadFont(\"C:\\\\Windows\\\\Fonts\\\\arial.ttf\")",
        "canvasDrawText(c, 10, 10, \"Hi\", [0,0,0,255], 20, f)",
    ],
    errors: &["文件不存在或无权限", "文件不是有效的 TrueType/OpenType 字体"],
};

/// register 注册所有图像处理内置函数。
pub fn register(vm: &mut VM) {
    // 图片加载/保存
    vm.register_builtin_doc("imageLoad", bi_image_load, &DOC_IMAGE_LOAD);
    vm.register_builtin_doc("imageLoadFromBytes", bi_image_load_from_bytes, &DOC_IMAGE_LOAD_FROM_BYTES);
    vm.register_builtin_doc("imageSave", bi_image_save, &DOC_IMAGE_SAVE);
    vm.register_builtin_doc("imageSaveToBytes", bi_image_save_to_bytes, &DOC_IMAGE_SAVE_TO_BYTES);

    // 图片基本操作
    vm.register_builtin_doc("imageNew", bi_image_new, &DOC_IMAGE_NEW);
    vm.register_builtin_doc("imageGetWidth", bi_image_get_width, &DOC_IMAGE_GET_WIDTH);
    vm.register_builtin_doc("imageGetHeight", bi_image_get_height, &DOC_IMAGE_GET_HEIGHT);
    vm.register_builtin_doc("imageGetPixel", bi_image_get_pixel, &DOC_IMAGE_GET_PIXEL);
    vm.register_builtin_doc("imageSetPixel", bi_image_set_pixel, &DOC_IMAGE_SET_PIXEL);
    vm.register_builtin_doc("imageFill", bi_image_fill, &DOC_IMAGE_FILL);
    vm.register_builtin_doc("imageClone", bi_image_clone, &DOC_IMAGE_CLONE);

    // 图片变换
    vm.register_builtin_doc("imageResize", bi_image_resize, &DOC_IMAGE_RESIZE);
    vm.register_builtin_doc("imageCrop", bi_image_crop, &DOC_IMAGE_CROP);
    vm.register_builtin_doc("imageRotate", bi_image_rotate, &DOC_IMAGE_ROTATE);
    vm.register_builtin_doc("imageRotateFree", bi_image_rotate_free, &DOC_IMAGE_ROTATE_FREE);
    vm.register_builtin_doc("imageFlipH", bi_image_flip_h, &DOC_IMAGE_FLIP_H);
    vm.register_builtin_doc("imageFlipV", bi_image_flip_v, &DOC_IMAGE_FLIP_V);
    vm.register_builtin_doc("imageBlend", bi_image_blend, &DOC_IMAGE_BLEND);

    // 颜色滤镜
    vm.register_builtin_doc("imageGray", bi_image_gray, &DOC_IMAGE_GRAY);
    vm.register_builtin_doc("imageInvert", bi_image_invert, &DOC_IMAGE_INVERT);
    vm.register_builtin_doc("imageBrightness", bi_image_brightness, &DOC_IMAGE_BRIGHTNESS);
    vm.register_builtin_doc("imageContrast", bi_image_contrast, &DOC_IMAGE_CONTRAST);
    vm.register_builtin_doc("imageBlur", bi_image_blur, &DOC_IMAGE_BLUR);
    vm.register_builtin_doc("imageSharpen", bi_image_sharpen, &DOC_IMAGE_SHARPEN);
    vm.register_builtin_doc("imageGamma", bi_image_gamma, &DOC_IMAGE_GAMMA);
    vm.register_builtin_doc("imageSepia", bi_image_sepia, &DOC_IMAGE_SEPIA);
    vm.register_builtin_doc("imageThreshold", bi_image_threshold, &DOC_IMAGE_THRESHOLD);
    vm.register_builtin_doc("imageTint", bi_image_tint, &DOC_IMAGE_TINT);
    vm.register_builtin_doc("imageOpacity", bi_image_opacity, &DOC_IMAGE_OPACITY);
    vm.register_builtin_doc("imageEdgeDetect", bi_image_edge_detect, &DOC_IMAGE_EDGE_DETECT);
    vm.register_builtin_doc("imageConvolve3x3", bi_image_convolve3x3, &DOC_IMAGE_CONVOLVE3X3);

    // 图片信息
    vm.register_builtin_doc("imageHistogram", bi_image_histogram, &DOC_IMAGE_HISTOGRAM);

    // Canvas 画布
    vm.register_builtin_doc("canvasNew", bi_canvas_new, &DOC_CANVAS_NEW);
    vm.register_builtin_doc("canvasFromImage", bi_canvas_from_image, &DOC_CANVAS_FROM_IMAGE);
    vm.register_builtin_doc("canvasToImage", bi_canvas_to_image, &DOC_CANVAS_TO_IMAGE);
    vm.register_builtin_doc("canvasGetWidth", bi_canvas_get_width, &DOC_CANVAS_GET_WIDTH);
    vm.register_builtin_doc("canvasGetHeight", bi_canvas_get_height, &DOC_CANVAS_GET_HEIGHT);
    vm.register_builtin_doc("canvasGetPixel", bi_canvas_get_pixel, &DOC_CANVAS_GET_PIXEL);
    vm.register_builtin_doc("canvasSetPixel", bi_canvas_set_pixel, &DOC_CANVAS_SET_PIXEL);
    vm.register_builtin_doc("canvasFill", bi_canvas_fill, &DOC_CANVAS_FILL);
    vm.register_builtin_doc("canvasClear", bi_canvas_clear, &DOC_CANVAS_CLEAR);

    // Canvas 绘图
    vm.register_builtin_doc("canvasDrawLine", bi_canvas_draw_line, &DOC_CANVAS_DRAW_LINE);
    vm.register_builtin_doc("canvasDrawLineW", bi_canvas_draw_line_w, &DOC_CANVAS_DRAW_LINE_W);
    vm.register_builtin_doc("canvasDrawRect", bi_canvas_draw_rect, &DOC_CANVAS_DRAW_RECT);
    vm.register_builtin_doc("canvasFillRect", bi_canvas_fill_rect, &DOC_CANVAS_FILL_RECT);
    vm.register_builtin_doc("canvasDrawRoundRect", bi_canvas_draw_round_rect, &DOC_CANVAS_DRAW_ROUND_RECT);
    vm.register_builtin_doc("canvasFillRoundRect", bi_canvas_fill_round_rect, &DOC_CANVAS_FILL_ROUND_RECT);
    vm.register_builtin_doc("canvasDrawCircle", bi_canvas_draw_circle, &DOC_CANVAS_DRAW_CIRCLE);
    vm.register_builtin_doc("canvasFillCircle", bi_canvas_fill_circle, &DOC_CANVAS_FILL_CIRCLE);
    vm.register_builtin_doc("canvasDrawEllipse", bi_canvas_draw_ellipse, &DOC_CANVAS_DRAW_ELLIPSE);
    vm.register_builtin_doc("canvasFillEllipse", bi_canvas_fill_ellipse, &DOC_CANVAS_FILL_ELLIPSE);
    vm.register_builtin_doc("canvasDrawTriangle", bi_canvas_draw_triangle, &DOC_CANVAS_DRAW_TRIANGLE);
    vm.register_builtin_doc("canvasFillTriangle", bi_canvas_fill_triangle, &DOC_CANVAS_FILL_TRIANGLE);
    vm.register_builtin_doc("canvasDrawText", bi_canvas_draw_text, &DOC_CANVAS_DRAW_TEXT);
    vm.register_builtin_doc("canvasDrawImage", bi_canvas_draw_image, &DOC_CANVAS_DRAW_IMAGE);
    vm.register_builtin_doc("canvasDrawGradient", bi_canvas_draw_gradient, &DOC_CANVAS_DRAW_GRADIENT);
    vm.register_builtin_doc("canvasMeasureText", bi_canvas_measure_text, &DOC_CANVAS_MEASURE_TEXT);

    // 颜色与字体
    vm.register_builtin_doc("colorNew", bi_color_new, &DOC_COLOR_NEW);
    vm.register_builtin_doc("colorFromHex", bi_color_from_hex, &DOC_COLOR_FROM_HEX);
    vm.register_builtin_doc("colorToHex", bi_color_to_hex, &DOC_COLOR_TO_HEX);
    vm.register_builtin_doc("colorBlend", bi_color_blend, &DOC_COLOR_BLEND);
    vm.register_builtin_doc("loadFont", bi_load_font, &DOC_LOAD_FONT);
}

// ============ 类型定义 ============

/// ImageState 图片对象，封装 image::DynamicImage。
///
/// 用 Mutex 保护以支持跨线程访问（run 关键字并发场景）。
/// 大多数操作返回新 Image，不就地修改（函数式风格）。
pub struct ImageState {
    /// img 内部 DynamicImage。
    pub img: Mutex<DynamicImage>,
}

/// CanvasState 画布对象，封装 RgbaImage 像素缓冲区。
///
/// 用于绘图操作（画线/矩形/圆/文字），支持直接像素读写。
pub struct CanvasState {
    /// buffer 像素缓冲区（RGBA 8位）。
    pub buffer: Mutex<RgbaImage>,
}

/// FontState 字体对象，封装 rusttype::Font。
///
/// 用于 canvasDrawText 文字渲染，支持 TrueType/OpenType 字体。
pub struct FontState {
    /// font 内部 rusttype Font。
    pub font: Font<'static>,
}

// ============ 辅助函数 ============

/// value_to_int 从 Value 提取整数（Int 直接返回，Float 截断为 i64）。
pub(crate) fn value_to_int(v: &Value, fn_name: &str, name: &str) -> Result<i64, Value> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Float(f) => Ok(*f as i64),
        _ => Err(error_value(format!(
            "{}() 颜色分量 {} 应为 int，得到 {} (可能原因：传入了错误类型)",
            fn_name, name, v.type_name(),
        ))),
    }
}

/// clamp_byte 将 Value 限制到 0-255 范围并转为 u8。
pub(crate) fn clamp_byte(v: &Value, fn_name: &str, name: &str) -> Result<u8, Value> {
    let n = value_to_int(v, fn_name, name)?;
    let clamped = n.max(0).min(255) as u8;
    Ok(clamped)
}

/// parse_color 从 Value 中解析颜色。
///
/// 接受 [r, g, b] 或 [r, g, b, a] 数组，a 默认 255。
/// 也接受 [r, g, b, a] 中元素为 Float 的场景（自动截断为 int）。
pub(crate) fn parse_color(v: &Value, fn_name: &str) -> Result<Rgba<u8>, Value> {
    let arr = match v {
        Value::Array(a) => a,
        Value::Undefined => {
            return Err(error_value(format!(
                "{}() 颜色参数为 undefined (可能原因：变量未初始化)",
                fn_name,
            )))
        }
        other => {
            return Err(error_value(format!(
                "{}() 颜色参数应为 [r, g, b] 或 [r, g, b, a] 数组，得到 {} (可能原因：参数顺序错误或未用 [] 字面量创建)",
                fn_name, other.type_name(),
            )))
        }
    };
    let guard = arr.lock().unwrap();
    if guard.len() < 3 {
        return Err(error_value(format!(
            "{}() 颜色参数应为 [r, g, b] 或 [r, g, b, a]，长度为 {} (可能原因：数组长度不足)",
            fn_name, guard.len(),
        )));
    }
    let r = clamp_byte(&guard[0], fn_name, "r")?;
    let g = clamp_byte(&guard[1], fn_name, "g")?;
    let b = clamp_byte(&guard[2], fn_name, "b")?;
    let a = if guard.len() >= 4 {
        clamp_byte(&guard[3], fn_name, "a")?
    } else {
        255
    };
    Ok(Rgba([r, g, b, a]))
}

/// bytes_from_value 从 Value 中提取字节 Vec（兼容 bytes 和 byteArray）。
fn bytes_from_value(v: &Value, fn_name: &str) -> Result<Vec<u8>, Value> {
    match v {
        Value::Bytes(b) => Ok(b.as_ref().to_vec()),
        Value::ByteArray(b) => Ok(b.lock().unwrap().clone()),
        Value::Undefined => Err(error_value(format!(
            "{}() 字节参数为 undefined (可能原因：变量未初始化)",
            fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 字节参数应为 bytes/byteArray，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// color_to_value 将 Rgba<u8> 转为 Value::Array。
pub(crate) fn color_to_value(c: Rgba<u8>) -> Value {
    let arr = vec![
        Value::Int(c[0] as i64),
        Value::Int(c[1] as i64),
        Value::Int(c[2] as i64),
        Value::Int(c[3] as i64),
    ];
    Value::Array(Arc::new(Mutex::new(arr)))
}

/// image_downcast 从 Value 中提取 ImageState 引用。
fn image_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<ImageState>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<ImageState>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 image 对象 (可能原因：传入了错误类型 {}，应先用 imageLoad/imageLoadFromBytes/imageNew 创建)",
                fn_name, v.type_name_ex(),
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 image，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// canvas_downcast 从 Value 中提取 CanvasState 引用。
fn canvas_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<CanvasState>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<CanvasState>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 canvas 对象 (可能原因：传入了错误类型 {}，应先用 canvasNew/canvasFromImage 创建)",
                fn_name, v.type_name_ex(),
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 canvas，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// font_downcast 从 Value 中提取 FontState 引用。
fn font_downcast<'a>(v: &'a Value, fn_name: &str) -> Result<&'a Arc<FontState>, Value> {
    match v {
        Value::Native(n) => n.downcast_ref::<Arc<FontState>>().ok_or_else(|| {
            error_value(format!(
                "{}() 参数不是 font 对象 (可能原因：传入了错误类型 {}，应先用 loadFont 创建)",
                fn_name, v.type_name_ex(),
            ))
        }),
        Value::Undefined => Err(error_value(format!(
            "{}() 参数为 undefined (可能原因：变量未初始化)", fn_name,
        ))),
        other => Err(error_value(format!(
            "{}() 参数应为 font，得到 {} (可能原因：参数顺序错误)",
            fn_name, other.type_name(),
        ))),
    }
}

/// wrap_image 将 DynamicImage 包装为 Value::Native。
pub(crate) fn wrap_image(img: DynamicImage) -> Value {
    Value::Native(Arc::new(Arc::new(ImageState {
        img: Mutex::new(img),
    })))
}

/// wrap_canvas 将 RgbaImage 包装为 Value::Native。
pub(crate) fn wrap_canvas(buffer: RgbaImage) -> Value {
    Value::Native(Arc::new(Arc::new(CanvasState {
        buffer: Mutex::new(buffer),
    })))
}

/// parse_format 将格式字符串转为 image::ImageFormat。
fn parse_format(s: &str) -> Result<image::ImageFormat, String> {
    match s.to_lowercase().as_str() {
        "png" => Ok(image::ImageFormat::Png),
        "jpg" | "jpeg" => Ok(image::ImageFormat::Jpeg),
        "gif" => Ok(image::ImageFormat::Gif),
        "bmp" => Ok(image::ImageFormat::Bmp),
        "webp" => Ok(image::ImageFormat::WebP),
        other => Err(format!(
            "不支持的图片格式: {} (可能原因：拼写错误，支持 png/jpg/jpeg/gif/bmp/webp)",
            other,
        )),
    }
}

/// parse_filter 将滤镜字符串转为 image::imageops::FilterType。
fn parse_filter(s: &str) -> image::imageops::FilterType {
    match s.to_lowercase().as_str() {
        "nearest" => image::imageops::FilterType::Nearest,
        "triangle" | "bilinear" => image::imageops::FilterType::Triangle,
        "catmullrom" | "catrom" => image::imageops::FilterType::CatmullRom,
        "gaussian" => image::imageops::FilterType::Gaussian,
        "lanczos3" | "lanczos" => image::imageops::FilterType::Lanczos3,
        _ => image::imageops::FilterType::Lanczos3,
    }
}

/// system_font_path 返回系统默认字体路径。
fn system_font_path() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        // Windows 常用字体
        let candidates = [
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\simsun.ttc",
        ];
        for p in &candidates {
            if std::path::Path::new(p).exists() {
                return Some(p);
            }
        }
        None
    } else if cfg!(target_os = "linux") {
        let candidates = [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
        ];
        for p in &candidates {
            if std::path::Path::new(p).exists() {
                return Some(p);
            }
        }
        None
    } else {
        None
    }
}

// ============ 图片加载/保存 ============

/// bi_image_load 从文件加载图片。
///
/// 用法：
///   imageLoad(path) → image
///
/// 自动识别格式（PNG/JPEG/GIF/BMP/WebP）。
fn bi_image_load(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "imageLoad")?;
    let img = image::open(path).map_err(|e| {
        error_value(format!(
            "imageLoad() 加载 '{}' 失败: {} (可能原因：文件不存在或不是支持的图片格式)",
            path, e,
        ))
    })?;
    Ok(wrap_image(img))
}

/// bi_image_load_from_bytes 从字节数据加载图片。
///
/// 用法：
///   imageLoadFromBytes(bytes) → image            自动识别格式
///   imageLoadFromBytes(bytes, format) → image     指定格式
///
/// bytes: bytes/byteArray
/// format: "png"/"jpg"/"gif"/"bmp"/"webp"（可选）
fn bi_image_load_from_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    bh::require_arg(args, 0, "imageLoadFromBytes")?;
    let data = bytes_from_value(&args[0], "imageLoadFromBytes")?;
    let img = if args.len() > 1 {
        let fmt_str = bh::as_str(args, 1, "imageLoadFromBytes")?;
        let format = parse_format(fmt_str).map_err(error_value)?;
        image::load_from_memory_with_format(&data, format).map_err(|e| {
            error_value(format!(
                "imageLoadFromBytes() 解析失败: {} (可能原因：数据损坏或格式不匹配)",
                e,
            ))
        })?
    } else {
        image::load_from_memory(&data).map_err(|e| {
            error_value(format!(
                "imageLoadFromBytes() 解析失败: {} (可能原因：数据不是有效的图片或格式不支持)",
                e,
            ))
        })?
    };
    Ok(wrap_image(img))
}

/// bi_image_save 保存图片到文件。
///
/// 用法：
///   imageSave(img, path, format)
///
/// format: "png"/"jpg"/"gif"/"bmp"/"webp"
fn bi_image_save(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageSave")?;
    let path = bh::as_str(args, 1, "imageSave")?;
    let fmt_str = bh::as_str(args, 2, "imageSave")?;
    let format = parse_format(fmt_str).map_err(error_value)?;

    let img = img_state.img.lock().unwrap();
    img.save_with_format(path, format).map_err(|e| {
        error_value(format!(
            "imageSave() 保存 '{}' 失败: {} (可能原因：路径不可写或磁盘空间不足)",
            path, e,
        ))
    })?;
    Ok(Value::Undefined)
}

/// bi_image_save_to_bytes 将图片保存为字节。
///
/// 用法：
///   imageSaveToBytes(img, format) → bytes
///
/// format: "png"/"jpg"/"gif"/"bmp"/"webp"
fn bi_image_save_to_bytes(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageSaveToBytes")?;
    let fmt_str = bh::as_str(args, 1, "imageSaveToBytes")?;
    let format = parse_format(fmt_str).map_err(error_value)?;

    let img = img_state.img.lock().unwrap();
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, format).map_err(|e| {
        error_value(format!(
            "imageSaveToBytes() 编码失败: {} (可能原因：图片数据无效)",
            e,
        ))
    })?;
    Ok(Value::Bytes(Arc::new(buf.into_inner())))
}

// ============ 图片基本操作 ============

/// bi_image_new 创建新图片（纯色填充）。
///
/// 用法：
///   imageNew(width, height) → image           默认透明
///   imageNew(width, height, bgColor) → image   指定背景色
fn bi_image_new(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let width = bh::as_int(args, 0, "imageNew")? as u32;
    let height = bh::as_int(args, 1, "imageNew")? as u32;
    if width == 0 || height == 0 {
        return Err(error_value(format!(
            "imageNew() 宽度和高度不能为 0 (得到 {}x{}) (可能原因：参数计算错误)",
            width, height,
        )));
    }

    let pixel = if args.len() > 2 {
        parse_color(&args[2], "imageNew")?
    } else {
        Rgba([0, 0, 0, 0]) // 透明
    };

    let buffer: RgbaImage = ImageBuffer::from_pixel(width, height, pixel);
    Ok(wrap_image(DynamicImage::ImageRgba8(buffer)))
}

/// bi_image_get_width 获取图片宽度。
fn bi_image_get_width(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageGetWidth")?;
    let img = img_state.img.lock().unwrap();
    Ok(Value::Int(img.width() as i64))
}

/// bi_image_get_height 获取图片高度。
fn bi_image_get_height(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageGetHeight")?;
    let img = img_state.img.lock().unwrap();
    Ok(Value::Int(img.height() as i64))
}

/// bi_image_get_pixel 获取像素颜色。
///
/// 用法：
///   imageGetPixel(img, x, y) → [r, g, b, a]
fn bi_image_get_pixel(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageGetPixel")?;
    let x = bh::as_int(args, 1, "imageGetPixel")? as u32;
    let y = bh::as_int(args, 2, "imageGetPixel")? as u32;
    let img = img_state.img.lock().unwrap();
    if x >= img.width() || y >= img.height() {
        return Err(error_value(format!(
            "imageGetPixel() 坐标 ({}, {}) 超出图片范围 ({}x{}) (可能原因：坐标从 0 开始计算)",
            x, y, img.width(), img.height(),
        )));
    }
    let pixel = img.get_pixel(x, y);
    Ok(color_to_value(pixel))
}

/// bi_image_set_pixel 设置像素颜色。
///
/// 用法：
///   imageSetPixel(img, x, y, color)
fn bi_image_set_pixel(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageSetPixel")?;
    let x = bh::as_int(args, 1, "imageSetPixel")? as u32;
    let y = bh::as_int(args, 2, "imageSetPixel")? as u32;
    let color = parse_color(&args[3], "imageSetPixel")?;

    let mut img = img_state.img.lock().unwrap();
    if x >= img.width() || y >= img.height() {
        return Err(error_value(format!(
            "imageSetPixel() 坐标 ({}, {}) 超出图片范围 ({}x{}) (可能原因：坐标从 0 开始计算)",
            x, y, img.width(), img.height(),
        )));
    }
    // 转为 RGBA8 操作
    let mut rgba = img.to_rgba8();
    rgba.put_pixel(x, y, color);
    *img = DynamicImage::ImageRgba8(rgba);
    Ok(Value::Undefined)
}

/// bi_image_fill 用指定颜色填充整个图片。
///
/// 用法：
///   imageFill(img, color)
fn bi_image_fill(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageFill")?;
    let color = parse_color(&args[1], "imageFill")?;
    let mut img = img_state.img.lock().unwrap();
    let (w, h) = (img.width(), img.height());
    let buffer: RgbaImage = ImageBuffer::from_pixel(w, h, color);
    *img = DynamicImage::ImageRgba8(buffer);
    Ok(Value::Undefined)
}

/// bi_image_clone 深拷贝图片。
///
/// 用法：
///   imageClone(img) → image
///
/// 返回与原图互不影响的独立副本。
fn bi_image_clone(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageClone")?;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.clone()))
}

// ============ 图片变换 ============

/// bi_image_resize 缩放图片。
///
/// 用法：
///   imageResize(img, width, height) → image              默认 Lanczos3
///   imageResize(img, width, height, filter) → image     指定滤镜
///
/// filter: "nearest"/"triangle"/"catmullrom"/"gaussian"/"lanczos3"
fn bi_image_resize(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageResize")?;
    let width = bh::as_int(args, 1, "imageResize")? as u32;
    let height = bh::as_int(args, 2, "imageResize")? as u32;
    if width == 0 || height == 0 {
        return Err(error_value("imageResize() 目标尺寸不能为 0 (可能原因：参数计算错误)"));
    }
    let filter = if args.len() > 3 {
        let s = bh::as_str(args, 3, "imageResize")?;
        parse_filter(s)
    } else {
        image::imageops::FilterType::Lanczos3
    };

    let img = img_state.img.lock().unwrap();
    let resized = img.resize(width, height, filter);
    Ok(wrap_image(resized))
}

/// bi_image_crop 裁剪图片。
///
/// 用法：
///   imageCrop(img, x, y, width, height) → image
fn bi_image_crop(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageCrop")?;
    let x = bh::as_int(args, 1, "imageCrop")? as u32;
    let y = bh::as_int(args, 2, "imageCrop")? as u32;
    let width = bh::as_int(args, 3, "imageCrop")? as u32;
    let height = bh::as_int(args, 4, "imageCrop")? as u32;

    let img = img_state.img.lock().unwrap();
    if x + width > img.width() || y + height > img.height() {
        return Err(error_value(format!(
            "imageCrop() 裁剪区域 ({},{},{},{}) 超出图片范围 ({}x{}) (可能原因：坐标或尺寸计算错误)",
            x, y, width, height, img.width(), img.height(),
        )));
    }
    let cropped = img.crop_imm(x, y, width, height);
    Ok(wrap_image(cropped))
}

/// bi_image_rotate 旋转图片（仅支持 90/180/270 度）。
///
/// 用法：
///   imageRotate(img, degrees) → image
///
/// degrees: 90/180/270（正值为顺时针）
fn bi_image_rotate(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageRotate")?;
    let degrees = bh::as_int(args, 1, "imageRotate")?;

    let img = img_state.img.lock().unwrap();
    let rotated = match degrees {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        -90 => img.rotate270(),
        -180 => img.rotate180(),
        -270 => img.rotate90(),
        other => {
            return Err(error_value(format!(
                "imageRotate() 仅支持 90/180/270 度，得到 {} (可能原因：任意角度旋转请用 imageRotateFree)",
                other,
            )))
        }
    };
    Ok(wrap_image(rotated))
}

/// bi_image_flip_h 水平翻转图片。
fn bi_image_flip_h(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageFlipH")?;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.fliph()))
}

/// bi_image_flip_v 垂直翻转图片。
fn bi_image_flip_v(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageFlipV")?;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.flipv()))
}

/// bi_image_rotate_free 任意角度旋转图片。
///
/// 用法：
///   imageRotateFree(img, degrees) → image
///   imageRotateFree(img, degrees, bgColor) → image
///
/// degrees: 任意角度（正值为逆时针）
/// bgColor: 旋转后空白区域填充色，默认透明
fn bi_image_rotate_free(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageRotateFree")?;
    let degrees = bh::as_float(args, 1, "imageRotateFree")?;
    let bg = if args.len() > 2 {
        parse_color(&args[2], "imageRotateFree")?
    } else {
        Rgba([0, 0, 0, 0])
    };
    let img = img_state.img.lock().unwrap();
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let radians = degrees.to_radians();
    let cos_t = radians.cos();
    let sin_t = radians.sin();
    // 计算旋转后画布大小（包围所有原图角点）
    let new_w = ((w as f64 * cos_t).abs() + (h as f64 * sin_t).abs()).ceil() as u32;
    let new_h = ((w as f64 * sin_t).abs() + (h as f64 * cos_t).abs()).ceil() as u32;
    let mut result = ImageBuffer::from_pixel(new_w, new_h, bg);
    let cx = new_w as f64 / 2.0;
    let cy = new_h as f64 / 2.0;
    let src_cx = w as f64 / 2.0;
    let src_cy = h as f64 / 2.0;
    // 逆映射 + 最近邻采样
    for y in 0..new_h {
        for x in 0..new_w {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            // 逆旋转：从目标坐标推回源坐标
            let sx = dx * cos_t + dy * sin_t + src_cx;
            let sy = -dx * sin_t + dy * cos_t + src_cy;
            if sx >= 0.0 && sx < w as f64 && sy >= 0.0 && sy < h as f64 {
                let px = rgba.get_pixel(sx as u32, sy as u32);
                result.put_pixel(x, y, *px);
            }
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

/// bi_image_blend 将叠加图绘制到基础图上（alpha 混合）。
///
/// 用法：
///   imageBlend(baseImg, overlayImg, x, y) → image
///
/// 在 baseImg 的 (x,y) 处叠加 overlayImg，半透明像素自动混合。
/// 返回新的混合后图片。
fn bi_image_blend(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let base_state = image_downcast(&args[0], "imageBlend")?;
    let overlay_state = image_downcast(&args[1], "imageBlend")?;
    let x = bh::as_int(args, 2, "imageBlend")? as i64;
    let y = bh::as_int(args, 3, "imageBlend")? as i64;

    let base = base_state.img.lock().unwrap();
    let overlay = overlay_state.img.lock().unwrap();
    let mut result = base.to_rgba8();
    let overlay_rgba = overlay.to_rgba8();

    for oy in 0..overlay_rgba.height() {
        for ox in 0..overlay_rgba.width() {
            let px = x + ox as i64;
            let py = y + oy as i64;
            if px < 0 || py < 0 || px >= result.width() as i64 || py >= result.height() as i64 {
                continue;
            }
            let src = overlay_rgba.get_pixel(ox, oy);
            if src[3] == 0 {
                continue;
            }
            let dst = result.get_pixel_mut(px as u32, py as u32);
            let alpha = src[3] as f32 / 255.0;
            for i in 0..3 {
                dst[i] = (dst[i] as f32 * (1.0 - alpha) + src[i] as f32 * alpha) as u8;
            }
            dst[3] = (dst[3] as f32 * (1.0 - alpha) + 255.0 * alpha) as u8;
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

// ============ 颜色滤镜 ============

/// bi_image_gray 灰度化图片。
fn bi_image_gray(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageGray")?;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.grayscale()))
}

/// bi_image_invert 反色图片。
fn bi_image_invert(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageInvert")?;
    let mut img = img_state.img.lock().unwrap();
    // invert 是就地修改，需要克隆
    let mut cloned = img.clone();
    cloned.invert();
    *img = cloned;
    Ok(Value::Undefined)
}

/// bi_image_brightness 调整亮度。
///
/// 用法：
///   imageBrightness(img, factor) → image
///
/// factor: -255 到 255，正值变亮，负值变暗
fn bi_image_brightness(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageBrightness")?;
    let factor = bh::as_int(args, 1, "imageBrightness")? as i32;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.brighten(factor)))
}

/// bi_image_contrast 调整对比度。
///
/// 用法：
///   imageContrast(img, factor) → image
///
/// factor: f32，正值增加对比度，负值降低
fn bi_image_contrast(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageContrast")?;
    let factor = bh::as_float(args, 1, "imageContrast")? as f32;
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.adjust_contrast(factor)))
}

/// bi_image_blur 高斯模糊。
///
/// 用法：
///   imageBlur(img, sigma) → image
///
/// sigma: 模糊半径（越大越模糊，通常 1.0-10.0）
fn bi_image_blur(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageBlur")?;
    let sigma = bh::as_float(args, 1, "imageBlur")?;
    if sigma < 0.0 {
        return Err(error_value(format!(
            "imageBlur() sigma 不能为负数，得到 {} (可能原因：参数计算错误)",
            sigma,
        )));
    }
    let img = img_state.img.lock().unwrap();
    Ok(wrap_image(img.blur(sigma as f32)))
}

/// bi_image_sharpen 锐化。
///
/// 用法：
///   imageSharpen(img) → image
fn bi_image_sharpen(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageSharpen")?;
    let img = img_state.img.lock().unwrap();
    // 锐化卷积核：中心权重高，周围为负
    let result = image::imageops::filter3x3(&*img, &[
        0.0, -1.0, 0.0,
        -1.0, 5.0, -1.0,
        0.0, -1.0, 0.0,
    ]);
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

/// bi_image_gamma 伽马校正。
///
/// 用法：
///   imageGamma(img, gamma) → image
///
/// gamma: > 1 变亮，< 1 变暗，1 不变
fn bi_image_gamma(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageGamma")?;
    let gamma = bh::as_float(args, 1, "imageGamma")?;
    if gamma <= 0.0 {
        return Err(error_value(format!(
            "imageGamma() gamma 必须 > 0，得到 {} (可能原因：参数计算错误)",
            gamma,
        )));
    }
    let img = img_state.img.lock().unwrap();
    let mut rgba = img.to_rgba8();
    let inv = 1.0 / gamma;
    for px in rgba.pixels_mut() {
        for i in 0..3 {
            let v = px[i] as f64 / 255.0;
            let corrected = v.powf(inv) * 255.0;
            px[i] = corrected.round().clamp(0.0, 255.0) as u8;
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(rgba)))
}

/// bi_image_sepia 棕褐色滤镜（复古效果）。
///
/// 用法：
///   imageSepia(img) → image
fn bi_image_sepia(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageSepia")?;
    let img = img_state.img.lock().unwrap();
    let mut rgba = img.to_rgba8();
    for px in rgba.pixels_mut() {
        let r = px[0] as f32;
        let g = px[1] as f32;
        let b = px[2] as f32;
        // 标准棕褐色矩阵
        let nr = (r * 0.393 + g * 0.769 + b * 0.189).min(255.0) as u8;
        let ng = (r * 0.349 + g * 0.686 + b * 0.168).min(255.0) as u8;
        let nb = (r * 0.272 + g * 0.534 + b * 0.131).min(255.0) as u8;
        px[0] = nr;
        px[1] = ng;
        px[2] = nb;
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(rgba)))
}

/// bi_image_threshold 二值化阈值。
///
/// 用法：
///   imageThreshold(img, threshold) → image
///
/// threshold: 0-255，灰度 > threshold 变白，否则变黑
fn bi_image_threshold(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageThreshold")?;
    let threshold = bh::as_int(args, 1, "imageThreshold")? as u8;
    let img = img_state.img.lock().unwrap();
    let gray = img.grayscale().to_rgba8();
    let mut result = gray.clone();
    for px in result.pixels_mut() {
        let val = if px[0] > threshold { 255 } else { 0 };
        px[0] = val;
        px[1] = val;
        px[2] = val;
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

/// bi_image_tint 用指定颜色着色（保留亮度，替换色调）。
///
/// 用法：
///   imageTint(img, color) → image
fn bi_image_tint(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageTint")?;
    let tint = parse_color(&args[1], "imageTint")?;
    let img = img_state.img.lock().unwrap();
    let mut rgba = img.to_rgba8();
    for px in rgba.pixels_mut() {
        // 按灰度混合，保留亮度变化
        let gray = (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000;
        let factor = gray as f32 / 255.0;
        px[0] = (tint[0] as f32 * factor) as u8;
        px[1] = (tint[1] as f32 * factor) as u8;
        px[2] = (tint[2] as f32 * factor) as u8;
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(rgba)))
}

/// bi_image_opacity 调整透明度。
///
/// 用法：
///   imageOpacity(img, factor) → image
///
/// factor: 0.0(全透明) 到 1.0(不变) 到 2.0+（可超出，截断到255）
fn bi_image_opacity(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageOpacity")?;
    let factor = bh::as_float(args, 1, "imageOpacity")? as f32;
    let img = img_state.img.lock().unwrap();
    let mut rgba = img.to_rgba8();
    for px in rgba.pixels_mut() {
        let new_alpha = (px[3] as f32 * factor).clamp(0.0, 255.0) as u8;
        px[3] = new_alpha;
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(rgba)))
}

/// bi_image_edge_detect 边缘检测（Sobel 算子）。
///
/// 用法：
///   imageEdgeDetect(img) → image
fn bi_image_edge_detect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageEdgeDetect")?;
    let img = img_state.img.lock().unwrap();
    let gray = img.grayscale().to_luma8();
    let (w, h) = gray.dimensions();
    if w < 3 || h < 3 {
        return Ok(wrap_image(img.grayscale()));
    }
    let mut result = ImageBuffer::new(w, h);
    // Sobel 算子
    let gx: [[i32; 3]; 3] = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]];
    let gy: [[i32; 3]; 3] = [[-1, -2, -1], [0, 0, 0], [1, 2, 1]];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut sx = 0i32;
            let mut sy = 0i32;
            for j in 0..3usize {
                for i in 0..3usize {
                    let p = gray.get_pixel(x + i as u32 - 1, y + j as u32 - 1)[0] as i32;
                    sx += p * gx[j][i];
                    sy += p * gy[j][i];
                }
            }
            let mag = ((sx * sx + sy * sy) as f64).sqrt() as i32;
            let val = mag.clamp(0, 255) as u8;
            result.put_pixel(x, y, Rgba([val, val, val, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

/// bi_image_convolve3x3 自定义 3x3 卷积核。
///
/// 用法：
///   imageConvolve3x3(img, kernel) → image
///
/// kernel: 9 个数字的数组 [k0,k1,k2,k3,k4,k5,k6,k7,k8]
/// 对应 3x3 矩阵，自动归一化（除以核之和，和为 0 时不除）
fn bi_image_convolve3x3(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageConvolve3x3")?;
    let arr = match &args[1] {
        Value::Array(a) => a.clone(),
        other => return Err(error_value(format!(
            "imageConvolve3x3() kernel 应为 9 元素数组，得到 {} (可能原因：参数顺序错误)",
            other.type_name(),
        ))),
    };
    let guard = arr.lock().unwrap();
    if guard.len() != 9 {
        return Err(error_value(format!(
            "imageConvolve3x3() kernel 必须为 9 元素数组，得到 {} 个 (可能原因：3x3 卷积核需要恰好 9 个值)",
            guard.len(),
        )));
    }
    let mut kernel = [0.0f32; 9];
    for i in 0..9 {
        kernel[i] = match &guard[i] {
            Value::Int(n) => *n as f32,
            Value::Float(f) => *f as f32,
            other => return Err(error_value(format!(
                "imageConvolve3x3() kernel[{}] 应为数字，得到 {} (可能原因：数组元素类型错误)",
                i, other.type_name(),
            ))),
        };
    }
    drop(guard);

    let img = img_state.img.lock().unwrap();
    let result = image::imageops::filter3x3(&*img, &kernel);
    Ok(wrap_image(DynamicImage::ImageRgba8(result)))
}

/// bi_image_histogram 获取图片直方图。
///
/// 用法：
///   imageHistogram(img) → [rHist, gHist, bHist, aHist]
///
/// 每个通道返回 256 元素的数组，值为该亮度值的像素数量。
fn bi_image_histogram(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "imageHistogram")?;
    let img = img_state.img.lock().unwrap();
    let rgba = img.to_rgba8();
    let mut hists = [[0u64; 256]; 4];
    for px in rgba.pixels() {
        hists[0][px[0] as usize] += 1;
        hists[1][px[1] as usize] += 1;
        hists[2][px[2] as usize] += 1;
        hists[3][px[3] as usize] += 1;
    }
    // 转为 4 个数组
    let channels: Vec<Value> = hists
        .iter()
        .map(|ch| {
            let arr: Vec<Value> = ch.iter().map(|&n| Value::Int(n as i64)).collect();
            Value::Array(Arc::new(Mutex::new(arr)))
        })
        .collect();
    Ok(Value::Array(Arc::new(Mutex::new(channels))))
}

// ============ Canvas 画布 ============

/// bi_canvas_new 创建新画布。
///
/// 用法：
///   canvasNew(width, height) → canvas           默认透明
///   canvasNew(width, height, bgColor) → canvas   指定背景色
fn bi_canvas_new(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let width = bh::as_int(args, 0, "canvasNew")? as u32;
    let height = bh::as_int(args, 1, "canvasNew")? as u32;
    if width == 0 || height == 0 {
        return Err(error_value("canvasNew() 宽度和高度不能为 0 (可能原因：参数计算错误)"));
    }
    let pixel = if args.len() > 2 {
        parse_color(&args[2], "canvasNew")?
    } else {
        Rgba([0, 0, 0, 0])
    };
    let buffer: RgbaImage = ImageBuffer::from_pixel(width, height, pixel);
    Ok(wrap_canvas(buffer))
}

/// bi_canvas_from_image 从图片创建画布（拷贝像素数据）。
///
/// 用法：
///   canvasFromImage(img) → canvas
fn bi_canvas_from_image(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let img_state = image_downcast(&args[0], "canvasFromImage")?;
    let img = img_state.img.lock().unwrap();
    let rgba = img.to_rgba8();
    Ok(wrap_canvas(rgba))
}

/// bi_canvas_to_image 画布转图片。
///
/// 用法：
///   canvasToImage(canvas) → image
fn bi_canvas_to_image(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasToImage")?;
    let buffer = canvas.buffer.lock().unwrap();
    Ok(wrap_image(DynamicImage::ImageRgba8(buffer.clone())))
}

/// bi_canvas_get_width 获取画布宽度。
fn bi_canvas_get_width(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasGetWidth")?;
    let buffer = canvas.buffer.lock().unwrap();
    Ok(Value::Int(buffer.width() as i64))
}

/// bi_canvas_get_height 获取画布高度。
fn bi_canvas_get_height(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasGetHeight")?;
    let buffer = canvas.buffer.lock().unwrap();
    Ok(Value::Int(buffer.height() as i64))
}

/// bi_canvas_get_pixel 获取画布像素颜色。
fn bi_canvas_get_pixel(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasGetPixel")?;
    let x = bh::as_int(args, 1, "canvasGetPixel")? as u32;
    let y = bh::as_int(args, 2, "canvasGetPixel")? as u32;
    let buffer = canvas.buffer.lock().unwrap();
    if x >= buffer.width() || y >= buffer.height() {
        return Err(error_value(format!(
            "canvasGetPixel() 坐标 ({}, {}) 超出画布范围 ({}x{}) (可能原因：坐标从 0 开始计算)",
            x, y, buffer.width(), buffer.height(),
        )));
    }
    let pixel = buffer.get_pixel(x, y);
    Ok(color_to_value(*pixel))
}

/// bi_canvas_set_pixel 设置画布像素颜色。
fn bi_canvas_set_pixel(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasSetPixel")?;
    let x = bh::as_int(args, 1, "canvasSetPixel")? as u32;
    let y = bh::as_int(args, 2, "canvasSetPixel")? as u32;
    let color = parse_color(&args[3], "canvasSetPixel")?;
    let mut buffer = canvas.buffer.lock().unwrap();
    if x >= buffer.width() || y >= buffer.height() {
        return Err(error_value(format!(
            "canvasSetPixel() 坐标 ({}, {}) 超出画布范围 ({}x{}) (可能原因：坐标从 0 开始计算)",
            x, y, buffer.width(), buffer.height(),
        )));
    }
    buffer.put_pixel(x, y, color);
    Ok(Value::Undefined)
}

/// bi_canvas_fill 用指定颜色填充整个画布。
fn bi_canvas_fill(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFill")?;
    let color = parse_color(&args[1], "canvasFill")?;
    let mut buffer = canvas.buffer.lock().unwrap();
    let (w, h) = (buffer.width(), buffer.height());
    for y in 0..h {
        for x in 0..w {
            buffer.put_pixel(x, y, color);
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_clear 清空画布。
///
/// 用法：
///   canvasClear(canvas)                 清为透明
///   canvasClear(canvas, color)          清为指定颜色
fn bi_canvas_clear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasClear")?;
    let color = if args.len() > 1 {
        parse_color(&args[1], "canvasClear")?
    } else {
        Rgba([0, 0, 0, 0])
    };
    let mut buffer = canvas.buffer.lock().unwrap();
    let (w, h) = (buffer.width(), buffer.height());
    for y in 0..h {
        for x in 0..w {
            buffer.put_pixel(x, y, color);
        }
    }
    Ok(Value::Undefined)
}

// ============ Canvas 绘图 ============

/// draw_pixel_safe 安全绘制像素（坐标越界自动跳过）。
fn draw_pixel_safe(buffer: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x >= 0 && (x as u32) < buffer.width() && y >= 0 && (y as u32) < buffer.height() {
        buffer.put_pixel(x as u32, y as u32, color);
    }
}

/// blend_pixel 混合像素（alpha 混合，用于文字抗锯齿）。
fn blend_pixel(buffer: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, alpha: f32) {
    if x < 0 || y < 0 || (x as u32) >= buffer.width() || (y as u32) >= buffer.height() {
        return;
    }
    let pixel = buffer.get_pixel_mut(x as u32, y as u32);
    let a = (alpha * color[3] as f32 / 255.0).clamp(0.0, 1.0);
    for i in 0..3 {
        pixel[i] = (pixel[i] as f32 * (1.0 - a) + color[i] as f32 * a) as u8;
    }
    pixel[3] = (pixel[3] as f32 * (1.0 - a) + 255.0 * a) as u8;
}

/// draw_line_raw 用 Bresenham 算法画线（不持锁，直接操作 buffer）。
fn draw_line_raw(buffer: &mut RgbaImage, x1: i32, y1: i32, x2: i32, y2: i32, color: Rgba<u8>) {
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut cx, mut cy) = (x1, y1);
    loop {
        draw_pixel_safe(buffer, cx, cy, color);
        if cx == x2 && cy == y2 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; cx += sx; }
        if e2 <= dx { err += dx; cy += sy; }
    }
}

/// draw_arc_quarter 画四分之一圆弧（用于圆角矩形）。
///
/// quadrant: 0=左上 1=右上 2=右下 3=左下
fn draw_arc_quarter(buffer: &mut RgbaImage, cx: i32, cy: i32, r: i32, quadrant: usize, color: Rgba<u8>) {
    if r <= 0 {
        draw_pixel_safe(buffer, cx, cy, color);
        return;
    }
    let mut x = r;
    let mut y = 0;
    let mut err = 1 - r;
    while x >= y {
        let (px, py) = match quadrant {
            0 => (cx - x, cy - y), // 左上
            1 => (cx + x, cy - y), // 右上
            2 => (cx + x, cy + y), // 右下
            3 => (cx - x, cy + y), // 左下
            _ => (cx, cy),
        };
        draw_pixel_safe(buffer, px, py, color);
        let (px2, py2) = match quadrant {
            0 => (cx - y, cy - x),
            1 => (cx + y, cy - x),
            2 => (cx + y, cy + x),
            3 => (cx - y, cy + x),
            _ => (cx, cy),
        };
        draw_pixel_safe(buffer, px2, py2, color);
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
}

/// bi_canvas_draw_line 画线（Bresenham 算法）。
///
/// 用法：
///   canvasDrawLine(canvas, x1, y1, x2, y2, color)
fn bi_canvas_draw_line(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawLine")?;
    let x1 = bh::as_int(args, 1, "canvasDrawLine")? as i32;
    let y1 = bh::as_int(args, 2, "canvasDrawLine")? as i32;
    let x2 = bh::as_int(args, 3, "canvasDrawLine")? as i32;
    let y2 = bh::as_int(args, 4, "canvasDrawLine")? as i32;
    let color = parse_color(&args[5], "canvasDrawLine")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    // Bresenham 画线算法
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut cx, mut cy) = (x1, y1);
    loop {
        draw_pixel_safe(&mut buffer, cx, cy, color);
        if cx == x2 && cy == y2 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; cx += sx; }
        if e2 <= dx { err += dx; cy += sy; }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_line_w 画带宽度的线（用圆刷模拟）。
///
/// 用法：
///   canvasDrawLineW(canvas, x1, y1, x2, y2, width, color)
fn bi_canvas_draw_line_w(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawLineW")?;
    let x1 = bh::as_int(args, 1, "canvasDrawLineW")? as i32;
    let y1 = bh::as_int(args, 2, "canvasDrawLineW")? as i32;
    let x2 = bh::as_int(args, 3, "canvasDrawLineW")? as i32;
    let y2 = bh::as_int(args, 4, "canvasDrawLineW")? as i32;
    let width = bh::as_int(args, 5, "canvasDrawLineW")? as i32;
    let color = parse_color(&args[6], "canvasDrawLineW")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    let r = (width / 2).max(0);
    // Bresenham 画线，每个点画半径 r 的填充圆
    let dx = (x2 - x1).abs();
    let dy = -(y2 - y1).abs();
    let sx = if x1 < x2 { 1 } else { -1 };
    let sy = if y1 < y2 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut cx, mut cy) = (x1, y1);
    let r_sq = r * r;
    loop {
        if r == 0 {
            draw_pixel_safe(&mut buffer, cx, cy, color);
        } else {
            for oy in -r..=r {
                for ox in -r..=r {
                    if ox * ox + oy * oy <= r_sq {
                        draw_pixel_safe(&mut buffer, cx + ox, cy + oy, color);
                    }
                }
            }
        }
        if cx == x2 && cy == y2 { break; }
        let e2 = 2 * err;
        if e2 >= dy { err += dy; cx += sx; }
        if e2 <= dx { err += dx; cy += sy; }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_rect 画矩形边框。
///
/// 用法：
///   canvasDrawRect(canvas, x, y, width, height, color)
fn bi_canvas_draw_rect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawRect")?;
    let x = bh::as_int(args, 1, "canvasDrawRect")? as i32;
    let y = bh::as_int(args, 2, "canvasDrawRect")? as i32;
    let w = bh::as_int(args, 3, "canvasDrawRect")? as i32;
    let h = bh::as_int(args, 4, "canvasDrawRect")? as i32;
    let color = parse_color(&args[5], "canvasDrawRect")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    // 画四条边
    for i in 0..w {
        draw_pixel_safe(&mut buffer, x + i, y, color);
        draw_pixel_safe(&mut buffer, x + i, y + h - 1, color);
    }
    for j in 0..h {
        draw_pixel_safe(&mut buffer, x, y + j, color);
        draw_pixel_safe(&mut buffer, x + w - 1, y + j, color);
    }
    Ok(Value::Undefined)
}

/// bi_canvas_fill_rect 填充矩形。
///
/// 用法：
///   canvasFillRect(canvas, x, y, width, height, color)
fn bi_canvas_fill_rect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFillRect")?;
    let x = bh::as_int(args, 1, "canvasFillRect")? as i32;
    let y = bh::as_int(args, 2, "canvasFillRect")? as i32;
    let w = bh::as_int(args, 3, "canvasFillRect")? as i32;
    let h = bh::as_int(args, 4, "canvasFillRect")? as i32;
    let color = parse_color(&args[5], "canvasFillRect")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    for j in 0..h {
        for i in 0..w {
            draw_pixel_safe(&mut buffer, x + i, y + j, color);
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_circle 画圆边框（中点画圆算法）。
///
/// 用法：
///   canvasDrawCircle(canvas, cx, cy, radius, color)
fn bi_canvas_draw_circle(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawCircle")?;
    let cx = bh::as_int(args, 1, "canvasDrawCircle")? as i32;
    let cy = bh::as_int(args, 2, "canvasDrawCircle")? as i32;
    let r = bh::as_int(args, 3, "canvasDrawCircle")? as i32;
    let color = parse_color(&args[4], "canvasDrawCircle")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    // 中点画圆算法
    let mut x = r;
    let mut y = 0;
    let mut err = 1 - r;
    while x >= y {
        draw_pixel_safe(&mut buffer, cx + x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx + x, cy - y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy - y, color);
        draw_pixel_safe(&mut buffer, cx + y, cy + x, color);
        draw_pixel_safe(&mut buffer, cx - y, cy + x, color);
        draw_pixel_safe(&mut buffer, cx + y, cy - x, color);
        draw_pixel_safe(&mut buffer, cx - y, cy - x, color);
        y += 1;
        if err < 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_fill_circle 填充圆。
///
/// 用法：
///   canvasFillCircle(canvas, cx, cy, radius, color)
fn bi_canvas_fill_circle(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFillCircle")?;
    let cx = bh::as_int(args, 1, "canvasFillCircle")? as i32;
    let cy = bh::as_int(args, 2, "canvasFillCircle")? as i32;
    let r = bh::as_int(args, 3, "canvasFillCircle")? as i32;
    let color = parse_color(&args[4], "canvasFillCircle")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    let r_sq = r * r;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r_sq {
                draw_pixel_safe(&mut buffer, cx + dx, cy + dy, color);
            }
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_ellipse 画椭圆边框。
///
/// 用法：
///   canvasDrawEllipse(canvas, cx, cy, rx, ry, color)
fn bi_canvas_draw_ellipse(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawEllipse")?;
    let cx = bh::as_int(args, 1, "canvasDrawEllipse")? as i32;
    let cy = bh::as_int(args, 2, "canvasDrawEllipse")? as i32;
    let rx = bh::as_int(args, 3, "canvasDrawEllipse")? as i32;
    let ry = bh::as_int(args, 4, "canvasDrawEllipse")? as i32;
    let color = parse_color(&args[5], "canvasDrawEllipse")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    if rx <= 0 || ry <= 0 {
        return Ok(Value::Undefined);
    }
    // 中点椭圆算法（分四象限）
    let mut x = 0i32;
    let mut y = ry;
    let rx2 = (rx * rx) as f64;
    let ry2 = (ry * ry) as f64;
    let mut d1 = ry2 - rx2 * ry as f64 + 0.25 * rx2;
    let mut dx_term = 2.0 * ry2 * x as f64;
    let mut dy_term = 2.0 * rx2 * y as f64;
    while dx_term < dy_term {
        draw_pixel_safe(&mut buffer, cx + x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx + x, cy - y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy - y, color);
        if d1 < 0.0 {
            x += 1;
            dx_term = 2.0 * ry2 * x as f64;
            d1 += dx_term + ry2;
        } else {
            x += 1;
            y -= 1;
            dx_term = 2.0 * ry2 * x as f64;
            dy_term = 2.0 * rx2 * y as f64;
            d1 += dx_term - dy_term + ry2;
        }
    }
    let mut d2 = ry2 * (x as f64 + 0.5).powi(2) + rx2 * (y as f64 - 1.0).powi(2) - rx2 * ry2;
    while y >= 0 {
        draw_pixel_safe(&mut buffer, cx + x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy + y, color);
        draw_pixel_safe(&mut buffer, cx + x, cy - y, color);
        draw_pixel_safe(&mut buffer, cx - x, cy - y, color);
        if d2 > 0.0 {
            y -= 1;
            dy_term = 2.0 * rx2 * y as f64;
            d2 += rx2 - dy_term;
        } else {
            y -= 1;
            x += 1;
            dx_term = 2.0 * ry2 * x as f64;
            dy_term = 2.0 * rx2 * y as f64;
            d2 += dx_term - dy_term + rx2;
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_fill_ellipse 填充椭圆。
///
/// 用法：
///   canvasFillEllipse(canvas, cx, cy, rx, ry, color)
fn bi_canvas_fill_ellipse(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFillEllipse")?;
    let cx = bh::as_int(args, 1, "canvasFillEllipse")? as i32;
    let cy = bh::as_int(args, 2, "canvasFillEllipse")? as i32;
    let rx = bh::as_int(args, 3, "canvasFillEllipse")? as i32;
    let ry = bh::as_int(args, 4, "canvasFillEllipse")? as i32;
    let color = parse_color(&args[5], "canvasFillEllipse")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    if rx <= 0 || ry <= 0 {
        return Ok(Value::Undefined);
    }
    let rx_f = rx as f64;
    let ry_f = ry as f64;
    for dy in -ry..=ry {
        for dx in -rx..=rx {
            let ex = (dx as f64) / rx_f;
            let ey = (dy as f64) / ry_f;
            if ex * ex + ey * ey <= 1.0 {
                draw_pixel_safe(&mut buffer, cx + dx, cy + dy, color);
            }
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_triangle 画三角形边框。
///
/// 用法：
///   canvasDrawTriangle(canvas, x1, y1, x2, y2, x3, y3, color)
fn bi_canvas_draw_triangle(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawTriangle")?;
    let x1 = bh::as_int(args, 1, "canvasDrawTriangle")? as i32;
    let y1 = bh::as_int(args, 2, "canvasDrawTriangle")? as i32;
    let x2 = bh::as_int(args, 3, "canvasDrawTriangle")? as i32;
    let y2 = bh::as_int(args, 4, "canvasDrawTriangle")? as i32;
    let x3 = bh::as_int(args, 5, "canvasDrawTriangle")? as i32;
    let y3 = bh::as_int(args, 6, "canvasDrawTriangle")? as i32;
    let color = parse_color(&args[7], "canvasDrawTriangle")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    draw_line_raw(&mut buffer, x1, y1, x2, y2, color);
    draw_line_raw(&mut buffer, x2, y2, x3, y3, color);
    draw_line_raw(&mut buffer, x3, y3, x1, y1, color);
    Ok(Value::Undefined)
}

/// bi_canvas_fill_triangle 填充三角形（重心坐标法）。
///
/// 用法：
///   canvasFillTriangle(canvas, x1, y1, x2, y2, x3, y3, color)
fn bi_canvas_fill_triangle(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFillTriangle")?;
    let x1 = bh::as_int(args, 1, "canvasFillTriangle")? as f32;
    let y1 = bh::as_int(args, 2, "canvasFillTriangle")? as f32;
    let x2 = bh::as_int(args, 3, "canvasFillTriangle")? as f32;
    let y2 = bh::as_int(args, 4, "canvasFillTriangle")? as f32;
    let x3 = bh::as_int(args, 5, "canvasFillTriangle")? as f32;
    let y3 = bh::as_int(args, 6, "canvasFillTriangle")? as f32;
    let color = parse_color(&args[7], "canvasFillTriangle")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    // 计算包围盒
    let min_x = x1.min(x2).min(x3).floor() as i32;
    let max_x = x1.max(x2).max(x3).ceil() as i32;
    let min_y = y1.min(y2).min(y3).floor() as i32;
    let max_y = y1.max(y2).max(y3).ceil() as i32;
    // 三角形面积
    let area = 0.5 * ((x2 - x1) * (y3 - y1) - (x3 - x1) * (y2 - y1));
    if area.abs() < 0.001 {
        return Ok(Value::Undefined);
    }
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let fx = px as f32;
            let fy = py as f32;
            // 重心坐标
            let w1 = 0.5 * ((x2 - fx) * (y3 - fy) - (x3 - fx) * (y2 - fy)) / area;
            let w2 = 0.5 * ((x3 - fx) * (y1 - fy) - (x1 - fx) * (y3 - fy)) / area;
            let w3 = 1.0 - w1 - w2;
            if w1 >= 0.0 && w2 >= 0.0 && w3 >= 0.0 {
                draw_pixel_safe(&mut buffer, px, py, color);
            }
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_round_rect 画圆角矩形边框。
///
/// 用法：
///   canvasDrawRoundRect(canvas, x, y, width, height, radius, color)
fn bi_canvas_draw_round_rect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawRoundRect")?;
    let x = bh::as_int(args, 1, "canvasDrawRoundRect")? as i32;
    let y = bh::as_int(args, 2, "canvasDrawRoundRect")? as i32;
    let w = bh::as_int(args, 3, "canvasDrawRoundRect")? as i32;
    let h = bh::as_int(args, 4, "canvasDrawRoundRect")? as i32;
    let r = bh::as_int(args, 5, "canvasDrawRoundRect")? as i32;
    let color = parse_color(&args[6], "canvasDrawRoundRect")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    let r = r.min(w / 2).min(h / 2).max(0);
    // 水平和垂直边
    draw_line_raw(&mut buffer, x + r, y, x + w - r, y, color);
    draw_line_raw(&mut buffer, x + r, y + h, x + w - r, y + h, color);
    draw_line_raw(&mut buffer, x, y + r, x, y + h - r, color);
    draw_line_raw(&mut buffer, x + w, y + r, x + w, y + h - r, color);
    // 四个角的圆弧
    draw_arc_quarter(&mut buffer, x + r, y + r, r, 0, color);
    draw_arc_quarter(&mut buffer, x + w - r, y + r, r, 1, color);
    draw_arc_quarter(&mut buffer, x + w - r, y + h - r, r, 2, color);
    draw_arc_quarter(&mut buffer, x + r, y + h - r, r, 3, color);
    Ok(Value::Undefined)
}

/// bi_canvas_fill_round_rect 填充圆角矩形。
///
/// 用法：
///   canvasFillRoundRect(canvas, x, y, width, height, radius, color)
fn bi_canvas_fill_round_rect(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasFillRoundRect")?;
    let x = bh::as_int(args, 1, "canvasFillRoundRect")? as i32;
    let y = bh::as_int(args, 2, "canvasFillRoundRect")? as i32;
    let w = bh::as_int(args, 3, "canvasFillRoundRect")? as i32;
    let h = bh::as_int(args, 4, "canvasFillRoundRect")? as i32;
    let r = bh::as_int(args, 5, "canvasFillRoundRect")? as i32;
    let color = parse_color(&args[6], "canvasFillRoundRect")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    let r = r.min(w / 2).min(h / 2).max(0);
    // 中间矩形区域
    for py in y + r..y + h - r {
        for px in x..x + w {
            draw_pixel_safe(&mut buffer, px, py, color);
        }
    }
    // 上下条带
    for py in y..y + r {
        for px in x + r..x + w - r {
            draw_pixel_safe(&mut buffer, px, py, color);
        }
    }
    for py in y + h - r..y + h {
        for px in x + r..x + w - r {
            draw_pixel_safe(&mut buffer, px, py, color);
        }
    }
    // 四角填充
    let r_sq = r * r;
    for dy in 0..=r {
        for dx in 0..=r {
            if dx * dx + dy * dy <= r_sq {
                // 左上
                draw_pixel_safe(&mut buffer, x + r - dx, y + r - dy, color);
                // 右上
                draw_pixel_safe(&mut buffer, x + w - r - 1 + dx, y + r - dy, color);
                // 左下
                draw_pixel_safe(&mut buffer, x + r - dx, y + h - r - 1 + dy, color);
                // 右下
                draw_pixel_safe(&mut buffer, x + w - r - 1 + dx, y + h - r - 1 + dy, color);
            }
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_gradient 画线性渐变矩形。
///
/// 用法：
///   canvasDrawGradient(canvas, x, y, w, h, color1, color2, direction)
///
/// direction: "h" 水平渐变, "v" 垂直渐变
fn bi_canvas_draw_gradient(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawGradient")?;
    let x = bh::as_int(args, 1, "canvasDrawGradient")? as i32;
    let y = bh::as_int(args, 2, "canvasDrawGradient")? as i32;
    let w = bh::as_int(args, 3, "canvasDrawGradient")? as i32;
    let h = bh::as_int(args, 4, "canvasDrawGradient")? as i32;
    let c1 = parse_color(&args[5], "canvasDrawGradient")?;
    let c2 = parse_color(&args[6], "canvasDrawGradient")?;
    let dir = bh::as_str(args, 7, "canvasDrawGradient")?;

    let mut buffer = canvas.buffer.lock().unwrap();
    let (steps, is_horizontal) = match dir {
        "h" | "horizontal" => (w, true),
        "v" | "vertical" => (h, false),
        other => return Err(error_value(format!(
            "canvasDrawGradient() direction 应为 \"h\" 或 \"v\"，得到 \"{}\" (可能原因：拼写错误)",
            other,
        ))),
    };
    if steps <= 0 {
        return Ok(Value::Undefined);
    }
    for i in 0..steps {
        let t = i as f32 / (steps - 1).max(1) as f32;
        let r = (c1[0] as f32 * (1.0 - t) + c2[0] as f32 * t) as u8;
        let g = (c1[1] as f32 * (1.0 - t) + c2[1] as f32 * t) as u8;
        let b = (c1[2] as f32 * (1.0 - t) + c2[2] as f32 * t) as u8;
        let a = (c1[3] as f32 * (1.0 - t) + c2[3] as f32 * t) as u8;
        let color = Rgba([r, g, b, a]);
        if is_horizontal {
            for py in 0..h {
                draw_pixel_safe(&mut buffer, x + i, y + py, color);
            }
        } else {
            for px in 0..w {
                draw_pixel_safe(&mut buffer, x + px, y + i, color);
            }
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_measure_text 测量文字尺寸（不实际绘制）。
///
/// 用法：
///   canvasMeasureText(text, fontSize?) → [width, height]
///   canvasMeasureText(text, fontSize, font) → [width, height]
fn bi_canvas_measure_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let text = bh::as_str(args, 0, "canvasMeasureText")?;
    let font_size = if args.len() > 1 {
        bh::as_int(args, 1, "canvasMeasureText")? as f32
    } else {
        16.0
    };

    // 获取字体
    let font_ref: &Font;
    let _owned_font;
    if args.len() > 2 {
        let font_state = font_downcast(&args[2], "canvasMeasureText")?;
        font_ref = &font_state.font;
    } else {
        match system_font_path() {
            Some(path) => {
                let font_data = std::fs::read(path).map_err(|e| {
                    error_value(format!(
                        "canvasMeasureText() 读取系统字体 '{}' 失败: {} (可能原因：字体文件不存在)",
                        path, e,
                    ))
                })?;
                let font = Font::try_from_vec(font_data).ok_or_else(|| {
                    error_value(format!(
                        "canvasMeasureText() 解析系统字体 '{}' 失败 (可能原因：字体文件损坏)",
                        path,
                    ))
                })?;
                _owned_font = font;
                font_ref = &_owned_font;
            }
            None => {
                return Err(error_value(
                    "canvasMeasureText() 未找到系统默认字体 (可能原因：系统未安装字体，请用 loadFont 加载字体后作为第 3 参数传入)",
                ));
            }
        }
    }

    let scale = Scale::uniform(font_size);
    let glyphs: Vec<_> = font_ref.layout(text, scale, Point { x: 0.0, y: font_size }).collect();
    let width = glyphs
        .last()
        .map(|g| g.position().x + g.unpositioned().h_metrics().advance_width)
        .unwrap_or(0.0);
    let height = font_size;
    let arr = vec![
        Value::Int(width.ceil() as i64),
        Value::Int(height.ceil() as i64),
    ];
    Ok(Value::Array(Arc::new(Mutex::new(arr))))
}

/// bi_canvas_draw_text 在画布上绘制文字。
///
/// 用法：
///   canvasDrawText(canvas, x, y, text, color, fontSize?) — 使用系统默认字体
///   canvasDrawText(canvas, x, y, text, color, fontSize, font) — 使用指定字体
///
/// fontSize 默认 16，font 为 loadFont 返回的对象
fn bi_canvas_draw_text(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawText")?;
    let x = bh::as_int(args, 1, "canvasDrawText")? as i32;
    let y = bh::as_int(args, 2, "canvasDrawText")? as i32;
    let text = bh::as_str(args, 3, "canvasDrawText")?.to_string();
    let color = parse_color(&args[4], "canvasDrawText")?;
    let font_size = if args.len() > 5 {
        bh::as_int(args, 5, "canvasDrawText")? as f32
    } else {
        16.0
    };

    // 获取字体：优先用参数指定的 font，否则尝试系统默认字体
    let font_ref: &Font;
    let _owned_font;
    if args.len() > 6 {
        let font_state = font_downcast(&args[6], "canvasDrawText")?;
        font_ref = &font_state.font;
    } else {
        // 尝试系统默认字体
        match system_font_path() {
            Some(path) => {
                let font_data = std::fs::read(path).map_err(|e| {
                    error_value(format!(
                        "canvasDrawText() 读取系统字体 '{}' 失败: {} (可能原因：字体文件不存在或无权限)",
                        path, e,
                    ))
                })?;
                let font = Font::try_from_vec(font_data).ok_or_else(|| {
                    error_value(format!(
                        "canvasDrawText() 解析系统字体 '{}' 失败 (可能原因：字体文件损坏或不支持的格式)",
                        path,
                    ))
                })?;
                _owned_font = font;
                font_ref = &_owned_font;
            }
            None => {
                return Err(error_value(
                    "canvasDrawText() 未找到系统默认字体 (可能原因：系统未安装字体，请用 loadFont 加载字体文件后作为第 7 参数传入)",
                ));
            }
        }
    }

    let mut buffer = canvas.buffer.lock().unwrap();
    let scale = Scale::uniform(font_size);
    let layout = font_ref.layout(&text, scale, Point { x: 0.0, y: font_size });

    for glyph in layout {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, coverage| {
                let px = x + bb.min.x + gx as i32;
                let py = y + bb.min.y + gy as i32;
                let alpha = coverage.min(1.0).max(0.0);
                if alpha > 0.01 {
                    blend_pixel(&mut buffer, px, py, color, alpha);
                }
            });
        }
    }
    Ok(Value::Undefined)
}

/// bi_canvas_draw_image 在画布上绘制图片。
///
/// 用法：
///   canvasDrawImage(canvas, img, x, y)
fn bi_canvas_draw_image(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let canvas = canvas_downcast(&args[0], "canvasDrawImage")?;
    let img_state = image_downcast(&args[1], "canvasDrawImage")?;
    let dx = bh::as_int(args, 2, "canvasDrawImage")? as i32;
    let dy = bh::as_int(args, 3, "canvasDrawImage")? as i32;

    let img = img_state.img.lock().unwrap();
    let src = img.to_rgba8();
    let mut buffer = canvas.buffer.lock().unwrap();

    for y in 0..src.height() {
        for x in 0..src.width() {
            let px = dx + x as i32;
            let py = dy + y as i32;
            let pixel = src.get_pixel(x, y);
            if pixel[3] > 0 {
                draw_pixel_safe(&mut buffer, px, py, *pixel);
            }
        }
    }
    Ok(Value::Undefined)
}

// ============ 颜色辅助 ============

/// bi_color_new 创建颜色数组。
///
/// 用法：
///   colorNew(r, g, b) → [r, g, b, 255]
///   colorNew(r, g, b, a) → [r, g, b, a]
fn bi_color_new(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let r = clamp_byte(&args[0], "colorNew", "r")?;
    let g = clamp_byte(&args[1], "colorNew", "g")?;
    let b = clamp_byte(&args[2], "colorNew", "b")?;
    let a = if args.len() > 3 {
        clamp_byte(&args[3], "colorNew", "a")?
    } else {
        255
    };
    Ok(color_to_value(Rgba([r, g, b, a])))
}

/// bi_color_from_hex 从十六进制字符串创建颜色。
///
/// 用法：
///   colorFromHex("#ff0000") → [255, 0, 0, 255]
///   colorFromHex("ff0000") → [255, 0, 0, 255]
///   colorFromHex("#ff000080") → [255, 0, 0, 128]
fn bi_color_from_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let hex = bh::as_str(args, 0, "colorFromHex")?;
    let s = hex.trim_start_matches('#');
    let parse_err = || error_value(format!(
        "colorFromHex() 无效的十六进制颜色: '{}' (可能原因：格式应为 #rrggbb 或 #rrggbbaa)",
        hex,
    ));
    let (r, g, b, a) = match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| parse_err())?;
            let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| parse_err())?;
            let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| parse_err())?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| parse_err())?;
            let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| parse_err())?;
            let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| parse_err())?;
            let a = u8::from_str_radix(&s[6..8], 16).map_err(|_| parse_err())?;
            (r, g, b, a)
        }
        _ => return Err(parse_err()),
    };
    Ok(color_to_value(Rgba([r, g, b, a])))
}

/// bi_color_to_hex 颜色转十六进制字符串。
///
/// 用法：
///   colorToHex([r, g, b]) → "#rrggbb"
///   colorToHex([r, g, b, a]) → "#rrggbbaa"
fn bi_color_to_hex(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let color = parse_color(&args[0], "colorToHex")?;
    let hex = format!("#{:02x}{:02x}{:02x}{:02x}", color[0], color[1], color[2], color[3]);
    Ok(Value::str_from(hex))
}

/// bi_color_blend 混合两种颜色。
///
/// 用法：
///   colorBlend(color1, color2, ratio) → [r, g, b, a]
///
/// ratio: 0.0=完全用color1, 1.0=完全用color2
fn bi_color_blend(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let c1 = parse_color(&args[0], "colorBlend")?;
    let c2 = parse_color(&args[1], "colorBlend")?;
    let ratio = bh::as_float(args, 2, "colorBlend")?;
    let t = ratio.clamp(0.0, 1.0) as f32;
    let r = (c1[0] as f32 * (1.0 - t) + c2[0] as f32 * t) as u8;
    let g = (c1[1] as f32 * (1.0 - t) + c2[1] as f32 * t) as u8;
    let b = (c1[2] as f32 * (1.0 - t) + c2[2] as f32 * t) as u8;
    let a = (c1[3] as f32 * (1.0 - t) + c2[3] as f32 * t) as u8;
    Ok(color_to_value(Rgba([r, g, b, a])))
}

// ============ 字体加载 ============

/// bi_load_font 从文件加载 TrueType/OpenType 字体。
///
/// 用法：
///   loadFont(fontPath) → font
///
/// 返回 font 对象，用于 canvasDrawText 第 7 参数。
fn bi_load_font(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let path = bh::as_str(args, 0, "loadFont")?;
    let font_data = std::fs::read(path).map_err(|e| {
        error_value(format!(
            "loadFont() 读取 '{}' 失败: {} (可能原因：文件不存在或无权限)",
            path, e,
        ))
    })?;
    let font = Font::try_from_vec(font_data).ok_or_else(|| {
        error_value(format!(
            "loadFont() 解析 '{}' 失败 (可能原因：文件不是有效的 TrueType/OpenType 字体)",
            path,
        ))
    })?;
    Ok(Value::Native(Arc::new(Arc::new(FontState { font }))))
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sflang;

    /// 辅助：评估 Sflang 表达式并返回结果
    fn eval(src: &str) -> Value {
        let mut sf = Sflang::new();
        let wrapped = format!("func __f() {{ {} }} var __r = __f()", src);
        sf.run_string(&wrapped).expect("eval failed");
        sf.get_global("__r").expect("__r not set")
    }

    #[test]
    fn test_color_new() {
        let v = eval("return colorNew(255, 128, 0)");
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g.len(), 4);
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(128));
                assert_eq!(g[2], Value::Int(0));
                assert_eq!(g[3], Value::Int(255)); // 默认 alpha
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_new_with_alpha() {
        let v = eval("return colorNew(255, 0, 0, 128)");
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[3], Value::Int(128));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_from_hex() {
        let v = eval("return colorFromHex(\"#ff0000\")");
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(0));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_from_hex_no_hash() {
        let v = eval("return colorFromHex(\"00ff00\")");
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[1], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_to_hex() {
        let v = eval("return colorToHex([255, 0, 0, 255])");
        match v {
            Value::Str(s) => assert_eq!(s.as_ref(), "#ff0000ff"),
            _ => panic!("应为字符串"),
        }
    }

    #[test]
    fn test_image_new_and_get_size() {
        let v = eval("img := imageNew(100, 50); return imageGetWidth(img)");
        assert_eq!(v, Value::Int(100));
    }

    #[test]
    fn test_image_new_height() {
        let v = eval("img := imageNew(100, 50); return imageGetHeight(img)");
        assert_eq!(v, Value::Int(50));
    }

    #[test]
    fn test_image_new_zero_error() {
        let mut sf = Sflang::new();
        let result = sf.run_string("img := imageNew(0, 50)");
        assert!(result.is_err(), "宽度为 0 应报错");
    }

    #[test]
    fn test_image_get_pixel() {
        let v = eval(r#"
            img := imageNew(10, 10, [255, 0, 0, 255])
            px := imageGetPixel(img, 5, 5)
            return px
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(0));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_set_pixel() {
        let v = eval(r#"
            img := imageNew(10, 10, [0, 0, 0, 255])
            imageSetPixel(img, 3, 4, [255, 255, 255, 255])
            return imageGetPixel(img, 3, 4)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(255));
                assert_eq!(g[2], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_resize() {
        let v = eval(r#"
            img := imageNew(100, 50, [255, 0, 0, 255])
            resized := imageResize(img, 50, 25)
            return imageGetWidth(resized)
        "#);
        assert_eq!(v, Value::Int(50));
    }

    #[test]
    fn test_image_flip() {
        // 翻转后尺寸不变
        let v = eval(r#"
            img := imageNew(100, 50, [255, 0, 0, 255])
            flipped := imageFlipH(img)
            return imageGetWidth(flipped)
        "#);
        assert_eq!(v, Value::Int(100));
    }

    #[test]
    fn test_image_gray() {
        // 灰度化后尺寸不变
        let v = eval(r#"
            img := imageNew(10, 10, [255, 0, 0, 255])
            gray := imageGray(img)
            return imageGetWidth(gray)
        "#);
        assert_eq!(v, Value::Int(10));
    }

    #[test]
    fn test_image_invert() {
        // 反色：白变黑
        let v = eval(r#"
            img := imageNew(10, 10, [255, 255, 255, 255])
            imageInvert(img)
            return imageGetPixel(img, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(0));
                assert_eq!(g[1], Value::Int(0));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_brightness() {
        // 调亮：黑变暗灰
        let v = eval(r#"
            img := imageNew(10, 10, [0, 0, 0, 255])
            bright := imageBrightness(img, 100)
            return imageGetPixel(bright, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                let r_val = match &g[0] { Value::Int(i) => *i, _ => 0 };
                assert!(r_val > 0, "调亮后 R 应 > 0");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_new() {
        let v = eval(r#"
            c := canvasNew(80, 60)
            return canvasGetWidth(c)
        "#);
        assert_eq!(v, Value::Int(80));
    }

    #[test]
    fn test_canvas_from_image() {
        let v = eval(r#"
            img := imageNew(100, 50, [255, 0, 0, 255])
            c := canvasFromImage(img)
            return canvasGetHeight(c)
        "#);
        assert_eq!(v, Value::Int(50));
    }

    #[test]
    fn test_canvas_to_image() {
        let v = eval(r#"
            c := canvasNew(30, 40, [0, 255, 0, 255])
            img := canvasToImage(c)
            return imageGetWidth(img)
        "#);
        assert_eq!(v, Value::Int(30));
    }

    #[test]
    fn test_canvas_set_pixel() {
        let v = eval(r#"
            c := canvasNew(10, 10, [0, 0, 0, 255])
            canvasSetPixel(c, 2, 3, [255, 255, 0, 255])
            return canvasGetPixel(c, 2, 3)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(255));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill() {
        let v = eval(r#"
            c := canvasNew(10, 10, [0, 0, 0, 255])
            canvasFill(c, [100, 150, 200, 255])
            return canvasGetPixel(c, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(100));
                assert_eq!(g[1], Value::Int(150));
                assert_eq!(g[2], Value::Int(200));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_line() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawLine(c, 0, 0, 19, 19, [255, 255, 255, 255])
            return canvasGetPixel(c, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_rect() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawRect(c, 2, 2, 6, 6, [255, 0, 0, 255])
            return canvasGetPixel(c, 2, 2)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill_rect() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasFillRect(c, 2, 2, 6, 6, [0, 255, 0, 255])
            return canvasGetPixel(c, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[1], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_circle() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawCircle(c, 10, 10, 5, [255, 0, 0, 255])
            return canvasGetPixel(c, 15, 10)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill_circle() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasFillCircle(c, 10, 10, 5, [0, 0, 255, 255])
            return canvasGetPixel(c, 10, 10)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[2], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_image() {
        let v = eval(r#"
            img := imageNew(5, 5, [255, 0, 0, 255])
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawImage(c, img, 3, 3)
            return canvasGetPixel(c, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_load_from_bytes_png() {
        // 先创建一个 PNG 图片的字节数据
        let img = DynamicImage::ImageRgba8(
            ImageBuffer::from_pixel(4, 4, Rgba([255, 0, 0, 255]))
        );
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let png_bytes = buf.into_inner();

        let mut sf = Sflang::new();
        sf.set_global("__testBytes", Value::Bytes(Arc::new(png_bytes)));
        let v = sf.run_string(r#"
            img := imageLoadFromBytes(__testBytes, "png")
            return imageGetPixel(img, 0, 0)
        "#);
        assert!(v.is_ok(), "加载 PNG 字节失败: {:?}", v);
        let val = v.unwrap();
        match val {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(0));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_save_to_bytes_png() {
        let v = eval(r#"
            img := imageNew(4, 4, [0, 255, 0, 255])
            data := imageSaveToBytes(img, "png")
            return len(data)
        "#);
        // PNG 字节数应 > 0
        assert_ne!(v, Value::Int(0));
    }

    #[test]
    fn test_image_rotate_90() {
        // 旋转 90 度后宽高互换
        let v = eval(r#"
            img := imageNew(100, 50, [255, 0, 0, 255])
            rotated := imageRotate(img, 90)
            return imageGetWidth(rotated)
        "#);
        assert_eq!(v, Value::Int(50));
    }

    #[test]
    fn test_image_crop() {
        let v = eval(r#"
            img := imageNew(100, 100, [255, 0, 0, 255])
            cropped := imageCrop(img, 10, 10, 30, 40)
            return imageGetWidth(cropped)
        "#);
        assert_eq!(v, Value::Int(30));
    }

    // ============ 新增功能测试 ============

    #[test]
    fn test_image_clone() {
        let v = eval(r#"
            img := imageNew(10, 10, [100, 200, 50, 255])
            clone := imageClone(img)
            imageSetPixel(clone, 0, 0, [0, 0, 0, 255])
            return imageGetPixel(img, 0, 0)
        "#);
        // 修改克隆不应影响原图
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(100));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_blur() {
        let v = eval(r#"
            img := imageNew(20, 20, [255, 0, 0, 255])
            blurred := imageBlur(img, 2.0)
            return imageGetWidth(blurred)
        "#);
        assert_eq!(v, Value::Int(20));
    }

    #[test]
    fn test_image_sharpen() {
        let v = eval(r#"
            img := imageNew(20, 20, [128, 128, 128, 255])
            sharp := imageSharpen(img)
            return imageGetWidth(sharp)
        "#);
        assert_eq!(v, Value::Int(20));
    }

    #[test]
    fn test_image_gamma() {
        // gamma=2.2 时，纯黑不变
        let v = eval(r#"
            img := imageNew(10, 10, [0, 0, 0, 255])
            result := imageGamma(img, 2.2)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_gamma_brighten() {
        // gamma < 1 时白色不变
        let v = eval(r#"
            img := imageNew(10, 10, [255, 255, 255, 255])
            result := imageGamma(img, 0.5)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_sepia() {
        let v = eval(r#"
            img := imageNew(10, 10, [255, 255, 255, 255])
            result := imageSepia(img)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 白色 sepia 后应偏黄
                let r_val = match &g[0] { Value::Int(i) => *i, _ => 0 };
                let g_val = match &g[1] { Value::Int(i) => *i, _ => 0 };
                let b_val = match &g[2] { Value::Int(i) => *i, _ => 0 };
                assert!(r_val >= g_val, "R 应 >= G");
                assert!(g_val >= b_val, "G 应 >= B");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_threshold() {
        let v = eval(r#"
            img := imageNew(10, 10, [100, 100, 100, 255])
            result := imageThreshold(img, 127)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 100 < 127 应变黑
                assert_eq!(g[0], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_threshold_high() {
        let v = eval(r#"
            img := imageNew(10, 10, [200, 200, 200, 255])
            result := imageThreshold(img, 127)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 200 > 127 应变白
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_tint() {
        let v = eval(r#"
            img := imageNew(10, 10, [255, 255, 255, 255])
            result := imageTint(img, [255, 0, 0, 255])
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 白色 tint 红色后应全红
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[1], Value::Int(0));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_opacity() {
        let v = eval(r#"
            img := imageNew(10, 10, [255, 0, 0, 255])
            result := imageOpacity(img, 0.5)
            return imageGetPixel(result, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                let a_val = match &g[3] { Value::Int(i) => *i, _ => 0 };
                assert_eq!(a_val, 127, "半透明后 alpha 应为 127");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_edge_detect() {
        let v = eval(r#"
            img := imageNew(10, 10, [255, 255, 255, 255])
            result := imageEdgeDetect(img)
            return imageGetWidth(result)
        "#);
        assert_eq!(v, Value::Int(10));
    }

    #[test]
    fn test_image_convolve3x3() {
        // 用单位卷积核（中心1，其余0），结果应与原图一致
        let v = eval(r#"
            img := imageNew(10, 10, [100, 150, 200, 255])
            result := imageConvolve3x3(img, [0, 0, 0, 0, 1, 0, 0, 0, 0])
            return imageGetPixel(result, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(100));
                assert_eq!(g[1], Value::Int(150));
                assert_eq!(g[2], Value::Int(200));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_image_histogram() {
        let v = eval(r#"
            img := imageNew(4, 4, [100, 200, 50, 255])
            hist := imageHistogram(img)
            return len(hist)
        "#);
        // 应返回 4 个通道的直方图
        assert_eq!(v, Value::Int(4));
    }

    #[test]
    fn test_image_histogram_values() {
        // 4x4 纯色图，R=100，直方图 hist[0][100] 应为 16
        let v = eval(r#"
            img := imageNew(4, 4, [100, 200, 50, 255])
            hist := imageHistogram(img)
            return hist[0][100]
        "#);
        assert_eq!(v, Value::Int(16));
    }

    #[test]
    fn test_image_rotate_free() {
        let v = eval(r#"
            img := imageNew(100, 50, [255, 0, 0, 255])
            rotated := imageRotateFree(img, 45)
            return imageGetWidth(rotated)
        "#);
        // 旋转 45 度后画布应变大
        let w = match v { Value::Int(i) => i, _ => 0 };
        assert!(w >= 100, "旋转 45 度后宽度应 >= 100");
    }

    #[test]
    fn test_image_blend() {
        let v = eval(r#"
            base := imageNew(20, 20, [0, 0, 0, 255])
            overlay := imageNew(10, 10, [255, 0, 0, 255])
            result := imageBlend(base, overlay, 5, 5)
            return imageGetPixel(result, 10, 10)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_clear() {
        let v = eval(r#"
            c := canvasNew(10, 10, [255, 0, 0, 255])
            canvasClear(c)
            return canvasGetPixel(c, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[3], Value::Int(0), "清空后 alpha 应为 0");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_clear_with_color() {
        let v = eval(r#"
            c := canvasNew(10, 10, [255, 0, 0, 255])
            canvasClear(c, [0, 0, 255, 255])
            return canvasGetPixel(c, 5, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[2], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_line_w() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawLineW(c, 0, 10, 19, 10, 3, [255, 255, 255, 255])
            return canvasGetPixel(c, 10, 10)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_ellipse() {
        let v = eval(r#"
            c := canvasNew(30, 30, [0, 0, 0, 255])
            canvasDrawEllipse(c, 15, 15, 10, 5, [255, 0, 0, 255])
            return canvasGetPixel(c, 25, 15)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill_ellipse() {
        let v = eval(r#"
            c := canvasNew(30, 30, [0, 0, 0, 255])
            canvasFillEllipse(c, 15, 15, 10, 5, [0, 255, 0, 255])
            return canvasGetPixel(c, 15, 15)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[1], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_triangle() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawTriangle(c, 0, 0, 19, 0, 10, 19, [255, 0, 0, 255])
            return canvasGetPixel(c, 0, 0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill_triangle() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasFillTriangle(c, 0, 0, 19, 0, 10, 19, [0, 0, 255, 255])
            return canvasGetPixel(c, 10, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[2], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_round_rect() {
        let v = eval(r#"
            c := canvasNew(30, 30, [0, 0, 0, 255])
            canvasDrawRoundRect(c, 5, 5, 20, 20, 5, [255, 0, 0, 255])
            return canvasGetPixel(c, 15, 5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_fill_round_rect() {
        let v = eval(r#"
            c := canvasNew(30, 30, [0, 0, 0, 255])
            canvasFillRoundRect(c, 5, 5, 20, 20, 5, [0, 255, 0, 255])
            return canvasGetPixel(c, 15, 15)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[1], Value::Int(255));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_gradient_h() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawGradient(c, 0, 0, 20, 20, [255, 0, 0, 255], [0, 0, 255, 255], "h")
            return canvasGetPixel(c, 0, 10)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 左端应偏红
                let r_val = match &g[0] { Value::Int(i) => *i, _ => 0 };
                let b_val = match &g[2] { Value::Int(i) => *i, _ => 0 };
                assert!(r_val > b_val, "左端 R 应 > B");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_draw_gradient_v() {
        let v = eval(r#"
            c := canvasNew(20, 20, [0, 0, 0, 255])
            canvasDrawGradient(c, 0, 0, 20, 20, [255, 0, 0, 255], [0, 0, 255, 255], "v")
            return canvasGetPixel(c, 10, 19)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                // 底端应偏蓝
                let r_val = match &g[0] { Value::Int(i) => *i, _ => 0 };
                let b_val = match &g[2] { Value::Int(i) => *i, _ => 0 };
                assert!(b_val > r_val, "底端 B 应 > R");
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_blend() {
        // 红蓝 50% 混合应为 [128, 0, 128]
        let v = eval(r#"
            return colorBlend([255, 0, 0, 255], [0, 0, 255, 255], 0.5)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                let r_val = match &g[0] { Value::Int(i) => *i, _ => 0 };
                let b_val = match &g[2] { Value::Int(i) => *i, _ => 0 };
                assert!((r_val - 128).abs() <= 1, "R 应约为 128，得到 {}", r_val);
                assert!((b_val - 128).abs() <= 1, "B 应约为 128，得到 {}", b_val);
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_color_blend_zero() {
        // ratio=0 时应完全用 color1
        let v = eval(r#"
            return colorBlend([255, 0, 0, 255], [0, 0, 255, 255], 0.0)
        "#);
        match v {
            Value::Array(a) => {
                let g = a.lock().unwrap();
                assert_eq!(g[0], Value::Int(255));
                assert_eq!(g[2], Value::Int(0));
            }
            _ => panic!("应为数组"),
        }
    }

    #[test]
    fn test_canvas_measure_text() {
        let v = eval(r#"
            size := canvasMeasureText("Hello", 20)
            return size[0]
        "#);
        // 宽度应 > 0
        let w = match v { Value::Int(i) => i, _ => 0 };
        assert!(w > 0, "文字宽度应 > 0");
    }
}
