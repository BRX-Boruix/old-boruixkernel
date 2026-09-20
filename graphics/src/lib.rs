#![no_std]

use limine::FramebufferRequest;
use spin::Mutex;

#[no_mangle]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new(0);

// Color constants
pub const COLOR_WHITE: u32 = 0xFFFFFFFF;
pub const COLOR_BLACK: u32 = 0xFF000000;
pub const COLOR_RED: u32 = 0xFFFF0000;
pub const COLOR_GREEN: u32 = 0xFF00FF00;
pub const COLOR_BLUE: u32 = 0xFF0000FF;
pub const COLOR_BACKGROUND: u32 = 0xFF103090; // "Depression Blue" as noted by reviewer

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FrameBufferInfo {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: usize,
    pub addr: usize,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
}

pub struct FrameBuffer {
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    addr: *mut u8,
}

// SAFETY: Framebuffer access must be synchronized.
// We use a Mutex to ensure thread safety.
unsafe impl Send for FrameBuffer {}
unsafe impl Sync for FrameBuffer {}

impl FrameBuffer {
    pub fn info(&self) -> FrameBufferInfo {
        FrameBufferInfo {
            width: self.width,
            height: self.height,
            pitch: self.pitch,
            bpp: self.bpp,
            addr: self.addr as usize,
            red_mask_size: self.red_mask_size,
            red_mask_shift: self.red_mask_shift,
            green_mask_size: self.green_mask_size,
            green_mask_shift: self.green_mask_shift,
            blue_mask_size: self.blue_mask_size,
            blue_mask_shift: self.blue_mask_shift,
        }
    }

    pub fn new() -> Option<Self> {
        let resp = FRAMEBUFFER_REQUEST.get_response();
        if let Some(resp) = resp.get() {
            if let Some(fb) = resp.framebuffers().first() {
                if let Some(addr) = fb.address.as_ptr() {
                    return Some(Self {
                        width: fb.width as usize,
                        height: fb.height as usize,
                        pitch: fb.pitch as usize,
                        bpp: fb.bpp as usize,
                        red_mask_size: fb.red_mask_size,
                        red_mask_shift: fb.red_mask_shift,
                        green_mask_size: fb.green_mask_size,
                        green_mask_shift: fb.green_mask_shift,
                        blue_mask_size: fb.blue_mask_size,
                        blue_mask_shift: fb.blue_mask_shift,
                        addr,
                    });
                }
            }
        }
        None
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn bpp(&self) -> usize {
        self.bpp
    }

    pub fn pitch(&self) -> usize {
        self.pitch
    }

    pub fn addr(&self) -> usize {
        self.addr as usize
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let pixel_offset = y * self.pitch + x * (self.bpp / 8);

        unsafe {
            let pixel_ptr = self.addr.add(pixel_offset);

            match self.bpp {
                32 => {
                    let packed = self.pack_color(color);
                    *(pixel_ptr as *mut u32) = packed;
                }
                24 => {
                    let packed = self.pack_color(color);
                    *pixel_ptr.add(0) = (packed & 0xFF) as u8;
                    *pixel_ptr.add(1) = ((packed >> 8) & 0xFF) as u8;
                    *pixel_ptr.add(2) = ((packed >> 16) & 0xFF) as u8;
                }
                _ => {
                    // Fallback or error logging could go here.
                    // For now, we just don't draw if not 32bpp to avoid crash
                }
            }
        }
    }

    fn pack_color(&self, color: u32) -> u32 {
        let r = (color >> 16) & 0xFF;
        let g = (color >> 8) & 0xFF;
        let b = color & 0xFF;

        let mut value = 0u32;
        value |= Self::pack_component(r, self.red_mask_size, self.red_mask_shift);
        value |= Self::pack_component(g, self.green_mask_size, self.green_mask_shift);
        value |= Self::pack_component(b, self.blue_mask_size, self.blue_mask_shift);
        value
    }

    fn pack_component(c: u32, size: u8, shift: u8) -> u32 {
        if size == 0 {
            return 0;
        }
        let max = (1u32 << size) - 1;
        let scaled = (c * max) / 255;
        scaled << shift
    }

    pub fn clear(&mut self, color: u32) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: u32) {
        for cy in y..(y + height) {
            for cx in x..(x + width) {
                self.put_pixel(cx, cy, color);
            }
        }
    }
}

pub static FRAMEBUFFER: Mutex<Option<FrameBuffer>> = Mutex::new(None);

pub fn init() -> bool {
    let mut fb = FRAMEBUFFER.lock();
    *fb = FrameBuffer::new();
    fb.is_some()
}

pub fn draw() {
    let mut fb_guard = FRAMEBUFFER.lock();
    if let Some(fb) = fb_guard.as_mut() {
        // Clear screen with background color
        fb.clear(COLOR_BACKGROUND);

        // Draw a centered rectangle
        let rect_w = fb.width() / 3;
        let rect_h = fb.height() / 3;
        let rect_x = (fb.width() - rect_w) / 2;
        let rect_y = (fb.height() - rect_h) / 2;

        fb.draw_rect(rect_x, rect_y, rect_w, rect_h, COLOR_WHITE);
    }
}
