use gitrevset::Expr;
use gitrevset::Repo;
use gitrevset::Result;
use std::env;

fn try_main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut print_ast = false;
    let mut repo: Option<Repo> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--ast" => {
                print_ast = true;
                i += 1;
            }
            "--open" => {
                let path = args.get(i + 1).expect("--open requires a path argument");
                repo = Some(Repo::open(path)?);
                i += 2;
            }
            "--clone" => {
                let url = args.get(i + 1).expect("--clone requires a URL argument");
                let path = args.get(i + 2).expect("--clone requires a path argument");
                repo = Some(Repo::clone(url, path)?);
                i += 3;
            }
            "--fetch" => {
                let remote = args
                    .get(i + 1)
                    .map(|s| s.as_str())
                    .unwrap_or("origin");
                let mut r = repo.take().unwrap_or_else(|| {
                    Repo::open_from_env().expect("failed to open repo")
                });
                r.fetch(remote)?;
                repo = Some(r);
                i += 2;
            }
            arg => {
                if repo.is_none() {
                    repo = Some(Repo::open_from_env()?);
                }
                if print_ast {
                    let ast = Expr::parse(arg)?;
                    println!("{:?}", ast);
                } else {
                    let repo = repo.as_ref().unwrap();
                    let set = repo.anyrevs(arg)?;
                    for v in set.iter()? {
                        println!("{}", v?.to_hex());
                    }
                }
                i += 1;
            }
        }
    }
    Ok(())
}

fn main() {
    match try_main() {
        Ok(()) => (),
        Err(e) => eprintln!("{}", e),
    }
}
