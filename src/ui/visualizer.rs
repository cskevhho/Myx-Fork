//! A restrained character field: spectrum-driven on native playback and
//! playback-timed ambient motion on Soloist, which does not expose audio data.
use crate::*;

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const GLYPHS: [&str; 12] = [
    "音", "波", "流", "光", "響", "空", "カ", "ナ", "ミ", "ノ", "サ", "リ",
];

pub(crate) fn render_visualizer(f: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if area.height < 4 || area.width < 16 {
        return;
    }
    let audio = app
        .svc
        .engine
        .bands
        .try_lock()
        .ok()
        .and_then(|g| g.is_active.then_some((g.values, g.peak_envelope.max(1e-6))));
    let label = if audio.is_some() {
        "SPECTRUM"
    } else {
        "AMBIENT · PLAYBACK MOTION"
    };
    f.render_widget(
        Paragraph::new(label)
            .style(theme.muted())
            .alignment(Alignment::Center),
        Rect { height: 1, ..area },
    );
    let rect = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    let time = if myx::config::get().reduced_motion {
        0.0
    } else {
        app.playback.position_ms() as f32 / 1000.0
    };
    let columns = (rect.width as usize / 2).min(80);
    let height = rect.height.min(20) as usize;
    let x_offset = (rect.width.saturating_sub((columns * 2) as u16)) / 2;
    let y_offset = (rect.height.saturating_sub(height as u16)) / 2;
    let stops = [theme.info, theme.primary, theme.accent];
    let mut lines = Vec::with_capacity(height);
    for y in 0..height {
        let mut spans = Vec::with_capacity(columns);
        for x in 0..columns {
            let xf = x as f32 / columns.max(1) as f32;
            let yf = y as f32 / height.max(1) as f32;
            // Superimposed travelling ripples echo the organic koi motion in
            // astro-v1; fixed Bayer thresholds keep the texture stable.
            let wave =
                (xf * 13.0 - time * 0.65).sin() * 0.16 + (xf * 21.0 + time * 0.38).sin() * 0.09;
            let amplitude = audio
                .as_ref()
                .map(|(values, peak)| {
                    let index = (x * NUM_BANDS / columns.max(1)).min(NUM_BANDS - 1);
                    (values[index] / peak).sqrt().clamp(0.0, 1.0)
                })
                .unwrap_or(0.5);
            let distance = (yf - (0.5 + wave)).abs();
            let intensity = ((0.30 + amplitude * 0.20 - distance) * 2.2).clamp(0.0, 1.0);
            let threshold = (BAYER[y % 4][x % 4] as f32 + 0.5) / 16.0;
            let glyph = if intensity <= threshold {
                "  "
            } else if intensity < 0.42 {
                "· "
            } else if intensity < 0.7 {
                ": "
            } else {
                GLYPHS[(x * 17 + y * 7) % GLYPHS.len()]
            };
            let color = gradient::interpolate(&stops, (1.0 - yf).clamp(0.0, 1.0));
            spans.push(Span::styled(glyph, Style::default().fg(color.into())));
        }
        lines.push(Line::from(spans));
    }
    f.render_widget(
        Paragraph::new(lines),
        Rect {
            x: rect.x + x_offset,
            y: rect.y + y_offset,
            width: (columns * 2) as u16,
            height: height as u16,
        },
    );
}
