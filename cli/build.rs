use std::{
    fs::File,
    io::{self, Write},
};

fn main() -> io::Result<()> {
    let mut file = File::create("document.txt")?;

    file.write_all(b"Hello world")?;

    Ok(())
}
