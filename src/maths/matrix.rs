use crate::maths::vector;
use libm;
use libm::{cosf, sinf, tanf};

#[derive(Copy, Clone)]
pub struct Matrix<T, const ROW: usize, const COLUMN: usize>([[T; ROW]; COLUMN]);

impl<T: Default + Copy, const ROW: usize, const COLUMN: usize> Default for Matrix<T, ROW, COLUMN>
{
    fn default() -> Self { Matrix([[T::default(); ROW]; COLUMN]) }
}

pub type SquareMatrix<T, const SIZE: usize> = Matrix<T, SIZE, SIZE>;

impl<T: Default + Copy, const SIZE: usize> SquareMatrix<T, SIZE>
{
    pub fn identity(one: T) -> Self
    {
        let mut matrix: SquareMatrix<T, SIZE> = SquareMatrix::default();
        for i in 0..SIZE {
            matrix.0[i][i] = one;
        }
        matrix
    }
}

pub type Matrix4f = SquareMatrix<f32, 4>;

impl Matrix4f
{
    pub fn translation_matrix(translate: vector::Vector3f) -> Self
    {
        Self([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [translate.x(), translate.y(), translate.z(), 1.0],
        ])
    }

    pub fn rotation_around_z_axis(angle: f32) -> Self
    {
        Self([
            [cosf(angle), -sinf(angle), 0.0, 0.0],
            [sinf(angle), cosf(angle), 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    pub fn projection_matrix(vertical_fov: f32, horizontal_fov: f32, aspect_ratio: f32) -> Self
    {
        let d_height = 1.0 / tanf(vertical_fov * (std::f32::consts::PI / 180.0) * 0.5);
        let d_width = 1.0 / tanf(horizontal_fov * (std::f32::consts::PI / 180.0) * 0.5);
        Self([
            [d_height, 0.0, 0.0, 0.0],
            [0.0, d_width, 0.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
        ])
    }
}
