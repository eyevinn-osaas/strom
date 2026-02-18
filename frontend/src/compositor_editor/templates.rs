use super::*;

impl CompositorEditor {
    // ===== Layout Templates =====

    /// Multiview - Row 1: 2 large, Row 2: 4 medium, Row 3: remaining small
    pub(super) fn apply_template_multiview(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let spacing = h * 3 / 100; // 3% spacing between rows

        // Row heights: 45%, 28%, remainder
        let row1_h = h * 45 / 100;
        let row2_h = h * 28 / 100;
        let row1_y = 0;
        let row2_y = row1_h + spacing;
        let row3_y = row2_y + row2_h + spacing;
        let row3_h = h - row3_y;

        // Row 1: First 2 inputs (large, side by side)
        let row1_w = w / 2;
        for i in 0..2 {
            if let Some(input) = self.inputs.get_mut(i) {
                input.xpos = (i as i32) * row1_w;
                input.ypos = row1_y;
                input.width = row1_w;
                input.height = row1_h;
                input.zorder = i as u32;
            }
        }

        // Row 2: Next 4 inputs (medium, 4 columns)
        let row2_w = w / 4;
        for i in 0..4 {
            let idx = 2 + i;
            if let Some(input) = self.inputs.get_mut(idx) {
                input.xpos = (i as i32) * row2_w;
                input.ypos = row2_y;
                input.width = row2_w;
                input.height = row2_h;
                input.zorder = idx as u32;
            }
        }

        // Row 3: Remaining inputs (small, evenly distributed)
        let remaining_count = self.inputs.len().saturating_sub(6);
        if remaining_count > 0 {
            let row3_w = w / remaining_count as i32;
            for i in 0..remaining_count {
                let idx = 6 + i;
                if let Some(input) = self.inputs.get_mut(idx) {
                    input.xpos = (i as i32) * row3_w;
                    input.ypos = row3_y;
                    input.width = row3_w;
                    input.height = row3_h;
                    input.zorder = idx as u32;
                }
            }
        }

        // Hide any inputs beyond what we have (shouldn't happen, but be safe)
        // All inputs should be positioned by now
    }

    /// Full screen - Input 0 fills the entire output
    pub(super) fn apply_template_fullscreen(&mut self) {
        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = self.output_width as i32;
            input.height = self.output_height as i32;
            input.zorder = 0;
        }
        // Hide other inputs off-screen
        for (i, input) in self.inputs.iter_mut().enumerate().skip(1) {
            input.xpos = -(self.output_width as i32);
            input.ypos = 0;
            input.width = 1;
            input.height = 1;
            input.zorder = i as u32;
        }
    }

    /// Picture-in-Picture - Input 0 full screen, Input 1 small in corner
    pub(super) fn apply_template_pip(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let pip_w = w / 4;
        let pip_h = h / 4;
        let margin = 20;

        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = w;
            input.height = h;
            input.zorder = 0;
        }
        if let Some(input) = self.inputs.get_mut(1) {
            input.xpos = w - pip_w - margin;
            input.ypos = h - pip_h - margin;
            input.width = pip_w;
            input.height = pip_h;
            input.zorder = 1;
        }
        // Hide remaining inputs
        for (i, input) in self.inputs.iter_mut().enumerate().skip(2) {
            input.xpos = -(w);
            input.zorder = i as u32;
        }
    }

    /// Side by Side - Two inputs split horizontally
    pub(super) fn apply_template_side_by_side(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let half_w = w / 2;

        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = half_w;
            input.height = h;
            input.zorder = 0;
        }
        if let Some(input) = self.inputs.get_mut(1) {
            input.xpos = half_w;
            input.ypos = 0;
            input.width = half_w;
            input.height = h;
            input.zorder = 1;
        }
        for (i, input) in self.inputs.iter_mut().enumerate().skip(2) {
            input.xpos = -(w);
            input.zorder = i as u32;
        }
    }

    /// Top / Bottom - Two inputs split vertically
    pub(super) fn apply_template_top_bottom(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let half_h = h / 2;

        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = w;
            input.height = half_h;
            input.zorder = 0;
        }
        if let Some(input) = self.inputs.get_mut(1) {
            input.xpos = 0;
            input.ypos = half_h;
            input.width = w;
            input.height = half_h;
            input.zorder = 1;
        }
        for (i, input) in self.inputs.iter_mut().enumerate().skip(2) {
            input.xpos = -(w);
            input.zorder = i as u32;
        }
    }

    /// 2x2 Grid - Four inputs in a grid
    pub(super) fn apply_template_grid_2x2(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let cell_w = w / 2;
        let cell_h = h / 2;

        let positions = [(0, 0), (cell_w, 0), (0, cell_h), (cell_w, cell_h)];
        for (i, input) in self.inputs.iter_mut().enumerate() {
            if i < 4 {
                input.xpos = positions[i].0;
                input.ypos = positions[i].1;
                input.width = cell_w;
                input.height = cell_h;
                input.zorder = i as u32;
            } else {
                input.xpos = -(w);
                input.zorder = i as u32;
            }
        }
    }

    /// 3x3 Grid - Nine inputs in a grid
    pub(super) fn apply_template_grid_3x3(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let cell_w = w / 3;
        let cell_h = h / 3;

        for (i, input) in self.inputs.iter_mut().enumerate() {
            if i < 9 {
                let col = (i % 3) as i32;
                let row = (i / 3) as i32;
                input.xpos = col * cell_w;
                input.ypos = row * cell_h;
                input.width = cell_w;
                input.height = cell_h;
                input.zorder = i as u32;
            } else {
                input.xpos = -(w);
                input.zorder = i as u32;
            }
        }
    }

    /// 1 Large + 2 Small - Main input with two smaller on the side
    pub(super) fn apply_template_1_large_2_small(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let main_w = (w * 3) / 4;
        let side_w = w - main_w;
        let side_h = h / 2;

        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = main_w;
            input.height = h;
            input.zorder = 0;
        }
        if let Some(input) = self.inputs.get_mut(1) {
            input.xpos = main_w;
            input.ypos = 0;
            input.width = side_w;
            input.height = side_h;
            input.zorder = 1;
        }
        if let Some(input) = self.inputs.get_mut(2) {
            input.xpos = main_w;
            input.ypos = side_h;
            input.width = side_w;
            input.height = side_h;
            input.zorder = 2;
        }
        for (i, input) in self.inputs.iter_mut().enumerate().skip(3) {
            input.xpos = -(w);
            input.zorder = i as u32;
        }
    }

    /// 1 Large + 3 Small - Main input with three smaller below
    pub(super) fn apply_template_1_large_3_small(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let main_h = (h * 3) / 4;
        let bottom_h = h - main_h;
        let bottom_w = w / 3;

        if let Some(input) = self.inputs.get_mut(0) {
            input.xpos = 0;
            input.ypos = 0;
            input.width = w;
            input.height = main_h;
            input.zorder = 0;
        }
        for i in 1..=3 {
            if let Some(input) = self.inputs.get_mut(i) {
                input.xpos = ((i - 1) as i32) * bottom_w;
                input.ypos = main_h;
                input.width = bottom_w;
                input.height = bottom_h;
                input.zorder = i as u32;
            }
        }
        for (i, input) in self.inputs.iter_mut().enumerate().skip(4) {
            input.xpos = -(w);
            input.zorder = i as u32;
        }
    }

    /// Vertical Stack - All inputs stacked vertically
    pub(super) fn apply_template_vertical_stack(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let count = self.inputs.len().max(1);
        let cell_h = h / count as i32;

        for (i, input) in self.inputs.iter_mut().enumerate() {
            input.xpos = 0;
            input.ypos = (i as i32) * cell_h;
            input.width = w;
            input.height = cell_h;
            input.zorder = i as u32;
        }
    }

    /// Horizontal Stack - All inputs stacked horizontally
    pub(super) fn apply_template_horizontal_stack(&mut self) {
        let w = self.output_width as i32;
        let h = self.output_height as i32;
        let count = self.inputs.len().max(1);
        let cell_w = w / count as i32;

        for (i, input) in self.inputs.iter_mut().enumerate() {
            input.xpos = (i as i32) * cell_w;
            input.ypos = 0;
            input.width = cell_w;
            input.height = h;
            input.zorder = i as u32;
        }
    }
}
