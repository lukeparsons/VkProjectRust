pub type Matrix4f = SquareMatrix<f32, 4>;

pub struct Matrix<T, const ROW: usize, const COLUMN: usize>
{
    values: [[T; ROW]; COLUMN],
}

impl<T: Default + Copy, const ROW: usize, const COLUMN: usize> Default for Matrix<T, ROW, COLUMN>
{
    fn default() -> Self { Matrix { values: [[T::default(); ROW]; COLUMN] } }
}

pub type SquareMatrix<T, const SIZE: usize> = Matrix<T, SIZE, SIZE>;

impl<T: Default + Copy, const SIZE: usize> SquareMatrix<T, SIZE>
{
    pub fn identity(one: T) -> Self
    {
        let mut matrix: SquareMatrix<T, SIZE> = SquareMatrix::default();
        for i in 0..SIZE {
            matrix.values[i][i] = one;
        }
        matrix
    }
}
