pub fn parse_addr(input: &[u8]) -> String {
    let mut acc = String::new();
    let mut input = input;
    while !input.is_empty() {
        match multiaddr::Protocol::from_bytes(input) {
            Ok((p, i)) => {
                input = i;
                acc = format!("{acc}{p}");
            }
            Err(err) => {
                input = &[];
                acc = format!("{acc}{err}");
            }
        }
    }
    acc
}
