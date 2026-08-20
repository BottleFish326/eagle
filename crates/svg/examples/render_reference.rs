use std::env;
use std::fs;
use std::path::PathBuf;

use asset_svg::render_svg_file;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let source = PathBuf::from(arguments.next().ok_or("missing SVG source path")?);
    let output = PathBuf::from(arguments.next().ok_or("missing PNG output path")?);
    if arguments.next().is_some() {
        return Err("usage: render_reference <source.svg> <output.png>".into());
    }
    let rendered = render_svg_file(&source, 2_048)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, rendered.bytes)?;
    println!("{}x{}", rendered.width, rendered.height);
    Ok(())
}
