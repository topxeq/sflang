//! builtins_image_gen.rs — 程序化图片生成
//!
//! 设计要点：
//!   - 所有算法基于公开数学公式与经典算法自行实现，无版权问题
//!   - 提供 PRNG（XorShift64*）保证种子可重现
//!   - 实现 2D Perlin 噪声（用于云彩、地形、大理石等）
//!   - 实现 Worley/Voronoi 噪声（用于细胞图案、纹理）
//!   - 提供丰富的图案生成器，可组合（返回 image 对象，可用 imageBlend 等组合）
//!
//! 函数列表：
//!   基础噪声：
//!     imageGenNoise(width, height, seed?) → image
//!     imageGenPerlin(width, height, scale?, seed?) → image
//!     imageGenFBM(width, height, octaves?, scale?, seed?) → image
//!     imageGenWorley(width, height, cellSize?, seed?) → image
//!     imageGenVoronoi(width, height, count?, seed?) → image
//!
//!   纹理：
//!     imageGenPlasma(width, height, scale?, seed?) → image
//!     imageGenMarble(width, height, scale?, seed?) → image
//!     imageGenWood(width, height, scale?, seed?) → image
//!     imageGenClouds(width, height, scale?, seed?) → image
//!
//!   渐变与图案：
//!     imageGenLinear(w, h, c1, c2, direction?) → image
//!     imageGenRadial(w, h, cx?, cy?, c1, c2) → image
//!     imageGenConic(w, h, cx?, cy?, c1, c2) → image
//!     imageGenChecker(w, h, cellSize?, c1?, c2?) → image
//!     imageGenStars(w, h, count?, seed?) → image
//!     imageGenMaze(w, h, cellSize?, seed?) → image
//!     imageGenCircles(w, h, count?, seed?) → image
//!     imageGenLines(w, h, count?, seed?) → image
//!     imageGenParticles(w, h, count?, seed?) → image
//!
//!   分形：
//!     imageGenMandelbrot(w, h, maxIter?, zoom?, cx?, cy?) → image
//!     imageGenJulia(w, h, cr, ci, maxIter?, zoom?) → image

use image::{DynamicImage, Rgba, RgbaImage};

use crate::builtins_helpers as bh;
use crate::builtins_image::{parse_color, wrap_image};
use crate::value::{error_value, Value};
use crate::vm::VM;

/// register 注册所有图片生成内置函数。
pub fn register(vm: &mut VM) {
    // 基础噪声
    vm.register_builtin("imageGenNoise", bi_image_gen_noise);
    vm.register_builtin("imageGenPerlin", bi_image_gen_perlin);
    vm.register_builtin("imageGenFBM", bi_image_gen_fbm);
    vm.register_builtin("imageGenWorley", bi_image_gen_worley);
    vm.register_builtin("imageGenVoronoi", bi_image_gen_voronoi);

    // 纹理
    vm.register_builtin("imageGenPlasma", bi_image_gen_plasma);
    vm.register_builtin("imageGenMarble", bi_image_gen_marble);
    vm.register_builtin("imageGenWood", bi_image_gen_wood);
    vm.register_builtin("imageGenClouds", bi_image_gen_clouds);

    // 渐变与图案
    vm.register_builtin("imageGenLinear", bi_image_gen_linear);
    vm.register_builtin("imageGenRadial", bi_image_gen_radial);
    vm.register_builtin("imageGenConic", bi_image_gen_conic);
    vm.register_builtin("imageGenChecker", bi_image_gen_checker);
    vm.register_builtin("imageGenStars", bi_image_gen_stars);
    vm.register_builtin("imageGenMaze", bi_image_gen_maze);
    vm.register_builtin("imageGenCircles", bi_image_gen_circles);
    vm.register_builtin("imageGenLines", bi_image_gen_lines);
    vm.register_builtin("imageGenParticles", bi_image_gen_particles);

    // 分形
    vm.register_builtin("imageGenMandelbrot", bi_image_gen_mandelbrot);
    vm.register_builtin("imageGenJulia", bi_image_gen_julia);
}

// ============ PRNG：XorShift64* ============

/// Rng 简单高效的伪随机数生成器（XorShift64*）。
///
/// 种子可重现，跨平台一致，适用于图片生成。
/// 周期为 2^64 - 1，足够图片生成使用。
struct Rng {
    state: u64,
}

impl Rng {
    /// new 创建 PRNG，种子为 0 时使用默认种子。
    fn new(seed: u64) -> Self {
        // 避免全 0 状态
        let s = if seed == 0 { 0x9E3779B97F4A7C15 } else { seed };
        Self { state: s }
    }

    /// next_u64 返回下一个 64 位随机数。
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// next_f64 返回 [0, 1) 范围的浮点数。
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// range_i64 返回 [min, max) 范围内的整数。
    fn range_i64(&mut self, min: i64, max: i64) -> i64 {
        if max <= min {
            return min;
        }
        let range = (max - min) as u64;
        min + (self.next_u64() % range) as i64
    }

    /// range_f64 返回 [min, max) 范围内的浮点数。
    fn range_f64(&mut self, min: f64, max: f64) -> f64 {
        min + self.next_f64() * (max - min)
    }
}

// ============ Perlin 噪声 ============

/// Perlin 2D Perlin 噪声生成器。
///
/// 基于 Ken Perlin 提出的改进版噪声算法（公共算法）。
/// 使用 256 个预生成的梯度向量和排列表，保证平滑可重现。
struct Perlin {
    /// perm 排列表（512 个，前 256 重复一次便于索引）。
    perm: [u8; 512],
    /// grads 梯度向量表（256 个 2D 向量）。
    grads: [(f64, f64); 256],
}

impl Perlin {
    /// new 用种子初始化 Perlin 噪声生成器。
    fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        // 生成 256 个梯度向量（单位圆上均匀分布，加少量扰动避免规律性）
        let mut grads = [(0.0, 0.0); 256];
        for i in 0..256 {
            let angle = rng.next_f64() * std::f64::consts::TAU;
            grads[i] = (angle.cos(), angle.sin());
        }
        // 生成排列表（Fisher-Yates 洗牌）
        let mut perm = [0u8; 512];
        let mut p = [0u8; 256];
        for i in 0..256 {
            p[i] = i as u8;
        }
        for i in (1..256).rev() {
            let j = rng.range_i64(0, (i + 1) as i64) as usize;
            p.swap(i, j);
        }
        for i in 0..512 {
            perm[i] = p[i & 255];
        }
        Self { perm, grads }
    }

    /// fade 平滑插值曲线（6t^5 - 15t^4 + 10t^3）。
    #[inline]
    fn fade(t: f64) -> f64 {
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    /// lerp 线性插值。
    #[inline]
    fn lerp(a: f64, b: f64, t: f64) -> f64 {
        a + t * (b - a)
    }

    /// grad 计算网格点处的梯度贡献。
    #[inline]
    fn grad(&self, hash: u8, x: f64, y: f64) -> f64 {
        let (gx, gy) = self.grads[hash as usize];
        gx * x + gy * y
    }

    /// noise_2d 计算 2D Perlin 噪声值，返回 [-1, 1]。
    fn noise_2d(&self, x: f64, y: f64) -> f64 {
        let xi = x.floor() as i32 & 255;
        let yi = y.floor() as i32 & 255;
        let xf = x - x.floor();
        let yf = y - y.floor();
        let u = Self::fade(xf);
        let v = Self::fade(yf);
        let aa = self.perm[(self.perm[xi as usize] as usize + yi as usize) & 511];
        let ab = self.perm[(self.perm[xi as usize] as usize + (yi + 1) as usize) & 511];
        let ba = self.perm[(self.perm[(xi + 1) as usize] as usize + yi as usize) & 511];
        let bb = self.perm[(self.perm[(xi + 1) as usize] as usize + (yi + 1) as usize) & 511];
        let x1 = Self::lerp(
            self.grad(aa, xf, yf),
            self.grad(ba, xf - 1.0, yf),
            u,
        );
        let x2 = Self::lerp(
            self.grad(ab, xf, yf - 1.0),
            self.grad(bb, xf - 1.0, yf - 1.0),
            u,
        );
        // 结果约在 [-1, 1]，加 0.5 后映射到 [0, 1]
        Self::lerp(x1, x2, v) * 0.5 + 0.5
    }

    /// fbm 分形布朗运动（多个倍频叠加 Perlin 噪声）。
    ///
    /// 返回 [0, 1] 范围的值，octaves 越多细节越丰富。
    fn fbm(&self, x: f64, y: f64, octaves: u32, persistence: f64, lacunarity: f64) -> f64 {
        let mut total = 0.0;
        let mut frequency = 1.0;
        let mut amplitude = 1.0;
        let mut max_value = 0.0;
        for _ in 0..octaves {
            total += self.noise_2d(x * frequency, y * frequency) * amplitude;
            max_value += amplitude;
            amplitude *= persistence;
            frequency *= lacunarity;
        }
        (total / max_value).clamp(0.0, 1.0)
    }
}

// ============ 辅助函数 ============

/// value_to_gray 将 [0, 1] 值映射到灰度字节。
#[inline]
fn value_to_gray(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// hsl_to_rgb 将 HSL（h: 0-360, s/l: 0-1）转 RGB。
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    if s == 0.0 {
        let g = (l * 255.0).round() as u8;
        return (g, g, g);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let h = h / 360.0;
    let hue_to_rgb = |t: f64| {
        let t = if t < 0.0 { t + 1.0 } else if t > 1.0 { t - 1.0 } else { t };
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let r = hue_to_rgb(h + 1.0 / 3.0);
    let g = hue_to_rgb(h);
    let b = hue_to_rgb(h - 1.0 / 3.0);
    ((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
}

/// lerp_color 在两个 RGBA 颜色之间线性插值。
#[inline]
fn lerp_color(c1: Rgba<u8>, c2: Rgba<u8>, t: f64) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    Rgba([
        (c1[0] as f64 * (1.0 - t) + c2[0] as f64 * t).round() as u8,
        (c1[1] as f64 * (1.0 - t) + c2[1] as f64 * t).round() as u8,
        (c1[2] as f64 * (1.0 - t) + c2[2] as f64 * t).round() as u8,
        (c1[3] as f64 * (1.0 - t) + c2[3] as f64 * t).round() as u8,
    ])
}

/// opt_seed 从可选参数中提取种子，缺省时用系统时间。
fn opt_seed(args: &[Value], idx: usize, _fn_name: &str) -> u64 {
    if let Some(v) = args.get(idx) {
        match v {
            Value::Int(n) => *n as u64,
            Value::Float(f) => *f as u64,
            Value::Undefined => default_seed(),
            _ => 0,
        }
    } else {
        default_seed()
    }
}

/// default_seed 用系统时间作为默认种子。
fn default_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x12345678)
}

/// opt_int 提取可选整数参数，缺省返回默认值。
fn opt_int(args: &[Value], idx: usize, default: i64, _fn_name: &str) -> i64 {
    if let Some(v) = args.get(idx) {
        match v {
            Value::Int(n) => *n,
            Value::Float(f) => *f as i64,
            _ => default,
        }
    } else {
        default
    }
}

/// opt_float 提取可选浮点参数，缺省返回默认值。
fn opt_float(args: &[Value], idx: usize, default: f64) -> f64 {
    if let Some(v) = args.get(idx) {
        match v {
            Value::Int(n) => *n as f64,
            Value::Float(f) => *f,
            _ => default,
        }
    } else {
        default
    }
}

// ============ 基础噪声生成器 ============

/// bi_image_gen_noise 生成白噪声图片。
///
/// 用法：
///   imageGenNoise(width, height) → image
///   imageGenNoise(width, height, seed) → image
fn bi_image_gen_noise(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenNoise")? as u32;
    let h = bh::as_int(args, 1, "imageGenNoise")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenNoise() width 和 height 必须 > 0"));
    }
    let seed = opt_seed(args, 2, "imageGenNoise");
    let mut rng = Rng::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = rng.next_f64();
            let g = value_to_gray(v);
            img.put_pixel(x, y, Rgba([g, g, g, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_perlin 生成 Perlin 噪声图片。
///
/// 用法：
///   imageGenPerlin(width, height) → image
///   imageGenPerlin(width, height, scale) → image
///   imageGenPerlin(width, height, scale, seed) → image
///
/// scale 控制噪声粒度，越大越粗糙（默认 0.05）
fn bi_image_gen_perlin(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenPerlin")? as u32;
    let h = bh::as_int(args, 1, "imageGenPerlin")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenPerlin() width 和 height 必须 > 0"));
    }
    let scale = opt_float(args, 2, 0.05);
    let seed = opt_seed(args, 3, "imageGenPerlin");
    if scale <= 0.0 {
        return Err(error_value(format!(
            "imageGenPerlin() scale 必须 > 0，得到 {}",
            scale,
        )));
    }
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = perlin.noise_2d(x as f64 * scale, y as f64 * scale);
            let g = value_to_gray(v);
            img.put_pixel(x, y, Rgba([g, g, g, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_fbm 生成分形布朗运动（FBM）噪声图片。
///
/// 用法：
///   imageGenFBM(width, height) → image
///   imageGenFBM(width, height, octaves, scale, seed) → image
///
/// octaves 控制细节层次（默认 4），scale 控制粒度（默认 0.05）
fn bi_image_gen_fbm(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenFBM")? as u32;
    let h = bh::as_int(args, 1, "imageGenFBM")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenFBM() width 和 height 必须 > 0"));
    }
    let octaves = opt_int(args, 2, 4, "imageGenFBM").max(1).min(8) as u32;
    let scale = opt_float(args, 3, 0.05);
    let seed = opt_seed(args, 4, "imageGenFBM");
    if scale <= 0.0 {
        return Err(error_value(format!(
            "imageGenFBM() scale 必须 > 0，得到 {}",
            scale,
        )));
    }
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = perlin.fbm(x as f64 * scale, y as f64 * scale, octaves, 0.5, 2.0);
            let g = value_to_gray(v);
            img.put_pixel(x, y, Rgba([g, g, g, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_worley 生成 Worley 噪声图片（细胞图案）。
///
/// 用法：
///   imageGenWorley(width, height) → image
///   imageGenWorley(width, height, cellSize, seed) → image
///
/// cellSize 控制特征点密度（默认 32）
fn bi_image_gen_worley(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenWorley")? as u32;
    let h = bh::as_int(args, 1, "imageGenWorley")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenWorley() width 和 height 必须 > 0"));
    }
    let cell_size = opt_int(args, 2, 32, "imageGenWorley").max(4) as u32;
    let seed = opt_seed(args, 3, "imageGenWorley");
    let mut rng = Rng::new(seed);
    // 在每个网格放置一个特征点（带随机偏移）
    let cols = (w + cell_size - 1) / cell_size;
    let rows = (h + cell_size - 1) / cell_size;
    let mut points: Vec<(f64, f64)> = Vec::with_capacity((cols * rows) as usize);
    for ry in 0..rows + 1 {
        for rx in 0..cols + 1 {
            let px = rx as f64 * cell_size as f64 + rng.next_f64() * cell_size as f64;
            let py = ry as f64 * cell_size as f64 + rng.next_f64() * cell_size as f64;
            points.push((px, py));
        }
    }
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let px = x as f64;
            let py = y as f64;
            // 找到最近特征点距离
            let mut min_dist = f64::MAX;
            let grid_x = x / cell_size;
            let grid_y = y / cell_size;
            // 只检查周围 3x3 网格
            for dy in 0..=1 {
                for dx in 0..=1 {
                    let gx = grid_x + dx;
                    let gy = grid_y + dy;
                    if gx <= cols && gy <= rows {
                        let idx = (gy * (cols + 1) + gx) as usize;
                        if idx < points.len() {
                            let (fx, fy) = points[idx];
                            let d = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
                            if d < min_dist {
                                min_dist = d;
                            }
                        }
                    }
                }
            }
            // 距离映射到灰度
            let v = (min_dist / (cell_size as f64 * 0.7)).clamp(0.0, 1.0);
            let g = value_to_gray(1.0 - v);
            img.put_pixel(x, y, Rgba([g, g, g, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_voronoi 生成 Voronoi 图（彩色细胞）。
///
/// 用法：
///   imageGenVoronoi(width, height) → image
///   imageGenVoronoi(width, height, count, seed) → image
///
/// count 控制特征点数量（默认 50）
fn bi_image_gen_voronoi(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenVoronoi")? as u32;
    let h = bh::as_int(args, 1, "imageGenVoronoi")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenVoronoi() width 和 height 必须 > 0"));
    }
    let count = opt_int(args, 2, 50, "imageGenVoronoi").max(2) as usize;
    let seed = opt_seed(args, 3, "imageGenVoronoi");
    let mut rng = Rng::new(seed);
    // 生成特征点与各自颜色
    let mut points: Vec<(f64, f64, Rgba<u8>)> = Vec::with_capacity(count);
    for _ in 0..count {
        let px = rng.next_f64() * w as f64;
        let py = rng.next_f64() * h as f64;
        let hue = rng.next_f64() * 360.0;
        let (r, g, b) = hsl_to_rgb(hue, 0.7, 0.5);
        points.push((px, py, Rgba([r, g, b, 255])));
    }
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let px = x as f64;
            let py = y as f64;
            let mut min_dist = f64::MAX;
            let mut nearest_color = Rgba([0, 0, 0, 255]);
            for (fx, fy, c) in &points {
                let d = (fx - px).powi(2) + (fy - py).powi(2);
                if d < min_dist {
                    min_dist = d;
                    nearest_color = *c;
                }
            }
            img.put_pixel(x, y, nearest_color);
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

// ============ 纹理生成器 ============

/// bi_image_gen_plasma 生成等离子体效果。
///
/// 用法：
///   imageGenPlasma(width, height) → image
///   imageGenPlasma(width, height, scale, seed) → image
fn bi_image_gen_plasma(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenPlasma")? as u32;
    let h = bh::as_int(args, 1, "imageGenPlasma")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenPlasma() width 和 height 必须 > 0"));
    }
    let scale = opt_float(args, 2, 0.05);
    let seed = opt_seed(args, 3, "imageGenPlasma");
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 * scale;
            let fy = y as f64 * scale;
            // 多个正弦波叠加产生等离子体效果
            let v = (fx.sin() + fy.sin() + perlin.noise_2d(fx * 2.0, fy * 2.0) * 2.0).abs();
            let t = (v / 4.0).clamp(0.0, 1.0);
            let hue = t * 360.0;
            let (r, g, b) = hsl_to_rgb(hue, 0.8, 0.5);
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_marble 生成大理石纹理。
///
/// 用法：
///   imageGenMarble(width, height) → image
///   imageGenMarble(width, height, scale, seed) → image
fn bi_image_gen_marble(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenMarble")? as u32;
    let h = bh::as_int(args, 1, "imageGenMarble")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenMarble() width 和 height 必须 > 0"));
    }
    let scale = opt_float(args, 2, 0.05);
    let seed = opt_seed(args, 3, "imageGenMarble");
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 * scale;
            let fy = y as f64 * scale;
            let n = perlin.fbm(fx, fy, 4, 0.5, 2.0);
            // 用噪声扰动正弦波产生大理石纹理
            let t = ((fx + fy + n * 4.0).sin() + 1.0) * 0.5;
            let v = value_to_gray(t);
            img.put_pixel(x, y, Rgba([v, v, v, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_wood 生成木纹纹理。
///
/// 用法：
///   imageGenWood(width, height) → image
///   imageGenWood(width, height, scale, seed) → image
fn bi_image_gen_wood(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenWood")? as u32;
    let h = bh::as_int(args, 1, "imageGenWood")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenWood() width 和 height 必须 > 0"));
    }
    let scale = opt_float(args, 2, 0.05);
    let seed = opt_seed(args, 3, "imageGenWood");
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 * scale;
            let fy = y as f64 * scale;
            let n = perlin.noise_2d(fx, fy);
            // 同心圆 + 噪声扰动模拟年轮
            let r = ((fx * fx + fy * fy) * 3.0 + n * 5.0).sin();
            let t = (r + 1.0) * 0.5;
            // 木纹色调
            let r = (160.0 + t * 50.0) as u8;
            let g = (110.0 + t * 40.0) as u8;
            let b = (60.0 + t * 30.0) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_clouds 生成云彩纹理。
///
/// 用法：
///   imageGenClouds(width, height) → image
///   imageGenClouds(width, height, scale, seed) → image
fn bi_image_gen_clouds(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenClouds")? as u32;
    let h = bh::as_int(args, 1, "imageGenClouds")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenClouds() width 和 height 必须 > 0"));
    }
    let scale = opt_float(args, 2, 0.02);
    let seed = opt_seed(args, 3, "imageGenClouds");
    let perlin = Perlin::new(seed);
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = perlin.fbm(x as f64 * scale, y as f64 * scale, 6, 0.5, 2.0);
            // 增强对比度，让云彩更明显
            let t = (v * 1.5).clamp(0.0, 1.0);
            let g = value_to_gray(t);
            img.put_pixel(x, y, Rgba([g, g, g, 255]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

// ============ 渐变与图案 ============

/// bi_image_gen_linear 生成线性渐变图片。
///
/// 用法：
///   imageGenLinear(width, height, color1, color2) → image
///   imageGenLinear(width, height, color1, color2, direction) → image
///
/// direction: "h" 水平（默认）, "v" 垂直, "d" 对角
fn bi_image_gen_linear(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenLinear")? as u32;
    let h = bh::as_int(args, 1, "imageGenLinear")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenLinear() width 和 height 必须 > 0"));
    }
    let c1 = parse_color(&args[2], "imageGenLinear")?;
    let c2 = parse_color(&args[3], "imageGenLinear")?;
    let dir = if args.len() > 4 {
        bh::as_str(args, 4, "imageGenLinear")?.to_string()
    } else {
        "h".to_string()
    };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let t = match dir.as_str() {
                "h" | "horizontal" => x as f64 / (w - 1).max(1) as f64,
                "v" | "vertical" => y as f64 / (h - 1).max(1) as f64,
                "d" | "diagonal" => {
                    (x as f64 + y as f64) / ((w + h - 2) as f64).max(1.0)
                }
                other => {
                    return Err(error_value(format!(
                        "imageGenLinear() direction 应为 \"h\"/\"v\"/\"d\"，得到 \"{}\"",
                        other,
                    )))
                }
            };
            img.put_pixel(x, y, lerp_color(c1, c2, t));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_radial 生成径向渐变图片。
///
/// 用法：
///   imageGenRadial(width, height, cx, cy, color1, color2) → image
///   imageGenRadial(width, height, color1, color2) → image  (cx/cy 默认居中)
fn bi_image_gen_radial(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenRadial")? as u32;
    let h = bh::as_int(args, 1, "imageGenRadial")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenRadial() width 和 height 必须 > 0"));
    }
    // 支持两种参数顺序：(w,h,c1,c2) 或 (w,h,cx,cy,c1,c2)
    let (cx, cy, c1, c2) = if args.len() >= 6 {
        let cx = bh::as_int(args, 2, "imageGenRadial")? as f64;
        let cy = bh::as_int(args, 3, "imageGenRadial")? as f64;
        let c1 = parse_color(&args[4], "imageGenRadial")?;
        let c2 = parse_color(&args[5], "imageGenRadial")?;
        (cx, cy, c1, c2)
    } else {
        let c1 = parse_color(&args[2], "imageGenRadial")?;
        let c2 = parse_color(&args[3], "imageGenRadial")?;
        (w as f64 / 2.0, h as f64 / 2.0, c1, c2)
    };
    let max_dist = ((w as f64).powi(2) + (h as f64).powi(2)).sqrt() / 2.0;
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)).sqrt();
            let t = (d / max_dist).clamp(0.0, 1.0);
            img.put_pixel(x, y, lerp_color(c1, c2, t));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_conic 生成锥形渐变图片。
///
/// 用法：
///   imageGenConic(width, height, color1, color2) → image
///   imageGenConic(width, height, cx, cy, color1, color2) → image
fn bi_image_gen_conic(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenConic")? as u32;
    let h = bh::as_int(args, 1, "imageGenConic")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenConic() width 和 height 必须 > 0"));
    }
    let (cx, cy, c1, c2) = if args.len() >= 6 {
        let cx = bh::as_int(args, 2, "imageGenConic")? as f64;
        let cy = bh::as_int(args, 3, "imageGenConic")? as f64;
        let c1 = parse_color(&args[4], "imageGenConic")?;
        let c2 = parse_color(&args[5], "imageGenConic")?;
        (cx, cy, c1, c2)
    } else {
        let c1 = parse_color(&args[2], "imageGenConic")?;
        let c2 = parse_color(&args[3], "imageGenConic")?;
        (w as f64 / 2.0, h as f64 / 2.0, c1, c2)
    };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let angle = (y as f64 - cy).atan2(x as f64 - cx);
            // atan2 返回 [-pi, pi]，映射到 [0, 1]
            let t = (angle / std::f64::consts::PI + 1.0) * 0.5;
            img.put_pixel(x, y, lerp_color(c1, c2, t));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_checker 生成棋盘格图案。
///
/// 用法：
///   imageGenChecker(width, height) → image
///   imageGenChecker(width, height, cellSize, color1, color2) → image
fn bi_image_gen_checker(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenChecker")? as u32;
    let h = bh::as_int(args, 1, "imageGenChecker")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenChecker() width 和 height 必须 > 0"));
    }
    let cell_size = opt_int(args, 2, 16, "imageGenChecker").max(1) as u32;
    let c1 = if args.len() > 3 {
        parse_color(&args[3], "imageGenChecker")?
    } else {
        Rgba([255, 255, 255, 255])
    };
    let c2 = if args.len() > 4 {
        parse_color(&args[4], "imageGenChecker")?
    } else {
        Rgba([0, 0, 0, 255])
    };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let cx = x / cell_size;
            let cy = y / cell_size;
            let color = if (cx + cy) % 2 == 0 { c1 } else { c2 };
            img.put_pixel(x, y, color);
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_stars 生成星空图案。
///
/// 用法：
///   imageGenStars(width, height) → image
///   imageGenStars(width, height, count, seed) → image
fn bi_image_gen_stars(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenStars")? as u32;
    let h = bh::as_int(args, 1, "imageGenStars")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenStars() width 和 height 必须 > 0"));
    }
    let count = opt_int(args, 2, 100, "imageGenStars").max(0) as usize;
    let seed = opt_seed(args, 3, "imageGenStars");
    let mut rng = Rng::new(seed);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([10, 10, 30, 255]));
    for _ in 0..count {
        let x = rng.range_i64(0, w as i64) as u32;
        let y = rng.range_i64(0, h as i64) as u32;
        let brightness = rng.next_f64();
        let g = value_to_gray(brightness);
        // 大星星有光晕
        let size = if brightness > 0.85 { 2 } else { 1 };
        for dy in 0..size * 2 + 1 {
            for dx in 0..size * 2 + 1 {
                let px = x as i32 + dx as i32 - size as i32;
                let py = y as i32 + dy as i32 - size as i32;
                if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
                    let d = ((dx as f64 - size as f64).powi(2)
                        + (dy as f64 - size as f64).powi(2))
                    .sqrt();
                    if d <= size as f64 {
                        let fade = (1.0 - d / size as f64).max(0.0);
                        let r = (g as f64 * fade) as u8;
                        img.put_pixel(px as u32, py as u32, Rgba([r, r, r, 255]));
                    }
                }
            }
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_maze 生成迷宫图案（基于 DFS 递归回溯）。
///
/// 用法：
///   imageGenMaze(width, height) → image
///   imageGenMaze(width, height, cellSize, seed) → image
fn bi_image_gen_maze(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenMaze")? as u32;
    let h = bh::as_int(args, 1, "imageGenMaze")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenMaze() width 和 height 必须 > 0"));
    }
    let cell_size = opt_int(args, 2, 10, "imageGenMaze").max(2) as u32;
    let seed = opt_seed(args, 3, "imageGenMaze");
    let mut rng = Rng::new(seed);
    // 迷宫网格（每个 cell 有 4 面墙）
    let cols = w / cell_size;
    let rows = h / cell_size;
    if cols < 2 || rows < 2 {
        // 网格太小，直接返回黑图
        let img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
        return Ok(wrap_image(DynamicImage::ImageRgba8(img)));
    }
    // walls[y][x] = (top, right, bottom, left) true 表示有墙
    let mut walls = vec![vec![(true, true, true, true); cols as usize]; rows as usize];
    let mut visited = vec![vec![false; cols as usize]; rows as usize];
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    visited[0][0] = true;
    let dirs = [(0i32, -1i32, 0, 2), (1, 0, 1, 3), (0, 1, 2, 0), (-1, 0, 3, 1)];
    while let Some(&(cx, cy)) = stack.last() {
        // 找未访问邻居
        let mut neighbors = Vec::new();
        for &(dx, dy, wall, opp_wall) in &dirs {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx >= 0 && nx < cols as i32 && ny >= 0 && ny < rows as i32 {
                let nux = nx as usize;
                let nuy = ny as usize;
                if !visited[nuy][nux] {
                    neighbors.push((nux, nuy, wall, opp_wall));
                }
            }
        }
        if neighbors.is_empty() {
            stack.pop();
        } else {
            // 随机选一个
            let idx = rng.range_i64(0, neighbors.len() as i64) as usize;
            let (nx, ny, wall, opp_wall) = neighbors[idx];
            // 拆墙
            let (mut w0, mut w1, mut w2, mut w3) = walls[cy][cx];
            match wall {
                0 => w0 = false,
                1 => w1 = false,
                2 => w2 = false,
                3 => w3 = false,
                _ => {}
            }
            walls[cy][cx] = (w0, w1, w2, w3);
            let (mut nw0, mut nw1, mut nw2, mut nw3) = walls[ny][nx];
            match opp_wall {
                0 => nw0 = false,
                1 => nw1 = false,
                2 => nw2 = false,
                3 => nw3 = false,
                _ => {}
            }
            walls[ny][nx] = (nw0, nw1, nw2, nw3);
            visited[ny][nx] = true;
            stack.push((nx, ny));
        }
    }
    // 绘制到图片
    let mut img = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    // 画墙
    for y in 0..rows {
        for x in 0..cols {
            let (top, right, bottom, left) = walls[y as usize][x as usize];
            let px = x * cell_size;
            let py = y * cell_size;
            let wall_color = Rgba([0, 0, 0, 255]);
            if top {
                for dx in 0..cell_size {
                    if px + dx < w {
                        img.put_pixel(px + dx, py, wall_color);
                    }
                }
            }
            if left {
                for dy in 0..cell_size {
                    if py + dy < h {
                        img.put_pixel(px, py + dy, wall_color);
                    }
                }
            }
            if x == cols - 1 && right {
                for dy in 0..cell_size {
                    if py + dy < h {
                        img.put_pixel(px + cell_size - 1, py + dy, wall_color);
                    }
                }
            }
            if y == rows - 1 && bottom {
                for dx in 0..cell_size {
                    if px + dx < w {
                        img.put_pixel(px + dx, py + cell_size - 1, wall_color);
                    }
                }
            }
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_circles 生成随机圆形图案。
///
/// 用法：
///   imageGenCircles(width, height) → image
///   imageGenCircles(width, height, count, seed) → image
fn bi_image_gen_circles(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenCircles")? as u32;
    let h = bh::as_int(args, 1, "imageGenCircles")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenCircles() width 和 height 必须 > 0"));
    }
    let count = opt_int(args, 2, 30, "imageGenCircles").max(1) as usize;
    let seed = opt_seed(args, 3, "imageGenCircles");
    let mut rng = Rng::new(seed);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([20, 20, 40, 255]));
    let max_r = (w.min(h) as f64 * 0.15).max(8.0);
    for _ in 0..count {
        let cx = rng.range_f64(0.0, w as f64);
        let cy = rng.range_f64(0.0, h as f64);
        let r = rng.range_f64(max_r * 0.3, max_r);
        let hue = rng.next_f64() * 360.0;
        let (cr, cg, cb) = hsl_to_rgb(hue, 0.7, 0.5);
        let color = Rgba([cr, cg, cb, 220]);
        // 用圆方程填充
        let r_sq = r * r;
        let x0 = (cx - r).max(0.0) as u32;
        let x1 = (cx + r).min(w as f64 - 1.0) as u32;
        let y0 = (cy - r).max(0.0) as u32;
        let y1 = (cy + r).min(h as f64 - 1.0) as u32;
        for py in y0..=y1 {
            for px in x0..=x1 {
                let d = (px as f64 - cx).powi(2) + (py as f64 - cy).powi(2);
                if d <= r_sq {
                    img.put_pixel(px, py, color);
                }
            }
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_lines 生成随机线条图案。
///
/// 用法：
///   imageGenLines(width, height) → image
///   imageGenLines(width, height, count, seed) → image
fn bi_image_gen_lines(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenLines")? as u32;
    let h = bh::as_int(args, 1, "imageGenLines")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenLines() width 和 height 必须 > 0"));
    }
    let count = opt_int(args, 2, 20, "imageGenLines").max(1) as usize;
    let seed = opt_seed(args, 3, "imageGenLines");
    let mut rng = Rng::new(seed);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([15, 15, 25, 255]));
    for _ in 0..count {
        let x1 = rng.range_i64(0, w as i64) as i32;
        let y1 = rng.range_i64(0, h as i64) as i32;
        let x2 = rng.range_i64(0, w as i64) as i32;
        let y2 = rng.range_i64(0, h as i64) as i32;
        let hue = rng.next_f64() * 360.0;
        let (r, g, b) = hsl_to_rgb(hue, 0.8, 0.6);
        let color = Rgba([r, g, b, 255]);
        // Bresenham 画线
        let dx = (x2 - x1).abs();
        let dy = -(y2 - y1).abs();
        let sx = if x1 < x2 { 1 } else { -1 };
        let sy = if y1 < y2 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut cx, mut cy) = (x1, y1);
        loop {
            if cx >= 0 && cx < w as i32 && cy >= 0 && cy < h as i32 {
                img.put_pixel(cx as u32, cy as u32, color);
            }
            if cx == x2 && cy == y2 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                cx += sx;
            }
            if e2 <= dx {
                err += dx;
                cy += sy;
            }
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_particles 生成粒子图案。
///
/// 用法：
///   imageGenParticles(width, height) → image
///   imageGenParticles(width, height, count, seed) → image
fn bi_image_gen_particles(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenParticles")? as u32;
    let h = bh::as_int(args, 1, "imageGenParticles")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenParticles() width 和 height 必须 > 0"));
    }
    let count = opt_int(args, 2, 50, "imageGenParticles").max(1) as usize;
    let seed = opt_seed(args, 3, "imageGenParticles");
    let mut rng = Rng::new(seed);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255]));
    // 每个粒子从中心向外发散，留下轨迹
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    for _ in 0..count {
        let angle = rng.next_f64() * std::f64::consts::TAU;
        let speed = rng.range_f64(0.5, 2.0);
        let mut px = cx;
        let mut py = cy;
        let (dx, dy) = (angle.cos() * speed, angle.sin() * speed);
        let hue = rng.next_f64() * 360.0;
        let (r, g, b) = hsl_to_rgb(hue, 0.9, 0.6);
        let steps = rng.range_i64(50, 200);
        for _ in 0..steps {
            px += dx;
            py += dy;
            if px < 0.0 || px >= w as f64 || py < 0.0 || py >= h as f64 {
                break;
            }
            // 渐变透明度，让粒子有拖尾
            let fade = 1.0 - (steps as f64).recip();
            let alpha = (200.0 * fade) as u8;
            img.put_pixel(px as u32, py as u32, Rgba([r, g, b, alpha]));
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

// ============ 分形 ============

/// bi_image_gen_mandelbrot 生成曼德博集合分形图。
///
/// 用法：
///   imageGenMandelbrot(width, height) → image
///   imageGenMandelbrot(width, height, maxIter, zoom, centerX, centerY) → image
///
/// 默认展示完整曼德博集合（中心 -0.5, 0，缩放 1.0）
fn bi_image_gen_mandelbrot(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenMandelbrot")? as u32;
    let h = bh::as_int(args, 1, "imageGenMandelbrot")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenMandelbrot() width 和 height 必须 > 0"));
    }
    let max_iter = opt_int(args, 2, 100, "imageGenMandelbrot").max(10) as u32;
    let zoom = opt_float(args, 3, 1.0).max(0.01);
    let cx = opt_float(args, 4, -0.5);
    let cy = opt_float(args, 5, 0.0);
    let mut img = RgbaImage::new(w, h);
    let scale = 3.5 / (w as f64 * zoom);
    for py in 0..h {
        for px in 0..w {
            let x0 = cx + (px as f64 - w as f64 / 2.0) * scale;
            let y0 = cy + (py as f64 - h as f64 / 2.0) * scale;
            let mut x = 0.0;
            let mut y = 0.0;
            let mut iter = 0;
            while x * x + y * y < 4.0 && iter < max_iter {
                let x_new = x * x - y * y + x0;
                y = 2.0 * x * y + y0;
                x = x_new;
                iter += 1;
            }
            let color = if iter == max_iter {
                Rgba([0, 0, 0, 255])
            } else {
                // 平滑着色：用对数缩放迭代次数
                let smooth = iter as f64 + 1.0 - (x * x + y * y).ln().ln() / (2.0f64.ln());
                let t = (smooth / max_iter as f64).clamp(0.0, 1.0);
                let hue = t * 360.0;
                let (r, g, b) = hsl_to_rgb(hue, 0.7, 0.5);
                Rgba([r, g, b, 255])
            };
            img.put_pixel(px, py, color);
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

/// bi_image_gen_julia 生成朱利亚集合分形图。
///
/// 用法：
///   imageGenJulia(width, height, cr, ci) → image
///   imageGenJulia(width, height, cr, ci, maxIter, zoom) → image
///
/// 常用参数：(cr=-0.7, ci=0.27)、(cr=-0.4, ci=0.6)、(cr=0.285, ci=0.01)
fn bi_image_gen_julia(_vm: &mut VM, args: &[Value]) -> Result<Value, Value> {
    let w = bh::as_int(args, 0, "imageGenJulia")? as u32;
    let h = bh::as_int(args, 1, "imageGenJulia")? as u32;
    if w == 0 || h == 0 {
        return Err(error_value("imageGenJulia() width 和 height 必须 > 0"));
    }
    let cr = bh::as_float(args, 2, "imageGenJulia")?;
    let ci = bh::as_float(args, 3, "imageGenJulia")?;
    let max_iter = opt_int(args, 4, 100, "imageGenJulia").max(10) as u32;
    let zoom = opt_float(args, 5, 1.0).max(0.01);
    let mut img = RgbaImage::new(w, h);
    let scale = 3.5 / (w as f64 * zoom);
    for py in 0..h {
        for px in 0..w {
            let mut x = (px as f64 - w as f64 / 2.0) * scale;
            let mut y = (py as f64 - h as f64 / 2.0) * scale;
            let mut iter = 0;
            while x * x + y * y < 4.0 && iter < max_iter {
                let x_new = x * x - y * y + cr;
                y = 2.0 * x * y + ci;
                x = x_new;
                iter += 1;
            }
            let color = if iter == max_iter {
                Rgba([0, 0, 0, 255])
            } else {
                let t = (iter as f64 / max_iter as f64).clamp(0.0, 1.0);
                let hue = t * 360.0;
                let (r, g, b) = hsl_to_rgb(hue, 0.7, 0.5);
                Rgba([r, g, b, 255])
            };
            img.put_pixel(px, py, color);
        }
    }
    Ok(wrap_image(DynamicImage::ImageRgba8(img)))
}

// ============ 单元测试 ============

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
    fn test_image_gen_noise() {
        let v = eval(r#"
            return imageGenNoise(20, 20, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_perlin() {
        let v = eval(r#"
            return imageGenPerlin(64, 64, 0.05, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_perlin_default() {
        let v = eval(r#"
            return imageGenPerlin(32, 32)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_fbm() {
        let v = eval(r#"
            return imageGenFBM(64, 64, 4, 0.05, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_worley() {
        let v = eval(r#"
            return imageGenWorley(64, 64, 16, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_voronoi() {
        let v = eval(r#"
            return imageGenVoronoi(50, 50, 10, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_plasma() {
        let v = eval(r#"
            return imageGenPlasma(64, 64, 0.05, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_marble() {
        let v = eval(r#"
            return imageGenMarble(64, 64, 0.05, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_wood() {
        let v = eval(r#"
            return imageGenWood(64, 64, 0.05, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_clouds() {
        let v = eval(r#"
            return imageGenClouds(64, 64, 0.02, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_linear_h() {
        let v = eval(r#"
            return imageGenLinear(20, 20, [0, 0, 0, 255], [255, 255, 255, 255])
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_linear_v() {
        let v = eval(r#"
            return imageGenLinear(20, 20, [0, 0, 0, 255], [255, 255, 255, 255], "v")
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_radial() {
        let v = eval(r#"
            return imageGenRadial(30, 30, [255, 0, 0, 255], [0, 0, 255, 255])
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_radial_with_center() {
        let v = eval(r#"
            return imageGenRadial(30, 30, 10, 10, [255, 0, 0, 255], [0, 0, 255, 255])
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_conic() {
        let v = eval(r#"
            return imageGenConic(30, 30, [255, 0, 0, 255], [0, 0, 255, 255])
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_checker() {
        let v = eval(r#"
            return imageGenChecker(40, 40, 8, [255, 255, 255, 255], [0, 0, 0, 255])
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_checker_default() {
        let v = eval(r#"
            return imageGenChecker(40, 40)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_stars() {
        let v = eval(r#"
            return imageGenStars(50, 50, 30, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_maze() {
        let v = eval(r#"
            return imageGenMaze(80, 80, 8, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_circles() {
        let v = eval(r#"
            return imageGenCircles(60, 60, 10, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_lines() {
        let v = eval(r#"
            return imageGenLines(60, 60, 10, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_particles() {
        let v = eval(r#"
            return imageGenParticles(60, 60, 20, 42)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_mandelbrot() {
        let v = eval(r#"
            return imageGenMandelbrot(60, 60, 50, 1.0, -0.5, 0.0)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_image_gen_julia() {
        let v = eval(r#"
            return imageGenJulia(60, 60, -0.7, 0.27, 50, 1.0)
        "#);
        assert!(matches!(v, Value::Native(_)));
    }

    #[test]
    fn test_rng_reproducible() {
        let mut r1 = Rng::new(42);
        let mut r2 = Rng::new(42);
        for _ in 0..10 {
            assert_eq!(r1.next_u64(), r2.next_u64());
        }
    }

    #[test]
    fn test_perlin_smooth() {
        let p = Perlin::new(42);
        // 相近位置应有相近值
        let v1 = p.noise_2d(0.5, 0.5);
        let v2 = p.noise_2d(0.51, 0.51);
        assert!((v1 - v2).abs() < 0.1);
    }

    #[test]
    fn test_image_gen_zero_size_error() {
        let mut sf = Sflang::new();
        let wrapped = "func __f() { return imageGenNoise(0, 10) } var __r = __f()";
        let result = sf.run_string(wrapped);
        assert!(result.is_err(), "应该报错");
    }
}
