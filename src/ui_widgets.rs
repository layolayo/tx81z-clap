use nih_plug_egui::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

/// Return true if operator `op_num` (1..=4) is a carrier in `alg` (0..=7).
pub fn is_carrier(alg: usize, op_num: usize) -> bool {
    match alg {
        0..=3 => op_num == 1,
        4 => op_num == 1 || op_num == 3,
        5 | 6 => op_num == 1 || op_num == 2 || op_num == 3,
        7 => true,
        _ => op_num == 1,
    }
}

/// Draw an interactive visual TX81Z FM Algorithm routing diagram (with OP4 feedback).
pub fn draw_algorithm_diagram(ui: &mut egui::Ui, alg: usize, feedback: i32, op_ons: [bool; 4]) {
    let desired = Vec2::new(260.0, 120.0);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let p = ui.painter_at(rect);

    p.rect_filled(rect, 6.0, Color32::from_rgb(22, 26, 34));
    p.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(55, 65, 80)),
        egui::StrokeKind::Inside,
    );

    // Normalized (x, y) positions for [OP1, OP2, OP3, OP4] inside rect
    let coords: [(f32, f32); 4] = match alg {
        0 => [(0.50, 0.82), (0.50, 0.60), (0.50, 0.38), (0.50, 0.16)], // 4->3->2->1
        1 => [(0.50, 0.80), (0.50, 0.52), (0.34, 0.22), (0.66, 0.22)], // (4+3)->2->1
        2 => [(0.50, 0.80), (0.34, 0.48), (0.66, 0.48), (0.66, 0.20)], // (4->3)+2->1
        3 => [(0.50, 0.80), (0.34, 0.50), (0.66, 0.50), (0.50, 0.20)], // 4->(3+2)->1
        4 => [(0.34, 0.72), (0.34, 0.28), (0.66, 0.72), (0.66, 0.28)], // 4->3 + 2->1
        5 => [(0.24, 0.72), (0.50, 0.72), (0.76, 0.72), (0.50, 0.24)], // 4->(1+2+3)
        6 => [(0.22, 0.72), (0.46, 0.72), (0.74, 0.72), (0.74, 0.26)], // 4->3 + 2 + 1
        _ => [(0.17, 0.68), (0.39, 0.68), (0.61, 0.68), (0.83, 0.68)], // 1+2+3+4
    };

    let to_pos = |nx: f32, ny: f32| -> Pos2 {
        Pos2::new(rect.left() + nx * rect.width(), rect.top() + ny * rect.height())
    };
    let centers: [Pos2; 4] = [
        to_pos(coords[0].0, coords[0].1),
        to_pos(coords[1].0, coords[1].1),
        to_pos(coords[2].0, coords[2].1),
        to_pos(coords[3].0, coords[3].1),
    ];

    // Edges: (from_op_idx, to_op_idx) where 0=OP1..3=OP4
    let edges: &[(usize, usize)] = match alg {
        0 => &[(3, 2), (2, 1), (1, 0)],
        1 => &[(3, 1), (2, 1), (1, 0)],
        2 => &[(3, 2), (2, 0), (1, 0)],
        3 => &[(3, 2), (3, 1), (2, 0), (1, 0)],
        4 => &[(3, 2), (1, 0)],
        5 => &[(3, 2), (3, 1), (3, 0)],
        6 => &[(3, 2)],
        _ => &[],
    };

    let line_stroke = Stroke::new(1.6, Color32::from_rgb(130, 150, 175));
    for &(from, to) in edges {
        p.line_segment([centers[from], centers[to]], line_stroke);
    }

    // Output bus line at bottom for carriers
    let bus_y = rect.bottom() - 8.0;
    for op in 0..4 {
        if is_carrier(alg, op + 1) {
            p.line_segment(
                [centers[op], Pos2::new(centers[op].x, bus_y)],
                Stroke::new(1.5, Color32::from_rgb(80, 210, 240)),
            );
        }
    }

    // OP4 Feedback loop
    let fb_c = centers[3];
    let fb_rect = Rect::from_min_max(
        Pos2::new(fb_c.x + 10.0, fb_c.y - 12.0),
        Pos2::new(fb_c.x + 26.0, fb_c.y + 4.0),
    );
    let fb_color = if feedback > 0 {
        Color32::from_rgb(255, 180, 70)
    } else {
        Color32::from_rgb(75, 85, 100)
    };
    p.rect_stroke(fb_rect, 2.0, Stroke::new(1.4, fb_color), egui::StrokeKind::Inside);

    // Operator boxes
    for op in 0..4 {
        let box_rect = Rect::from_center_size(centers[op], Vec2::new(26.0, 18.0));
        let carrier = is_carrier(alg, op + 1);
        let fill = if !op_ons[op] {
            Color32::from_rgb(45, 45, 50)
        } else if carrier {
            Color32::from_rgb(35, 125, 165)
        } else {
            Color32::from_rgb(145, 95, 45)
        };
        let border = if carrier {
            Color32::from_rgb(100, 220, 255)
        } else {
            Color32::from_rgb(230, 170, 90)
        };
        p.rect_filled(box_rect, 3.0, fill);
        p.rect_stroke(
            box_rect,
            3.0,
            Stroke::new(1.2, border),
            egui::StrokeKind::Inside,
        );
        p.text(
            centers[op],
            egui::Align2::CENTER_CENTER,
            format!("{}", op + 1),
            egui::FontId::proportional(11.0),
            Color32::WHITE,
        );
    }
}

/// Draw a live 4-stage TX81Z EG (Attack -> Decay1 -> Decay2 -> Release) curve.
pub fn draw_eg_graph(
    ui: &mut egui::Ui,
    ar: i32,
    d1r: i32,
    d1l: i32,
    d2r: i32,
    rr: i32,
    carrier: bool,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, Color32::from_rgb(18, 22, 28));

    let pad = 4.0;
    let w = (rect.width() - pad * 2.0).max(10.0);
    let h = (rect.height() - pad * 2.0).max(10.0);
    let left = rect.left() + pad;
    let bottom = rect.bottom() - pad;

    // Higher rate = faster stage (shorter horizontal width)
    let atk_w = ((32 - ar).max(1) as f32 / 32.0) * (w * 0.25);
    let d1_w = ((32 - d1r).max(1) as f32 / 32.0) * (w * 0.28);
    let d1_lvl = (d1l as f32 / 15.0) * h;
    let d2_w = w * 0.24;
    let d2_drop = (d2r as f32 / 31.0) * d1_lvl * 0.7;
    let d2_lvl = (d1_lvl - d2_drop).max(0.0);
    let rel_w = ((16 - rr).max(1) as f32 / 16.0) * (w * 0.23);

    let pts = [
        Pos2::new(left, bottom),
        Pos2::new(left + atk_w, bottom - h),
        Pos2::new(left + atk_w + d1_w, bottom - d1_lvl),
        Pos2::new(left + atk_w + d1_w + d2_w, bottom - d2_lvl),
        Pos2::new(left + atk_w + d1_w + d2_w + rel_w, bottom),
    ];

    let color = if carrier {
        Color32::from_rgb(90, 215, 250)
    } else {
        Color32::from_rgb(245, 175, 85)
    };
    p.add(egui::Shape::line(pts.to_vec(), Stroke::new(1.8, color)));
}

/// Draw a mini preview of the 8 TX81Z operator waveforms (W1..W8).
pub fn draw_waveform_preview(ui: &mut egui::Ui, wave: i32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(42.0, 18.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, Color32::from_rgb(18, 22, 28));

    let cy = rect.center().y;
    let amp = rect.height() * 0.36;
    let steps = 24;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let phase = t * std::f32::consts::TAU;
        let s = phase.sin();
        let y_val = match wave {
            0 => s,                                                     // W1: Sine
            1 => if t < 0.5 { s.abs() } else { -s.abs() * 0.0 + 0.0 }, // W2: Half-sine-ish / alt
            2 => s.abs(),                                               // W3: Full rectified
            3 => if (t % 0.5) < 0.25 { s.abs() } else { 0.0 },          // W4: Quarter pulses
            4 => if t < 0.5 { (phase * 2.0).sin() } else { 0.0 },       // W5: Double freq 1st half
            5 => if t < 0.5 { (phase * 2.0).sin().abs() } else { 0.0 }, // W6
            6 => if t < 0.5 { s.abs() } else { -(phase * 2.0).sin().abs() }, // W7
            _ => if t < 0.25 || (t >= 0.5 && t < 0.75) { s.abs() } else { 0.0 }, // W8
        };
        pts.push(Pos2::new(
            rect.left() + 2.0 + t * (rect.width() - 4.0),
            cy - y_val * amp,
        ));
    }
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.4_f32, Color32::from_rgb(130, 230, 180)),
    ));
}

/// Draw a live preview of the TX81Z LFO (Saw Up, Square, Triangle, S&H) with Delay & PMD/AMD.
pub fn draw_lfo_preview(ui: &mut egui::Ui, wave: i32, speed: i32, delay: i32, pmd: i32, amd: i32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 4.0, Color32::from_rgb(18, 22, 28));
    p.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, Color32::from_rgb(50, 60, 75)),
        egui::StrokeKind::Inside,
    );

    let cy = rect.center().y;
    let amp = rect.height() * 0.40;
    let cycles = 1.5 + (speed as f32 / 99.0) * 5.0;
    let delay_frac = (delay as f32 / 99.0) * 0.45;
    let pmd_scale = pmd as f32 / 99.0;
    let amd_scale = amd as f32 / 99.0;

    let steps = 96;
    let mut pmd_pts = Vec::with_capacity(steps + 1);
    let mut amd_pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let phase = (t * cycles).fract();
        let env = if delay == 0 {
            1.0
        } else if t < delay_frac {
            0.0
        } else {
            let ramp_len = (delay_frac * 1.2).max(0.15);
            ((t - delay_frac) / ramp_len).min(1.0)
        };
        let raw = match wave {
            0 => 2.0 * phase - 1.0,                                    // Saw Up
            1 => if phase < 0.5 { 1.0 } else { -1.0 },                 // Square
            2 => 1.0 - 4.0 * (phase - 0.5).abs(),                      // Triangle
            _ => {
                let step = (t * cycles * 2.0).floor() as i32;
                let hash = ((step.wrapping_mul(1103515245).wrapping_add(12345)) >> 16) & 0x7F;
                (hash as f32 / 63.5) - 1.0                             // S&H
            }
        };
        let x = rect.left() + 4.0 + t * (rect.width() - 8.0);
        pmd_pts.push(Pos2::new(x, cy - raw * env * pmd_scale * amp));
        amd_pts.push(Pos2::new(x, cy - raw * env * amd_scale * amp));
    }

    p.add(egui::Shape::line(
        amd_pts,
        Stroke::new(1.5_f32, Color32::from_rgb(245, 175, 85)),
    ));
    p.add(egui::Shape::line(
        pmd_pts,
        Stroke::new(1.7_f32, Color32::from_rgb(90, 215, 250)),
    ));
    p.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        "PMD",
        egui::FontId::proportional(9.5),
        Color32::from_rgb(90, 215, 250),
    );
    p.text(
        Pos2::new(rect.left() + 34.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        "AMD",
        egui::FontId::proportional(9.5),
        Color32::from_rgb(245, 175, 85),
    );
}
