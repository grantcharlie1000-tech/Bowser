//! Perceptual matching of known GTA VI screenshots and official video thumbnails.
//! The catalogue stores fingerprints only. Every comparison runs on the existing image worker.

use std::sync::OnceLock;

use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};

const SIDE: usize = 72;
const GRID: usize = 9;
const MAX_PIXELS: u64 = 25_000_000;
type Fingerprint = (u64, u64);

fn references() -> &'static [Fingerprint] {
    static REFERENCES: OnceLock<Vec<Fingerprint>> = OnceLock::new();
    REFERENCES.get_or_init(|| {
        include_str!("visual-references.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let (horizontal, vertical) = line.split_once(' ')?;
                Some((u64::from_str_radix(horizontal, 16).ok()?, u64::from_str_radix(vertical, 16).ok()?))
            })
            .collect()
    })
}

pub(super) fn matches(decoder: &BitmapDecoder, width: u32, height: u32) -> bool {
    if width < super::MIN_WIDTH || height < super::MIN_HEIGHT || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return false;
    }
    let Ok(sample) = read_luma(decoder) else { return false };
    if matches_reference(fingerprint(&sample, 0, 0)) {
        return true;
    }
    // YouTube's 4:3 thumbnails can have a 16:9 frame. Dark scenes often make the
    // black bars look uneven, so check the fixed 16:9 crop when both edges are dark.
    let (top, bottom) = dark_edges(&sample, false);
    let letterbox = SIDE / 8;
    if width * 3 == height * 4 && top.min(bottom) >= letterbox - 1
        && matches_reference(fingerprint(&sample, 0, letterbox))
    {
        return true;
    }
    let (left, right) = dark_edges(&sample, true);
    let x = paired_inset(left, right);
    let y = paired_inset(top, bottom);
    (x != 0 || y != 0) && matches_reference(fingerprint(&sample, x, y))
}

fn read_luma(decoder: &BitmapDecoder) -> windows::core::Result<[u8; SIDE * SIDE]> {
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(SIDE as u32)?;
    transform.SetScaledHeight(SIDE as u32)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Ignore,
            &transform,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .join()?
        .DetachPixelData()?;
    let mut luma = [0; SIDE * SIDE];
    for (v, bgra) in luma.iter_mut().zip(pixels.chunks_exact(4)) {
        *v = ((77 * u16::from(bgra[2]) + 150 * u16::from(bgra[1]) + 29 * u16::from(bgra[0])) >> 8) as u8;
    }
    Ok(luma)
}

fn dark_edges(sample: &[u8; SIDE * SIDE], columns: bool) -> (usize, usize) {
    let dark = |line| {
        (0..SIDE)
            .filter(|&i| sample[if columns { i * SIDE + line } else { line * SIDE + i }] < 24)
            .count() >= SIDE * 9 / 10
    };
    let first = (0..SIDE / 4).take_while(|&i| dark(i)).count();
    let last = (0..SIDE / 4).take_while(|&i| dark(SIDE - 1 - i)).count();
    (first, last)
}

fn paired_inset(first: usize, last: usize) -> usize {
    if first.min(last) >= 3 && first.abs_diff(last) <= 2 { first.min(last) } else { 0 }
}

fn fingerprint(sample: &[u8; SIDE * SIDE], inset_x: usize, inset_y: usize) -> Fingerprint {
    let mut cells = [[0; GRID]; GRID];
    let cell = |i, inset| inset + i * (SIDE - 2 * inset) / GRID;
    for (y, row) in cells.iter_mut().enumerate() {
        let top = cell(y, inset_y);
        let bottom = cell(y + 1, inset_y);
        for (x, value) in row.iter_mut().enumerate() {
            let left = cell(x, inset_x);
            let right = cell(x + 1, inset_x);
            let sum: usize = (top..bottom)
                .flat_map(|y| &sample[y * SIDE + left..y * SIDE + right])
                .map(|&v| usize::from(v))
                .sum();
            *value = sum * 256 / ((right - left) * (bottom - top));
        }
    }
    let mut horizontal = 0;
    let mut vertical = 0;
    for y in 0..GRID - 1 {
        for x in 0..GRID - 1 {
            horizontal = horizontal << 1 | u64::from(cells[y][x] > cells[y][x + 1]);
            vertical = vertical << 1 | u64::from(cells[y][x] > cells[y + 1][x]);
        }
    }
    (horizontal, vertical)
}

fn matches_reference((horizontal, vertical): Fingerprint) -> bool {
    if [horizontal, vertical].into_iter().any(|bits| !(8..=56).contains(&bits.count_ones())) {
        return false;
    }
    references().iter().any(|&(h, v)| {
        let dh = (h ^ horizontal).count_ones();
        let dv = (v ^ vertical).count_ones();
        dh <= 8 && dv <= 8 && dh + dv <= 12
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_visuals_match_with_small_encoding_differences() {
        assert!(references().len() >= 200);
        let &(h, v) = references().first().unwrap();
        assert!(matches_reference((h, v)));
        assert!(matches_reference((h ^ 0xf, v ^ 0xf)));
        assert!(!matches_reference((h ^ 0xffff, v ^ 0xffff)));
        assert!(!matches_reference((0, 0)));
    }

    #[test]
    fn paired_black_bars_are_removed_without_cropping_a_single_dark_edge() {
        let mut sample = [100; SIDE * SIDE];
        sample[..8 * SIDE].fill(0);
        assert_eq!(dark_edges(&sample, false), (8, 0));
        assert_eq!(paired_inset(8, 0), 0);
        sample[(SIDE - 8) * SIDE..].fill(0);
        assert_eq!(dark_edges(&sample, false), (8, 8));
        assert_eq!(paired_inset(8, 8), 8);
        assert_eq!(dark_edges(&sample, true), (0, 0));
    }
}
