//! An animated koi pond, adapted from astro-v1's ASCII koi geometry.
use crate::*;

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const PROFILE: [(f32, f32); 9] = [
    (0.0, 0.0),
    (0.025, 0.052),
    (0.08, 0.096),
    (0.19, 0.133),
    (0.32, 0.14),
    (0.46, 0.116),
    (0.6, 0.077),
    (0.71, 0.035),
    (0.8, 0.019),
];
const FIN: [(f32, f32); 6] = [
    (0.19, 0.09),
    (0.28, 0.18),
    (0.44, 0.239),
    (0.49, 0.191),
    (0.42, 0.113),
    (0.32, 0.08),
];
const PELVIC: [(f32, f32); 4] = [(0.56, 0.05), (0.65, 0.12), (0.73, 0.13), (0.69, 0.052)];
const TAIL: [(f32, f32); 13] = [
    (0.76, -0.018),
    (0.87, -0.065),
    (1.015, -0.14),
    (1.065, -0.158),
    (1.105, -0.142),
    (1.09, -0.106),
    (0.99, -0.008),
    (1.085, 0.105),
    (1.1, 0.14),
    (1.06, 0.158),
    (1.01, 0.14),
    (0.87, 0.063),
    (0.76, 0.018),
];

#[derive(Clone, Copy, Debug)]
struct Fish {
    x: f32,
    y: f32,
    heading: f32,
    length: f32,
    phase: f32,
    kind: usize,
}

fn inside(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut hit = false;
    let mut previous = points.len() - 1;
    for i in 0..points.len() {
        let (ax, ay) = points[i];
        let (bx, by) = points[previous];
        if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
            hit = !hit;
        }
        previous = i;
    }
    hit
}

fn body_width(u: f32) -> f32 {
    for pair in PROFILE.windows(2) {
        let [(ax, ay), (bx, by)] = [pair[0], pair[1]];
        if u <= bx {
            return ay + (by - ay) * (u - ax) / (bx - ax);
        }
    }
    0.0
}

/// Sample the body, moving fins and forked tail in the fish's local coordinates.
fn fish_glyph(fish: Fish, x: f32, y: f32, time: f32) -> Option<char> {
    let (sin, cos) = fish.heading.sin_cos();
    let dx = x - fish.x;
    let dy = y - fish.y;
    let u = -(dx * cos + dy * sin) / fish.length;
    if !(-0.05..=1.12).contains(&u) {
        return None;
    }
    let envelope = ((u - 0.08) / 1.02).max(0.0).powf(1.35);
    let bend = 0.055 * (u * 5.4 - fish.phase - time * 1.65).sin() * envelope;
    let v = (-dx * sin + dy * cos) / fish.length - bend;
    let av = v.abs();
    let fin = inside(u, av, &FIN) || inside(u, av, &PELVIC);
    let tail = inside(u, v, &TAIL);
    let width = body_width(u);
    if (0.0..0.8).contains(&u) && av < width {
        let edge = av / width.max(0.001);
        let patches = if fish.kind == 0 {
            [
                (0.13, -0.045, 0.11, 0.085),
                (0.38, 0.065, 0.13, 0.095),
                (0.56, -0.04, 0.1, 0.065),
            ]
        } else {
            [
                (0.17, 0.025, 0.13, 0.1),
                (0.46, -0.03, 0.15, 0.095),
                (0.65, 0.02, 0.06, 0.045),
            ]
        };
        let patch = patches.iter().any(|&(cx, cy, rx, ry)| {
            ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2)
                < 1.0 + 0.16 * (u * 63.0 + v * 38.0).sin()
        });
        let scale = (u * 166.0 + (v * 136.0).cos() * 0.7).sin() * (v * 128.0).cos();
        return Some(if u < 0.11 && av > width * 0.38 {
            '•'
        } else if edge > 0.85 {
            ':'
        } else if patch {
            if scale > 0.0 {
                '#'
            } else {
                '+'
            }
        } else if scale > 0.35 {
            ':'
        } else {
            '·'
        });
    }
    if tail {
        return Some(if (v / (u - 0.72).max(0.01) * 21.0).sin() > 0.25 {
            '+'
        } else {
            ':'
        });
    }
    if fin {
        return Some(if ((u * 0.8 + av * 1.8) * 105.0).sin() > 0.25 {
            ':'
        } else {
            '.'
        });
    }
    None
}

/// Physical coordinates account for terminal cells being roughly twice as tall
/// as they are wide. Elliptical paths remain inside the drawable area.
fn fish_at(width: usize, height: usize, time: f32, kind: usize) -> Fish {
    let w = width as f32;
    let h = height as f32 * 2.0;
    let length = (w * 0.29).min(h * 1.15).clamp(8.0, 32.0);
    let phase = time * if kind == 0 { 0.16 } else { -0.13 } + kind as f32 * 2.4;
    let rx = (w * 0.5 - length * 0.65).max(0.0);
    let ry = (h * 0.5 - length * 0.45).max(0.0);
    let direction = if kind == 0 { 1.0 } else { -1.0 };
    let dx = -phase.sin() * rx * direction;
    let dy = phase.cos() * ry * direction;
    let heading = if dx.abs() + dy.abs() < 0.001 {
        0.0
    } else {
        dy.atan2(dx)
    };
    // Route the body centre; the glyph sampler's origin is the nose.
    Fish {
        x: w * 0.5 + rx * phase.cos() + heading.cos() * length * 0.4,
        y: h * 0.5 + ry * phase.sin() + heading.sin() * length * 0.4,
        heading,
        length,
        phase: kind as f32 * 2.1,
        kind,
    }
}

pub(crate) fn render_visualizer(f: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if area.height < 5 || area.width < 18 {
        return;
    }
    let width = usize::from(area.width.min(120));
    let height = usize::from((area.height - 1).min(20));
    let time = if myx::config::get().reduced_motion {
        0.0
    } else {
        app.playback.position_ms() as f32 / 1000.0
    };
    let fishes = [
        fish_at(width, height, time, 0),
        fish_at(width, height, time, 1),
    ];
    let mut lines = Vec::with_capacity(height);
    for y in 0..height {
        let mut spans = Vec::with_capacity(width);
        for x in 0..width {
            let sample = fishes.iter().find_map(|fish| {
                fish_glyph(*fish, x as f32, y as f32 * 2.0, time).map(|glyph| (glyph, fish.kind))
            });
            let (glyph, color) = match sample {
                Some((glyph, kind)) => (
                    glyph,
                    if kind == 0 {
                        theme.primary
                    } else {
                        theme.accent
                    },
                ),
                None => {
                    // Quiet ordered-dither water lets the koi silhouettes read.
                    let ripple = (x as f32 * 0.11 + y as f32 * 0.35 - time * 0.35).sin();
                    let dither = BAYER[y % 4][x % 4] as f32 / 16.0;
                    let mark = if ripple > 0.65 && dither > 0.77 {
                        '·'
                    } else if ripple < -0.82 && dither > 0.88 {
                        'ｶ'
                    } else {
                        ' '
                    };
                    (mark, theme.border_dimmest)
                }
            };
            spans.push(Span::styled(
                glyph.to_string(),
                Style::default().fg(color.into()),
            ));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: area.x + (area.width - width as u16) / 2,
            y: area.y + (area.height - height as u16) / 2,
            width: width as u16,
            height: height as u16,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn koi_body_and_tail_are_drawn_and_swimming_changes_the_pose() {
        let fish = Fish {
            x: 40.0,
            y: 20.0,
            heading: 0.0,
            length: 30.0,
            phase: 0.0,
            kind: 0,
        };
        assert!(fish_glyph(fish, 34.0, 20.0, 0.0).is_some());
        assert!(fish_glyph(fish, 10.0, 17.0, 0.0).is_some());
        assert!(fish_glyph(fish, 40.0, 2.0, 0.0).is_none());
        let a = fish_at(100, 20, 0.0, 0);
        let b = fish_at(100, 20, 5.0, 0);
        assert!((a.x - b.x).abs() + (a.y - b.y).abs() > 1.0);
    }
    #[test]
    fn small_pond_produces_finite_geometry() {
        for w in [18, 24, 40, 120] {
            for h in [4, 7, 20] {
                let fish = fish_at(w, h, 42.0, 1);
                assert!(fish.x.is_finite() && fish.y.is_finite() && fish.heading.is_finite());
            }
        }
    }
}
