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

fn main() {
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

    let pack_db = sqlite::open(&args[1]).unwrap();
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

    let mut datapack = std::fs::File::open("Data001.rsdk").unwrap();

    while let Ok(State::Row) = statement.next() {
        let read_data = statement.read::<i64, _>("pack").unwrap();
        if read_data != current_data {
            current_data = read_data;
            datapack = std::fs::File::open(format!("Data{:0>3}.rsdk", current_data)).unwrap();
        }

        let key = statement.read::<String, _>("path").unwrap();
        let mut filename = format!("MISSING/{}", key);
        match hash_map.get_mut(key.as_str()) {
            Some((name, used)) => {
                hits += 1;
                filename = name.clone();
                *used = true;
                //println!("{} - {} @ {}", key, name, current_data);
            }
            None => (), //println!("{} - MISSING @ {}", key, current_data),
        }

        datapack
            .seek(io::SeekFrom::Start(
                (statement.read::<i64, _>("offset").unwrap() + 0x30) as u64,
            ))
            .unwrap();
        let mut buf = vec![0; statement.read::<i64, _>("size").unwrap() as usize];
        match datapack.read_exact(&mut buf) {
            Ok(_) => (),
            Err(error) => println!("{} - {} - ERROR: {}", key, filename, error),
        }

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
    println!();

    for (key, (name, used)) in hash_map.iter() {
        if !used {
            println!("{} - {} - UNUSED", key, name);
        }
    }

    println!();

    println!(
        "{}/{}/{} - {:.2}% / {:.2}% (+{})",
        hits,
        hash_map.len(),
        max,
        hits as f64 / hash_map.len() as f64 * 100.0,
        hits as f64 / max as f64 * 100.0,
        new
    );
}
