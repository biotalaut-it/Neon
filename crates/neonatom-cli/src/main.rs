use neonatom::{
    runtime::eval,
    syntax::{parse, Limits},
    value::Value,
};
use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

fn read_source(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if bytes.len() > 1024 * 1024 {
        return Err("source exceeds 1 MiB limit".into());
    }
    String::from_utf8(bytes).map_err(|e| format!("source is not UTF-8: {e}"))
}
fn execute(args: &[String]) -> Result<(), String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err("usage: neonatom <run|check|emit> <file> [--limit N]".into());
    };
    let path = args
        .get(1)
        .ok_or_else(|| "missing source file".to_string())?;
    let source = read_source(Path::new(path))?;
    let program = parse(&source).map_err(|e| e.to_string())?;
    if command == "check" {
        println!("ok");
        return Ok(());
    }
    let limit = if command == "emit" {
        if args.get(2).map(String::as_str) != Some("--limit") {
            return Err("emit requires --limit N".into());
        }
        args.get(3)
            .ok_or_else(|| "emit requires a numeric limit".to_string())?
            .parse::<usize>()
            .map_err(|_| "invalid emit limit".to_string())?
    } else if command == "run" {
        20
    } else {
        return Err(format!("unknown command: {command}"));
    };
    let value = eval(&program, Limits::default()).map_err(|e| e.to_string())?;
    match value {
        Value::Stream(mut stream) => {
            let mut out = io::BufWriter::new(io::stdout().lock());
            let mut bytes = 0usize;
            let mut n = 0usize;
            while n < limit {
                match stream.next() {
                    None => break,
                    Some(Err(e)) => return Err(e.to_string()),
                    Some(Ok(v)) => {
                        let line = format!("{v}\n");
                        bytes = bytes.saturating_add(line.len());
                        if bytes > Limits::default().max_output_bytes {
                            return Err("output byte limit exceeded".into());
                        }
                        out.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
                        n += 1;
                    }
                }
            }
            out.flush().map_err(|e| e.to_string())?;
        }
        value => println!("{value}"),
    }
    Ok(())
}
fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match execute(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("neonatom: {e}");
            ExitCode::from(2)
        }
    }
}
