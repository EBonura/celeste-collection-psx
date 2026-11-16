//! The two screen buffers, presented through the SDK's one present path (`psx_rt::present`).
//!
//! `Screen` keeps the shape of the `FrameBuffer` it replaces: `clear` at the start of a frame, draw,
//! `swap` at the end. `swap` hands the frame to the presenter, which flips it at the first VBlank
//! after the GPU has finished it, and renders audio ahead until that flip has landed (what
//! `wait_vblank` did before the swap).

use psx_gpu::display::{DoubleBuffer, Resolution};
use psx_hw::gpu::{gp0, pack_xy};
use psx_io::gpu::{wait_command_ready, write_command};
use psx_rt::present::{self, PresentCap, Presenter};

/// The 320x240 double buffer and the presenter that flips it.
pub struct Screen {
    buffers: DoubleBuffer,
    presenter: Presenter,
}

impl Screen {
    /// Take over presentation. Call after `install_vblank_counter`, with the GPU idle. The games
    /// run their logic and draw at the display rate, so the cap is lifted.
    #[inline(never)]
    pub fn new() -> Self {
        present::set_cap(PresentCap::UNCAPPED);
        let mut screen = Self {
            buffers: DoubleBuffer::new(Resolution::R320X240),
            presenter: Presenter::new(None),
        };
        screen.presenter.begin_frame(&screen.buffers);
        screen
    }

    /// Fill the buffer being drawn with a colour.
    #[inline(never)]
    pub fn clear(&self, r: u8, g: u8, b: u8) {
        crate::backend::draw_sync();
        let (at, size) = (self.buffers.draw_origin(), self.buffers.size());
        wait_command_ready();
        write_command(gp0::fill_rect(r, g, b));
        write_command(pack_xy(at.0, at.1));
        write_command(pack_xy(size.0, size.1));
    }

    /// Present the frame drawn into the draw side, wait (rendering audio) until it is on screen,
    /// and point the draw area at the other side.
    #[inline(never)]
    pub fn swap(&mut self) {
        crate::backend::draw_sync();
        self.presenter.present_drawn(&mut self.buffers);
        crate::sfx::fill_while(present::is_flip_pending);
        self.presenter.begin_frame(&self.buffers);
    }
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}
