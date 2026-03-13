fn id<T>(x: magic) -> magic {
    return x;
}

fn add(a: magic, b: magic) -> magic {
    return a + b;
}

fn main() -> magic {
    let x: magic = 10;
    let y: magic = id(x);
    let z: magic = add(y, 32);
    eprintln!("test {} {} {}", x, y, z);
    return z;
}
