// Face, Eye and Mouth geometry ported from M5Stack-Avatar 0.10.0.
// Copyright (c) 2018 Shinya Ishikawa. Licensed under the MIT license.
// See LICENSE in this crate for the complete notice.

//! The original white M5Stack Avatar face, independent of LCD and firmware code.
//!
//! Uses the original 320×240 face coordinates, 0.4 scale and (-96, -56) offset
//! configured by this project's C++ firmware. Rendering writes RGB565 bytes in
//! display transmission order and performs no allocation.

use std::fmt;

pub mod animation;

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 128;
pub const FRAME_BYTES: usize = WIDTH * HEIGHT * 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Expression {
    #[default]
    Neutral,
    Happy,
    Sleepy,
    Doubt,
    Sad,
    Angry,
}

/// Positions use the same ranges as Avatar's DrawContext: mouth 0..1, breath
/// -1..1, gaze -1..1, and clockwise rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceParams {
    pub expression: Expression,
    pub mouth_open: f32,
    pub breath: f32,
    pub gaze_x: f32,
    pub gaze_y: f32,
    pub eyes_open: bool,
    pub rotation_deg: f32,
}

impl Default for FaceParams {
    fn default() -> Self {
        Self {
            expression: Expression::Neutral,
            mouth_open: 0.0,
            breath: 0.0,
            gaze_x: 0.0,
            gaze_y: 0.0,
            eyes_open: true,
            rotation_deg: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    UnsupportedSize { width: usize, height: usize },
    BufferSize { expected: usize, actual: usize },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSize { width, height } => {
                write!(
                    f,
                    "Avatar face requires 128×128 pixels, received {width}×{height}"
                )
            }
            Self::BufferSize { expected, actual } => {
                write!(
                    f,
                    "Avatar framebuffer needs {expected} bytes, received {actual}"
                )
            }
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders the complete face over `background`, a native RGB565 color.
///
/// Only the original ATOMS3R 128×128 viewport is supported. An invalid viewport
/// or buffer is rejected before any pixels are changed. Primary color is white.
pub fn render_rgb565_be(
    frame: &mut [u8],
    width: usize,
    height: usize,
    params: FaceParams,
    background: u16,
) -> Result<(), RenderError> {
    if (width, height) != (WIDTH, HEIGHT) {
        return Err(RenderError::UnsupportedSize { width, height });
    }
    if frame.len() != FRAME_BYTES {
        return Err(RenderError::BufferSize {
            expected: FRAME_BYTES,
            actual: frame.len(),
        });
    }

    let face = Geometry::new(params);
    let angle = finite(params.rotation_deg, 0.0) % 360.0;
    let (sin, cos) = angle.to_radians().sin_cos();
    let bg = background.to_be_bytes();
    if bg[0] == bg[1] {
        frame.fill(bg[0]);
    } else {
        for pixel in frame.chunks_exact_mut(2) {
            pixel.copy_from_slice(&bg);
        }
    }

    // Most of the display is background. Limit the inverse transform to the
    // two eyes and mouth so drawing a face does not hold up audio playback.
    // Sampling itself stays identical to the original full-screen renderer.
    for bounds in face.bounds() {
        let (left, top, right, bottom) = bounds.viewport(sin, cos);
        for y in top..bottom {
            for x in left..right {
                // LovyanGFX pushRotateZoom uses pixel-center pivots (+0.5).
                // These inverse coordinates sample the original 1-bit sprite.
                let dx = (x as f32 - 64.0) * 2.5;
                let dy = (y as f32 - 64.0) * 2.5;
                let sx = (cos * dx + sin * dy + 160.5).floor() as i32;
                let sy = (-sin * dx + cos * dy + 120.5).floor() as i32;
                if (0..320).contains(&sx) && (0..240).contains(&sy) && face.contains(sx, sy) {
                    let index = (y * WIDTH + x) * 2;
                    frame[index..index + 2].copy_from_slice(&[0xff, 0xff]);
                }
            }
        }
    }
    Ok(())
}

fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

struct Geometry {
    expression: Expression,
    eyes_open: bool,
    mouth_x: i32,
    mouth_y: i32,
    mouth_width: i32,
    mouth_height: i32,
    eye_offset_x: i32,
    eye_offset_y: i32,
    breath: f32,
}

#[derive(Clone, Copy)]
struct Bounds {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
}

impl Bounds {
    fn viewport(self, sin: f32, cos: f32) -> (usize, usize, usize, usize) {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for sy in [self.top, self.top + self.height] {
            for sx in [self.left, self.left + self.width] {
                let dx = (sx as f32 - 160.5) * 0.4;
                let dy = (sy as f32 - 120.5) * 0.4;
                let x = 64.0 + cos * dx - sin * dy;
                let y = 64.0 + sin * dx + cos * dy;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        // Include source pixel cells and a two-pixel guard for the forward
        // bound's floating-point rounding. The inverse sampler remains the
        // authority for whether any candidate pixel is white.
        (
            ((min_x.floor() as i32 - 2).clamp(0, WIDTH as i32)) as usize,
            ((min_y.floor() as i32 - 2).clamp(0, HEIGHT as i32)) as usize,
            ((max_x.ceil() as i32 + 2).clamp(0, WIDTH as i32)) as usize,
            ((max_y.ceil() as i32 + 2).clamp(0, HEIGHT as i32)) as usize,
        )
    }
}

impl Geometry {
    fn new(params: FaceParams) -> Self {
        let breath = finite(params.breath, 0.0).clamp(-1.0, 1.0);
        let open = finite(params.mouth_open, 0.0).clamp(0.0, 1.0);
        let mouth_height = (4.0 + 56.0 * open) as i32;
        let mouth_width = (50.0 + 40.0 * (1.0 - open)) as i32;
        // BoundingRect::setPosition truncates the positive sum, before the
        // mouth adds its separate two-pixel breathing displacement.
        let mouth_top = (148.0 + breath * 3.0) as i32;
        Self {
            expression: params.expression,
            eyes_open: params.eyes_open,
            mouth_x: 163 - mouth_width / 2,
            mouth_y: (mouth_top as f32 - (mouth_height / 2) as f32 + breath * 2.0) as i32,
            mouth_width,
            mouth_height,
            eye_offset_x: (finite(params.gaze_x, 0.0).clamp(-1.0, 1.0) * 3.0) as i32,
            eye_offset_y: (finite(params.gaze_y, 0.0).clamp(-1.0, 1.0) * 3.0) as i32,
            breath,
        }
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        in_rect(
            x,
            y,
            self.mouth_x,
            self.mouth_y,
            self.mouth_width,
            self.mouth_height,
        ) || self.eye(x, y, 90, 93, false)
            || self.eye(x, y, 230, 96, true)
    }

    fn bounds(&self) -> [Bounds; 3] {
        [
            Bounds {
                left: self.mouth_x,
                top: self.mouth_y,
                width: self.mouth_width,
                height: self.mouth_height,
            },
            Bounds {
                left: 90 + self.eye_offset_x - 8,
                top: (93.0 + self.breath * 3.0) as i32 + self.eye_offset_y - 8,
                width: 17,
                height: 17,
            },
            Bounds {
                left: 230 + self.eye_offset_x - 8,
                top: (96.0 + self.breath * 3.0) as i32 + self.eye_offset_y - 8,
                width: 17,
                height: 17,
            },
        ]
    }

    fn eye(&self, x: i32, y: i32, center_x: i32, center_y: i32, is_left: bool) -> bool {
        let x = x - center_x - self.eye_offset_x;
        let y = y - (center_y as f32 + self.breath * 3.0) as i32 - self.eye_offset_y;
        if !self.eyes_open {
            return in_rect(x, y, -8, -2, 16, 4);
        }
        if !circle8(x, y) {
            return false;
        }
        match self.expression {
            Expression::Happy => y < 0 && !circle5(x, y),
            Expression::Sleepy => y >= 2,
            Expression::Sad | Expression::Angry => {
                let row = y + 8;
                if !(0..=8).contains(&row) {
                    return true;
                }
                // Eye.cpp erases the upper triangle. Its sloping edge is
                // included, matching fillTriangle's integer scan conversion.
                let bottom_left = !is_left != (self.expression != Expression::Sad);
                let erased = if bottom_left {
                    x <= 8 - (2 * row - 1).max(0)
                } else {
                    x >= -8 + (2 * row - 1).max(0)
                };
                !erased
            }
            Expression::Neutral | Expression::Doubt => true,
        }
    }
}

fn in_rect(x: i32, y: i32, left: i32, top: i32, width: i32, height: i32) -> bool {
    (left..left + width).contains(&x) && (top..top + height).contains(&y)
}

// Half-widths of the original integer circle raster, indexed by |y|. Keep its
// pixel edges before scaling rather than introducing antialiasing/new shapes.
fn circle8(x: i32, y: i32) -> bool {
    const HALF_WIDTH: [i32; 9] = [8, 8, 8, 7, 7, 6, 5, 4, 2];
    HALF_WIDTH
        .get(y.unsigned_abs() as usize)
        .is_some_and(|&r| x.abs() <= r)
}

fn circle5(x: i32, y: i32) -> bool {
    const HALF_WIDTH: [i32; 6] = [5, 5, 5, 4, 3, 2];
    HALF_WIDTH
        .get(y.unsigned_abs() as usize)
        .is_some_and(|&r| x.abs() <= r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(params: FaceParams) -> Vec<u8> {
        let mut frame = vec![0; FRAME_BYTES];
        render_rgb565_be(&mut frame, WIDTH, HEIGHT, params, 0).unwrap();
        frame
    }

    fn white(frame: &[u8], x: usize, y: usize) -> bool {
        frame[(y * WIDTH + x) * 2..][..2] == [0xff, 0xff]
    }

    // The original full-screen sampler is intentionally kept independent of
    // the bounds optimization to catch clipped eyes, mouths and rotated edges.
    fn render_reference(params: FaceParams, background: u16) -> Vec<u8> {
        let mut frame = vec![0; FRAME_BYTES];
        let face = Geometry::new(params);
        let angle = finite(params.rotation_deg, 0.0) % 360.0;
        let (sin, cos) = angle.to_radians().sin_cos();
        let bg = background.to_be_bytes();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let dx = (x as f32 - 64.0) * 2.5;
                let dy = (y as f32 - 64.0) * 2.5;
                let sx = (cos * dx + sin * dy + 160.5).floor() as i32;
                let sy = (-sin * dx + cos * dy + 120.5).floor() as i32;
                let pixel =
                    if (0..320).contains(&sx) && (0..240).contains(&sy) && face.contains(sx, sy) {
                        [0xff, 0xff]
                    } else {
                        bg
                    };
                let index = (y * WIDTH + x) * 2;
                frame[index..index + 2].copy_from_slice(&pixel);
            }
        }
        frame
    }

    #[test]
    fn bounded_renderer_matches_full_sampler_for_all_expressions_and_rotations() {
        let expressions = [
            Expression::Neutral,
            Expression::Happy,
            Expression::Sleepy,
            Expression::Doubt,
            Expression::Sad,
            Expression::Angry,
        ];
        let mut seed = 0xc0ffee_u32;
        let mut random = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / 16777215.0
        };
        let mut frame = vec![0; FRAME_BYTES];
        for sample in 0..512 {
            let params = FaceParams {
                expression: expressions[sample % expressions.len()],
                mouth_open: random() * 1.5 - 0.25,
                breath: random() * 3.0 - 1.5,
                gaze_x: random() * 3.0 - 1.5,
                gaze_y: random() * 3.0 - 1.5,
                eyes_open: sample % 3 != 0,
                rotation_deg: match sample % 8 {
                    0 => 0.0,
                    1 => 90.0,
                    2 => -180.0,
                    3 => 270.0,
                    4 => -359.999,
                    _ => random() * 1440.0 - 720.0,
                },
            };
            let background = (random() * u16::MAX as f32) as u16;
            render_rgb565_be(&mut frame, WIDTH, HEIGHT, params, background).unwrap();
            assert_eq!(
                frame,
                render_reference(params, background),
                "sample {sample}: {params:?}"
            );
        }
    }

    #[test]
    fn neutral_face_has_original_small_eyes_and_single_row_mouth() {
        let frame = render(FaceParams::default());
        // Source mouth x118..207,y146..149 -> physical x47..82,y75.
        for y in 65..90 {
            for x in 40..90 {
                assert_eq!(white(&frame, x, y), y == 75 && (47..=82).contains(&x));
            }
        }
        assert!(white(&frame, 36, 53));
        assert!(white(&frame, 92, 54));
        assert!(!white(&frame, 36, 49));
        assert!(!white(&frame, 92, 58));
        assert!(!white(&frame, 64, 53));
        assert!(frame
            .chunks_exact(2)
            .all(|p| p == [0, 0] || p == [0xff, 0xff]));
    }

    #[test]
    fn open_mouth_is_narrower_and_taller_than_closed_mouth() {
        let frame = render(FaceParams {
            mouth_open: 1.0,
            ..FaceParams::default()
        });
        // Original 50×60 mouth at (138,118), scaled to 20×24 pixels.
        for y in 60..92 {
            for x in 45..87 {
                assert_eq!(
                    white(&frame, x, y),
                    (55..=74).contains(&x) && (63..=86).contains(&y)
                );
            }
        }
    }

    #[test]
    fn blink_makes_horizontal_lines_and_doubt_keeps_original_neutral_geometry() {
        let neutral = render(FaceParams::default());
        assert_eq!(
            neutral,
            render(FaceParams {
                expression: Expression::Doubt,
                ..FaceParams::default()
            })
        );
        let closed = render(FaceParams {
            eyes_open: false,
            ..FaceParams::default()
        });
        for y in 48..60 {
            for x in 30..44 {
                assert_eq!(white(&closed, x, y), y == 53 && (33..=38).contains(&x));
            }
        }
    }

    #[test]
    fn expression_masks_retain_white_color_and_erase_eye_parts() {
        let neutral = render(FaceParams::default());
        for expression in [
            Expression::Happy,
            Expression::Sleepy,
            Expression::Sad,
            Expression::Angry,
        ] {
            let frame = render(FaceParams {
                expression,
                ..FaceParams::default()
            });
            assert_ne!(frame, neutral);
            for y in 48..60 {
                for x in 30..100 {
                    if white(&frame, x, y) {
                        assert!(white(&neutral, x, y));
                    }
                }
            }
        }
    }

    #[test]
    fn eye_pixels_match_independent_original_avatar_raster_fixtures() {
        // Captured independently from the original integer circle/triangle
        // primitives and LovyanGFX's 0.4 pixel-center transform.
        let fixtures: &[(Expression, usize, usize, &[&str])] = &[
            (
                Expression::Neutral,
                33,
                50,
                &[
                    "..##...", ".#####.", "######.", "#######", "#######", ".#####.", "..###..",
                ],
            ),
            (
                Expression::Neutral,
                89,
                51,
                &[
                    "..##...", ".#####.", "######.", "#######", "#######", "######.", "..###..",
                ],
            ),
            (
                Expression::Angry,
                33,
                51,
                &[
                    ".#.....", "####...", "######.", "#######", ".#####.", "..###..",
                ],
            ),
            (
                Expression::Angry,
                89,
                53,
                &["...###.", "..#####", "#######", "######.", "..###.."],
            ),
            (
                Expression::Sad,
                33,
                51,
                &[
                    ".....#.", "...###.", ".######", "#######", ".#####.", "..###..",
                ],
            ),
            (
                Expression::Sad,
                89,
                53,
                &["####...", "#####..", "#######", "######.", "..###.."],
            ),
            (Expression::Happy, 33, 50, &["..##..", ".#..##", "##...#"]),
            (
                Expression::Happy,
                89,
                51,
                &["..##...", ".#####.", "##...#.", "#.....#"],
            ),
            (
                Expression::Sleepy,
                33,
                54,
                &["#######", ".#####.", "..###.."],
            ),
            (
                Expression::Sleepy,
                89,
                55,
                &["#######", "######.", "..###.."],
            ),
        ];
        for &(expression, x, y, rows) in fixtures {
            let frame = render(FaceParams {
                expression,
                ..FaceParams::default()
            });
            for (dy, row) in rows.iter().enumerate() {
                for (dx, pixel) in row.bytes().enumerate() {
                    assert_eq!(
                        white(&frame, x + dx, y + dy),
                        pixel == b'#',
                        "{expression:?} at ({}, {})",
                        x + dx,
                        y + dy
                    );
                }
            }
        }
    }

    #[test]
    fn negative_breath_preserves_original_positive_coordinate_integer_casts() {
        let params = FaceParams {
            breath: -0.25,
            ..FaceParams::default()
        };
        let face = Geometry::new(params);
        // Face rectangle 148-0.75 truncates to147; Mouth 147-2-0.5 ->144.
        assert_eq!(face.mouth_y, 144);
        // Eye center 93-0.75 truncates to92, rather than adding trunc(-0.75).
        assert!(face.eye(90, 84, 90, 93, false));
        assert!(!face.eye(90, 83, 90, 93, false));
    }

    #[test]
    fn background_is_native_rgb565_written_big_endian_and_rotation_moves_whole_face() {
        let mut frame = vec![0; FRAME_BYTES];
        render_rgb565_be(&mut frame, WIDTH, HEIGHT, FaceParams::default(), 0x03e0).unwrap();
        assert_eq!(&frame[..2], &[0x03, 0xe0]);
        let rotated = render(FaceParams {
            rotation_deg: 180.0,
            ..FaceParams::default()
        });
        assert!(!white(&rotated, 64, 75));
        assert!(white(&rotated, 64, 53));
    }

    #[test]
    fn invalid_buffers_are_rejected_without_mutation_and_numeric_inputs_are_bounded() {
        let mut short = [0x55; 8];
        assert!(matches!(
            render_rgb565_be(&mut short, WIDTH, HEIGHT, FaceParams::default(), 0),
            Err(RenderError::BufferSize { .. })
        ));
        assert_eq!(short, [0x55; 8]);
        assert!(matches!(
            render_rgb565_be(&mut short, 1, 4, FaceParams::default(), 0),
            Err(RenderError::UnsupportedSize { .. })
        ));
        assert_eq!(
            render(FaceParams::default()),
            render(FaceParams {
                mouth_open: f32::NAN,
                breath: f32::INFINITY,
                gaze_x: f32::NAN,
                gaze_y: f32::NEG_INFINITY,
                rotation_deg: f32::NAN,
                ..FaceParams::default()
            })
        );
    }
}
