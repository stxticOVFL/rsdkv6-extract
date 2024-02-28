use dunce;
use json;
use md5;
use sqlite::State;
use std::collections::HashMap;
use std::env;
use std::fs::{create_dir_all, File};
use std::io::{self, BufRead, Read, Seek, Write};
use std::path::Path;

fn compute_md5<T: AsRef<[u8]>>(data: &T) -> md5::Digest {
    let digest = md5::compute(data);
    /*let mut flip: md5::Digest = digest;

    digest
        .into_iter()
        .enumerate()
        .map(|(i, _)| i)
        .step_by(4)
        .for_each(|i| {
            flip.0[i + 0] = digest.0[i + 3];
            flip.0[i + 1] = digest.0[i + 2];
            flip.0[i + 2] = digest.0[i + 1];
            flip.0[i + 3] = digest.0[i + 0];
        });
    */

    digest
}

// https://doc.rust-lang.org/rust-by-example/std_misc/file/read_lines.html
fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

macro_rules! output {
    ($file:ident) => {{
        println!();
        ($file).write(b"\n").unwrap();
    }};
    ($file:ident, $($t:tt)*) => {{
        println!($($t)*);
        $file.write(format!($($t)*).as_bytes()).unwrap();
        $file.write(b"\n").unwrap();
    }};
}

fn main() {
    let mut out = File::options()
        .write(true)
        .truncate(true)
        .create(true)
        .open("out.txt")
        .unwrap();
    let mut has_packs = false;
    match env::args().len() {
        3 => (),
        4 => has_packs = true,
        _ => {
            println!("usage: rsdkv6-extract [pack.db] [file list] (pack name list)");
            return;
        }
    }

    let args: Vec<String> = env::args().collect();

    let mut hash_map = HashMap::new();
    let lines: io::Lines<io::BufReader<File>> = read_lines(&args[2]).unwrap();

    for line in lines.flatten() {
        let lower = line.to_lowercase();
        let digest = compute_md5(&lower);
        let digest_str = format!("{:x}", digest);
        hash_map.insert(digest_str, (line, false));
    }

    let pack_db =
        sqlite::Connection::open_with_flags(&args[1], sqlite::OpenFlags::new().with_read_only())
            .unwrap();

    let query = "
        SELECT files.path,
                files.pack,
                files.offset,
                files.size,
                packs.name as [packname]
        FROM files
            INNER JOIN
            packs ON packs.id = files.pack
        ORDER BY files.pack
    ";
    let mut statement = pack_db.prepare(query).unwrap();

    let mut max_check = pack_db.prepare("SELECT COUNT(id) FROM files").unwrap();
    max_check.next().unwrap();
    let max = max_check.read::<i64, _>(0).unwrap();

    let mut new = 0;
    let mut hits = 0;
    let mut current_data = 1;

    let basepack = dunce::canonicalize(Path::new(&args[1])).unwrap();
    let packpath = basepack.parent().unwrap().display().to_string();

    let path = format!("{}/Data001.rsdk", packpath);

    let mut datapack = std::fs::File::open(path).unwrap();

    while let Ok(State::Row) = statement.next() {
        let read_data = statement.read::<i64, _>("pack").unwrap();
        if read_data != current_data {
            current_data = read_data;
            datapack =
                std::fs::File::open(format!("{}/Data{:0>3}.rsdk", packpath, current_data)).unwrap();
        }

        let key = statement.read::<String, _>("path").unwrap();
        let mut filename = format!("MISSING/{}", key);
        match hash_map.get_mut(key.as_str()) {
            Some((name, used)) => {
                hits += 1;
                filename = name.clone();
                *used = true;
            }
            None => (),
        }

        datapack
            .seek(io::SeekFrom::Start(
                (statement.read::<i64, _>("offset").unwrap() + 0x30) as u64,
            ))
            .unwrap();
        let mut buf = vec![0; statement.read::<i64, _>("size").unwrap() as usize];
        match datapack.read_exact(&mut buf) {
            Ok(_) => (),
            Err(error) => {
                output!(out, "{} - {} - ERROR: {}", key, filename, error);
                continue;
            }
        }

        if filename.starts_with("MISSING") {
            filename += format!("@{}", current_data).as_str();
            // try to guess from the first 4 letters
            let header = &buf[..4];
            let mut matched = true;
            match header {
                b"MThd" => filename += ".mid",
                b"\x1F\x8B\x08\x08" => filename += ".pvr.gz",
                b"GPU\x00" => filename += ".bin.gpu",
                b"PAL\x00" => filename += ".bin.pal",
                b"MDL\x01" => filename += ".bin.mdl",
                b"MDL\x02" => filename += ".bin.mdl",
                b"LYR\x01" => filename += ".bin.lyr",
                b"LYR\x02" => filename += ".bin.lyr",
                b"RIFF" => filename += ".wav",
                b"OggS" => filename += ".ogg",
                b"SQLi" => filename += ".db",
                b"ANI\x00" => filename += ".bin.ani",
                b"SPR\x01" => filename += ".bin.spr",
                b"VFX\x00" => filename += ".bin.vfx",
                b"DKIF" => filename += ".ivf",
                b"COM\x00" => filename += ".bin.com",
                _ => matched = false,
            }

            if !matched {
                match json::parse(std::str::from_utf8(&buf).unwrap_or_default()) {
                    Ok(_) => filename += ".cfg",
                    _ => (),
                }
            }
        }

        output!(out, "{} - {} @ {}", key, filename, current_data);

        let path = std::path::Path::new(&filename).parent().unwrap();
        std::fs::create_dir_all(path).unwrap();

        if !Path::new(&filename).exists() {
            new += 1;
            std::fs::File::create(filename)
                .unwrap()
                .write_all(&mut buf)
                .unwrap();
        }
    }

    output!(out);

    hash_map.retain(|key, (name, used)| {
        if !*used {
            output!(out, "{} - {} - UNUSED", key, name);
        }
        *used
    });

    output!(out);

    println!(
        //out,
        "{}/{}/{} - {:.2}% / {:.2}% (+{})",
        hits,
        hash_map.len(),
        max,
        hits as f64 / hash_map.len() as f64 * 100.0,
        hits as f64 / max as f64 * 100.0,
        new
    );
}
