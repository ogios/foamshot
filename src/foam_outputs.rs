use cairo::{Context, ImageSurface};
use log::debug;
use smithay_client_toolkit::shm::slot::{self, Buffer, SlotPool};
use wayland_client::{
    QueueHandle,
    protocol::{wl_output, wl_shm::Format, wl_surface},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity},
};

use crate::{cairo_render::draw_base, foamshot::FoamShot};

/// NOTE: 为物理显示器做的抽象，包含其基础信息
#[derive(Default)]
pub struct FoamOutput {
    /// 索引，由output event进行赋值
    pub id: usize,
    /// 显示器的命名，也许会有用
    pub name: String,

    pub output: Option<wl_output::WlOutput>,
    pub width: i32,
    pub height: i32,

    ///显示器 左上角 全局坐标 x
    pub global_x: i32,
    ///显示器 左上角 全局坐标 y
    pub global_y: i32,
    pub logical_width: i32,
    pub logical_height: i32,
    /// 显示器缩放分数，默认设置为1
    #[allow(unused)]
    pub scale: i32,

    /// 用于screencopy
    // pub screencopy_frame: Option<zwlr_screencopy_frame_v1::ZwlrScreencopyFrameV1>,
    pub base_buffer: Option<Buffer>,
    pub current_canvas: Option<Vec<u8>>,
    // add freeze layer surfae to impl set_freeze
    pub surface: Option<wl_surface::WlSurface>,
    pub layer_surface: Option<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1>,
    // TODO: add sub rect with Option
    pub subrect: Option<SubRect>,
    pub old_subrect: Option<SubRect>,

    /// TEST:
    pub pool: Option<slot::SlotPool>,
}

impl FoamOutput {
    pub fn convert_pos_to_surface(
        src_output: &FoamOutput,
        target_output: &FoamOutput,
        surface_x: f64,
        surface_y: f64,
    ) -> (f64, f64) {
        let global_x = src_output.global_x as f64 + surface_x;
        let global_y = src_output.global_y as f64 + surface_y;

        let dst_x = global_x - target_output.global_x as f64;
        let dst_y = global_y - target_output.global_y as f64;

        (dst_x, dst_y)
    }
    pub fn new(id: usize, output: wl_output::WlOutput, pool: SlotPool) -> Self {
        Self {
            id,
            output: Some(output),
            scale: 1,
            name: "unnamed".to_string(),
            pool: Some(pool),
            ..Default::default()
        }
    }
    pub fn new_subrect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            self.subrect = None
        }
        self.subrect = Some(SubRect::new(self.id, x, y, w, h))
    }

    pub fn init_layer(
        &mut self,
        layer_shell: &zwlr_layer_shell_v1::ZwlrLayerShellV1,
        qh: &QueueHandle<FoamShot>,
    ) {
        let id = self.id;
        let output = self.output.as_ref().unwrap();
        // let layer_shell = wl_ctx.layer_shell.as_ref().expect("Missing layer shell");
        // let qh = wl_ctx.qh.as_ref().expect("Missing qh");
        let (w, h) = (self.width, self.height);
        let surface = self.surface.as_mut().expect("Missing surfaces");

        let layer = zwlr_layer_shell_v1::ZwlrLayerShellV1::get_layer_surface(
            layer_shell,
            surface,
            Some(output),
            Layer::Top,
            "foam_freeze".to_string(),
            qh,
            id,
        );

        // 配置 layer
        layer.set_anchor(Anchor::all());
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);

        self.layer_surface = Some(layer);
        surface.damage(0, 0, w, h);
        surface.commit();
    }

    pub fn store_canvas(&mut self) {
        let buffer = self.base_buffer.as_ref().unwrap();
        let pool = self.pool.as_mut().unwrap();
        let canvas: &mut [u8] = buffer.canvas(pool).unwrap();
        self.current_canvas = Some(canvas.to_vec())
    }

    pub fn freeze_attach(&mut self, base_canvas: &[u8]) {
        debug!("fn: freeze_attach");
        let (w, h) = (self.width, self.height);
        let surface = self.surface.as_ref().expect("Missing surfaces");
        let pool = self.pool.as_mut().unwrap();
        let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Argb8888).unwrap();
        canvas.copy_from_slice(base_canvas);

        draw_base(canvas, w, h);

        buffer.attach_to(surface).unwrap();
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        self.base_buffer = Some(buffer)
    }

    pub fn update_select_subrect(&mut self, base_canvas: &[u8], freeze: bool) {
        // NOTE: 绘制之前刷新缓存
        self.old_subrect = self.subrect.clone();

        let (w, h) = (self.width, self.height);
        let surface = self.surface.as_ref().expect("Missing surfaces");
        let pool = self.pool.as_mut().unwrap();
        let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Argb8888).unwrap();

        if freeze {
            canvas.fill(0);

            canvas.copy_from_slice(base_canvas);
        } else {
            canvas.fill(0);
        }

        let cairo_surface = unsafe {
            ImageSurface::create_for_data_unsafe(
                canvas.as_mut_ptr(),
                cairo::Format::ARgb32,
                w,
                h,
                w * 4,
            )
            .unwrap()
        };

        let cr = Context::new(&cairo_surface).unwrap();

        // 获取Cairo表面尺寸
        let surface_width = cairo_surface.width() as f64;
        let surface_height = cairo_surface.height() as f64;

        // 设置半透明白色
        cr.set_source_rgba(0.8, 0.8, 0.8, 0.3);
        cr.rectangle(0.0, 0.0, surface_width, surface_height);

        let (x, y, rw, rh) = (
            self.subrect.as_ref().unwrap().relative_min_x,
            self.subrect.as_ref().unwrap().relative_min_y,
            self.subrect.as_ref().unwrap().width,
            self.subrect.as_ref().unwrap().height,
        );

        // 添加内部矩形路径（作为裁剪区域）
        cr.rectangle(x.into(), y.into(), rw.into(), rh.into());

        // 使用奇偶填充规则，形成环形区域
        cr.set_fill_rule(cairo::FillRule::EvenOdd);

        // 填充路径区域
        cr.fill().unwrap();

        buffer.attach_to(surface).unwrap(); // 如果 attach_to 失败则返回

        surface.damage_buffer(0, 0, w, h);

        // 提交 surface
        surface.commit();
        self.base_buffer = Some(buffer)
    }

    pub fn clean_attach(&mut self) {
        let (w, h) = (self.width, self.height);
        let surface = self.surface.as_ref().expect("Missing surfaces");
        let pool = self.pool.as_mut().unwrap();
        let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Argb8888).unwrap();
        canvas.fill(0);
        buffer.attach_to(surface).unwrap();
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        self.base_buffer = Some(buffer)
    }

    pub fn no_freeze_attach(&mut self) {
        let (w, h) = (self.width, self.height);
        let surface = self.surface.as_ref().expect("Missing surfaces");
        let pool = self.pool.as_mut().unwrap();
        let (buffer, canvas) = pool.create_buffer(w, h, w * 4, Format::Argb8888).unwrap();
        canvas.fill(0);
        draw_base(canvas, w, h);

        buffer.attach_to(surface).unwrap();
        surface.damage_buffer(0, 0, w, h);
        surface.commit();
        self.base_buffer = Some(buffer)
    }

    #[inline(always)]
    pub fn is_subrect_changed(&self) -> bool {
        match self.old_subrect.as_ref() {
            // 当old存在时，检查new是否存在以及值是否相同
            Some(old) => self.subrect.as_ref() != Some(old),
            // old不存在时必然发生变化
            None => true,
        }
    }
    #[inline(always)]
    pub fn has_subrect(&self) -> bool {
        self.subrect.is_some()
    }

    pub fn is_dirty(&mut self) -> bool {
        self.has_subrect() && self.is_subrect_changed()
    }
    pub fn send_next_frame(&mut self, qh: &QueueHandle<FoamShot>, udata: usize) {
        let surface = self.surface.as_mut().unwrap();
        surface.frame(qh, udata);
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubRect {
    pub monitor_id: usize,
    pub relative_min_x: i32,
    pub relative_min_y: i32,
    pub width: i32,
    pub height: i32,
}
impl SubRect {
    pub fn new(id: usize, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            monitor_id: id,
            relative_min_x: x,
            relative_min_y: y,
            width: w,
            height: h,
        }
    }
}
