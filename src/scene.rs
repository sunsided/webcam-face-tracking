//! The room behind the window and the objects in it, rebuilt as one triangle list per frame.
//!
//! Everything is generated on the CPU in world space: the scene is a few thousand vertices, so
//! re-uploading it each frame is cheaper than any instancing bookkeeping, and the index list
//! never changes, so it is uploaded once.

use bytemuck::{Pod, Zeroable};

use crate::projection::Vec3;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub pos: Vec3,
    pub normal: Vec3,
    /// Linear RGB, plus `a = 1` for surfaces that get the grid pattern.
    pub color: [f32; 4],
}

/// Room dimensions in meters. The opening is the window, the back wall sits at `z = -depth`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Room {
    pub half_w: f32,
    pub half_h: f32,
    pub depth: f32,
}

impl Room {
    /// A room that fits the window and is a bit deeper than it is wide.
    pub fn for_window(window_size: [f32; 2]) -> Self {
        Self {
            half_w: window_size[0] * 0.5,
            half_h: window_size[1] * 0.5,
            depth: window_size[0] * 1.1,
        }
    }

    /// Grid spacing for the wall pattern: ten cells across the window.
    pub fn grid(&self) -> f32 {
        self.half_w * 0.2
    }
}

#[derive(Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    fn quad(&mut self, p: [Vec3; 4], normal: Vec3, color: [f32; 4]) {
        let base = self.vertices.len() as u32;
        for pos in p {
            self.vertices.push(Vertex { pos, normal, color });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A box with half extents `h`, rotated by `rot` and moved to `center`.
    fn cuboid(&mut self, center: Vec3, h: Vec3, rot: Rot, color: [f32; 4]) {
        let corner =
            |sx: f32, sy: f32, sz: f32| add(center, rot.apply([sx * h[0], sy * h[1], sz * h[2]]));
        // Each face: a fixed axis and its sign, then two in-plane axes for the corners.
        let faces: [(Vec3, [[f32; 3]; 4]); 6] = [
            (
                [1.0, 0.0, 0.0],
                [[1., -1., -1.], [1., 1., -1.], [1., 1., 1.], [1., -1., 1.]],
            ),
            (
                [-1.0, 0.0, 0.0],
                [
                    [-1., -1., 1.],
                    [-1., 1., 1.],
                    [-1., 1., -1.],
                    [-1., -1., -1.],
                ],
            ),
            (
                [0.0, 1.0, 0.0],
                [[-1., 1., -1.], [-1., 1., 1.], [1., 1., 1.], [1., 1., -1.]],
            ),
            (
                [0.0, -1.0, 0.0],
                [
                    [-1., -1., 1.],
                    [-1., -1., -1.],
                    [1., -1., -1.],
                    [1., -1., 1.],
                ],
            ),
            (
                [0.0, 0.0, 1.0],
                [[-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]],
            ),
            (
                [0.0, 0.0, -1.0],
                [
                    [1., -1., -1.],
                    [-1., -1., -1.],
                    [-1., 1., -1.],
                    [1., 1., -1.],
                ],
            ),
        ];
        for (n, c) in faces {
            let p = c.map(|s| corner(s[0], s[1], s[2]));
            self.quad(p, rot.apply(n), color);
        }
    }

    fn sphere(&mut self, center: Vec3, r: f32, color: [f32; 4]) {
        const RINGS: u32 = 16;
        const SEGMENTS: u32 = 32;
        let base = self.vertices.len() as u32;
        for i in 0..=RINGS {
            let theta = std::f32::consts::PI * i as f32 / RINGS as f32;
            for j in 0..=SEGMENTS {
                let phi = std::f32::consts::TAU * j as f32 / SEGMENTS as f32;
                let n = [
                    theta.sin() * phi.cos(),
                    theta.cos(),
                    theta.sin() * phi.sin(),
                ];
                self.vertices.push(Vertex {
                    pos: add(center, scale(n, r)),
                    normal: n,
                    color,
                });
            }
        }
        let row = SEGMENTS + 1;
        for i in 0..RINGS {
            for j in 0..SEGMENTS {
                let a = base + i * row + j;
                let b = a + row;
                self.indices
                    .extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
}

/// Build the room and its objects at animation time `t` (seconds).
pub fn build(room: &Room, t: f32) -> Mesh {
    let mut m = Mesh::default();
    let Room {
        half_w: w,
        half_h: h,
        depth: d,
    } = *room;

    // Walls, normals pointing into the room; `a = 1` turns on the grid pattern.
    let wall = srgb(0xe8e2d6, 1.0);
    let floor = srgb(0xb9ad9a, 1.0);
    let back = srgb(0xd4dbe3, 1.0);
    m.quad(
        [[-w, -h, 0.0], [w, -h, 0.0], [w, -h, -d], [-w, -h, -d]],
        [0.0, 1.0, 0.0],
        floor,
    );
    m.quad(
        [[-w, h, -d], [w, h, -d], [w, h, 0.0], [-w, h, 0.0]],
        [0.0, -1.0, 0.0],
        wall,
    );
    m.quad(
        [[-w, -h, 0.0], [-w, -h, -d], [-w, h, -d], [-w, h, 0.0]],
        [1.0, 0.0, 0.0],
        wall,
    );
    m.quad(
        [[w, -h, -d], [w, -h, 0.0], [w, h, 0.0], [w, h, -d]],
        [-1.0, 0.0, 0.0],
        wall,
    );
    m.quad(
        [[-w, -h, -d], [w, -h, -d], [w, h, -d], [-w, h, -d]],
        [0.0, 0.0, 1.0],
        back,
    );

    // A pedestal with a ball on it, back left.
    let ped_h = [w * 0.14, h * 0.32, w * 0.14];
    let ped = [-w * 0.5, -h + ped_h[1], -d * 0.62];
    m.cuboid(ped, ped_h, Rot::IDENTITY, srgb(0xc9714f, 0.0));
    let ball_r = w * 0.12;
    m.sphere(
        [ped[0], ped[1] + ped_h[1] + ball_r, ped[2]],
        ball_r,
        srgb(0xf2c14e, 0.0),
    );

    // A slim pillar, back right.
    let pil_h = [w * 0.06, h * 0.7, w * 0.06];
    m.cuboid(
        [w * 0.62, -h + pil_h[1], -d * 0.82],
        pil_h,
        Rot::y(0.5),
        srgb(0x5b7db1, 0.0),
    );

    // A tumbling cube in the middle of the room.
    let s = h * 0.17;
    let rot = Rot::y(t * 0.4).then(Rot::x(t * 0.27));
    let cube = [w * 0.05, h * 0.12, -d * 0.42];
    m.cuboid(cube, [s, s, s], rot, srgb(0x6fae8f, 0.0));

    // A moon orbiting the cube, so a shadow sweeps across the walls.
    let a = t * 0.9;
    let orbit = [
        a.cos() * w * 0.32,
        (a * 0.7).sin() * h * 0.25,
        a.sin() * d * 0.22,
    ];
    m.sphere(add(cube, orbit), w * 0.05, srgb(0xe7e3f0, 0.0));

    // A ball in front of the window, so it seems to float out of the screen.
    m.sphere(
        [w * 0.42, -h * 0.45, d * 0.12],
        w * 0.07,
        srgb(0xd9534f, 0.0),
    );

    m
}

/// A rotation as three column vectors.
#[derive(Copy, Clone, Debug)]
struct Rot([Vec3; 3]);

impl Rot {
    const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    fn x(a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self([[1.0, 0.0, 0.0], [0.0, c, s], [0.0, -s, c]])
    }

    fn y(a: f32) -> Self {
        let (s, c) = a.sin_cos();
        Self([[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]])
    }

    /// `self` applied first, then `other`.
    fn then(self, other: Self) -> Self {
        Self(self.0.map(|col| other.apply(col)))
    }

    fn apply(&self, v: Vec3) -> Vec3 {
        let [a, b, c] = self.0;
        [
            a[0] * v[0] + b[0] * v[1] + c[0] * v[2],
            a[1] * v[0] + b[1] * v[1] + c[1] * v[2],
            a[2] * v[0] + b[2] * v[1] + c[2] * v[2],
        ]
    }
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `0xRRGGBB` sRGB to linear RGB, with the pattern flag in alpha.
fn srgb(hex: u32, pattern: f32) -> [f32; 4] {
    let ch = |shift: u32| {
        let c = ((hex >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [ch(16), ch(8), ch(0), pattern]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_is_stable_over_time() {
        let room = Room::for_window([0.34, 0.19]);
        let a = build(&room, 0.0);
        let b = build(&room, 12.3);
        assert_eq!(a.vertices.len(), b.vertices.len());
        assert_eq!(a.indices, b.indices);
        assert!(a.indices.iter().all(|&i| (i as usize) < a.vertices.len()));
    }

    #[test]
    fn cube_face_normals_point_outward() {
        let mut m = Mesh::default();
        let center = [0.1, 0.2, -0.3];
        m.cuboid(
            center,
            [0.1, 0.2, 0.3],
            Rot::y(0.7).then(Rot::x(0.3)),
            [1.0; 4],
        );
        for quad in m.vertices.chunks(4) {
            let mid = quad
                .iter()
                .fold([0.0; 3], |acc, v| add(acc, scale(v.pos, 0.25)));
            let out = [mid[0] - center[0], mid[1] - center[1], mid[2] - center[2]];
            let n = quad[0].normal;
            assert!(n[0] * out[0] + n[1] * out[1] + n[2] * out[2] > 0.0);
        }
    }
}
