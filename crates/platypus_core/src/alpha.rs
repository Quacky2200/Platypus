// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Alpha bleeding: colours for fully transparent pixels.

/// Gives every fully transparent pixel (alpha 0) the average colour of its nearest
/// non-transparent neighbours, up to `radius` pixels away. Art tools leave arbitrary
/// colour (often white) in transparent pixels; filtering and mip generation blend it into
/// the edge of the shape as a pale halo. Unity does the same on import ("alpha is
/// transparency"). Alpha is left alone, so nothing visible changes except the halo.
///
/// `rgba` is `width * height` pixels of straight RGBA8.
pub fn bleed_transparent(rgba: &mut [u8], width: usize, height: usize, radius: usize) {
    if width == 0 || height == 0 || rgba.len() != width * height * 4 {
        return;
    }
    // Pixels that carry a real colour: anything not fully transparent.
    let mut known: Vec<bool> = rgba.as_chunks::<4>().0.iter().map(|p| p[3] != 0).collect();
    let neighbours = |i: usize| {
        let (x, y) = ((i % width) as isize, (i / width) as isize);
        (-1isize..=1).flat_map(move |dy| {
            (-1isize..=1).filter_map(move |dx| {
                let (nx, ny) = (x + dx, y + dy);
                let inside = (dx, dy) != (0, 0)
                    && nx >= 0
                    && ny >= 0
                    && (nx as usize) < width
                    && (ny as usize) < height;
                inside.then(|| ny as usize * width + nx as usize)
            })
        })
    };
    // The transparent pixels touching a known one: they get colour first.
    let mut frontier: Vec<usize> = (0..known.len())
        .filter(|&i| !known[i] && neighbours(i).any(|n| known[n]))
        .collect();
    let mut queued = vec![false; known.len()];
    for &i in &frontier {
        queued[i] = true;
    }
    for _ in 0..radius {
        if frontier.is_empty() {
            break;
        }
        // Colours come from the pixels known before this pass only, so a pass spreads one pixel.
        let colours: Vec<[u8; 3]> = frontier
            .iter()
            .map(|&i| {
                let (mut sum, mut count) = ([0u32; 3], 0u32);
                for n in neighbours(i).filter(|&n| known[n]) {
                    for c in 0..3 {
                        sum[c] += u32::from(rgba[n * 4 + c]);
                    }
                    count += 1;
                }
                [0, 1, 2].map(|c| ((sum[c] + count / 2) / count) as u8)
            })
            .collect();
        for (&i, colour) in frontier.iter().zip(&colours) {
            rgba[i * 4..i * 4 + 3].copy_from_slice(colour);
        }
        for &i in &frontier {
            known[i] = true;
        }
        let mut next = Vec::new();
        for &i in &frontier {
            for n in neighbours(i) {
                if !known[n] && !queued[n] {
                    queued[n] = true;
                    next.push(n);
                }
            }
        }
        frontier = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RDY-RND-02
    #[test]
    fn transparent_pixels_take_the_colour_of_the_shape_beside_them() {
        // Dark red, then three transparent white pixels.
        let mut px = vec![
            100, 0, 0, 255, //
            255, 255, 255, 0, //
            255, 255, 255, 0, //
            255, 255, 255, 0,
        ];
        bleed_transparent(&mut px, 4, 1, 2);
        assert_eq!(&px[4..7], &[100, 0, 0]);
        assert_eq!(&px[8..11], &[100, 0, 0]);
        // Out of reach (radius 2): untouched.
        assert_eq!(&px[12..15], &[255, 255, 255]);
        // Alpha never changes, and a partly transparent pixel keeps its colour.
        assert_eq!([px[7], px[11], px[15]], [0, 0, 0]);
        let mut half = vec![10, 20, 30, 128, 255, 255, 255, 0];
        bleed_transparent(&mut half, 2, 1, 4);
        assert_eq!(half, vec![10, 20, 30, 128, 10, 20, 30, 0]);
    }

    // RDY-RND-02
    #[test]
    fn a_pixel_between_two_colours_gets_their_average_and_bad_sizes_are_ignored() {
        let mut px = vec![200, 0, 0, 255, 9, 9, 9, 0, 0, 100, 0, 255];
        bleed_transparent(&mut px, 3, 1, 1);
        assert_eq!(&px[4..7], &[100, 50, 0]);
        let mut short = vec![1, 2, 3, 0];
        bleed_transparent(&mut short, 2, 2, 3);
        assert_eq!(short, vec![1, 2, 3, 0]);
    }
}
