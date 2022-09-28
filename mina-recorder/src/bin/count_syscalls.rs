use std::collections::BTreeMap;

fn main() {
    use std::{env, fs::File, io::{self, BufRead}};

    let filename = env::args().nth(1).unwrap();
    let file = File::open(filename).unwrap();
    let mut counters = BTreeMap::new();
    for line in io::BufReader::new(file).lines() {
        let line = line.unwrap();
        if let Some(word) = line.split_whitespace().nth(2) {
            if let Some(word) = word.split('(').next() {
                const ALPHABET: &'static str = "abcdefghijklmnopqrstuvwxyz_";
                if ALPHABET.chars().find(|c| *c == word.chars().nth(0).unwrap()).is_some() {
                    *counters.entry(word.to_owned()).or_insert_with(|| 0usize) += 1;
                }
            }
        }
    }
    let mut counters = counters.into_iter().collect::<Vec<_>>();
    counters.sort_by(|(_, x), (_, y)| x.cmp(y).reverse());
    for (syscall, count) in counters {
        println!("{syscall}: {count}");
    }
}
