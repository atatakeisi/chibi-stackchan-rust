//! Render the firmware's actual face pixels to a portable RGB image.
use stackchan_avatar_face::{render_rgb565_be, Expression, FaceParams, FRAME_BYTES, HEIGHT, WIDTH};
use std::{fs::File, io::{self, Write}, path::PathBuf};

fn main() -> io::Result<()> {
    let output = std::env::args_os().nth(1).map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/private/tmp/stackchan-avatar-face.ppm"));
    let expressions = [Expression::Neutral, Expression::Happy, Expression::Sleepy,
        Expression::Doubt, Expression::Sad, Expression::Angry];
    let mut grid = vec![0; WIDTH * HEIGHT * 6 * 2 * 3];
    let grid_width = WIDTH * 6;
    let mut frame = vec![0; FRAME_BYTES];
    for (column, expression) in expressions.into_iter().enumerate() {
        for row in 0..2 {
            let params = FaceParams { expression, mouth_open: row as f32, ..FaceParams::default() };
            render_rgb565_be(&mut frame, WIDTH, HEIGHT, params, 0).unwrap();
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let src = (y * WIDTH + x) * 2;
                    let c = u16::from_be_bytes([frame[src], frame[src + 1]]);
                    let dst = ((row * HEIGHT + y) * grid_width + column * WIDTH + x) * 3;
                    grid[dst] = ((c >> 11) * 255 / 31) as u8;
                    grid[dst + 1] = (((c >> 5) & 63) as u8 as u16 * 255 / 63) as u8;
                    grid[dst + 2] = ((c & 31) * 255 / 31) as u8;
                }
            }
        }
    }
    let mut file = File::create(&output)?;
    write!(file, "P6\n{} {}\n255\n", grid_width, HEIGHT * 2)?;
    file.write_all(&grid)?;
    println!("{}", output.display());
    Ok(())
}
