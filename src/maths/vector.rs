#[derive(Copy, Clone)]
pub struct Vector<T, const SIZE: usize>([T; SIZE]);

impl<T, const SIZE: usize> Vector<T, SIZE>
{
    pub fn new(values: [T; SIZE]) -> Self { Vector(values) }
}

impl<T: Copy> Vector<T, 3>
{
    pub fn x(&self) -> T { self.0[0] }
    pub fn y(&self) -> T { self.0[1] }
    pub fn z(&self) -> T { self.0[2] }
}

pub type Vector3f = Vector<f32, 3>;
