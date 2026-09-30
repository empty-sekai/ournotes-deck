fn main() {
    let path = std::env::args().nth(1).expect("one JSON input path required");
    let bytes = std::fs::read(path).unwrap();
    match recommendation_portability_harness::evaluate_json(&bytes) {
        Ok(result) => {
            use std::io::Write;
            std::io::stdout().write_all(&result).unwrap();
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}

